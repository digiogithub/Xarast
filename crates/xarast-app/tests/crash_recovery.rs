//! Phase 12 acceptance criteria 12 and 13 (XARA-US-0064): a forced panic
//! on the render thread writes a crash report and an autosave of the
//! modified document, and the next start offers recovery and safe mode;
//! three crashes within ten minutes make the next start safe without
//! asking.
//!
//! The crashing session is a real process: this test binary runs itself
//! again with `XARAST_FORCE_PANIC=render`, and the child's `child_session`
//! test plays the application. The parent then plays the next start, so
//! the sentinel and the autosave see a process that is really gone.

mod crash_recovery {
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::Arc;
    use std::sync::mpsc;
    use std::time::{Duration, SystemTime};

    use xarast_app::autosave::AutosavePolicy;
    use xarast_app::crash::{self, FatalWatch, SafeMode};
    use xarast_app::{
        AppState, Changed, DocumentId, FrameJob, Intent, PlatformRequest, PromptAnswer,
        RenderThread,
    };
    use xarast_doc::NodeKind;

    /// Set in the child: the state directory it plays in.
    const CHILD_ENV: &str = "XARAST_CRASH_TEST_STATE";
    /// A directory name no report may mention.
    const PRIVATE: &str = "private-dir-7c1e";

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "xarast-crash-recovery-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn policy() -> AutosavePolicy {
        AutosavePolicy {
            interval: Duration::ZERO,
            idle: Duration::ZERO,
        }
    }

    /// Runs `child_session` in a new process that panics on its render
    /// thread; returns its exit code.
    fn crash_once(state: &Path) -> Option<i32> {
        let out = Command::new(std::env::current_exe().expect("test binary"))
            .args(["crash_recovery::child_session", "--exact", "--nocapture"])
            .env(CHILD_ENV, state)
            .env(crash::FORCE_PANIC_ENV, "render")
            .output()
            .expect("the child runs");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("crash report written to"),
            "the child's stderr:\n{stderr}"
        );
        out.status.code()
    }

    /// The application, in the child: a modified document, a render
    /// thread that panics on its first frame, and the shell's answer to a
    /// fatal panic (autosave, exit 101).
    #[test]
    fn child_session() {
        let Some(state) = std::env::var_os(CHILD_ENV).map(PathBuf::from) else {
            return; // Only ever run as the child.
        };
        let (_check, _sentinel) = crash::begin_session(&state, false, SystemTime::now());
        crash::install_panic_hook(state.join("crashes"));
        crash::set_context("gpu_adapter", "test adapter");
        let private = state.join(PRIVATE).join("work.xarast");
        crash::record_log_line(&format!("opened {}", private.display()));

        let mut app = AppState::new().with_autosave(state.join("autosave"), policy());
        app.new_document();
        let s = app.active().expect("a document");
        let layer = s
            .doc
            .tree
            .preorder(s.doc.tree.root())
            .find(|id| matches!(s.doc.tree.kind(*id), Some(NodeKind::Layer(_))))
            .expect("a layer");
        let c = app
            .apply(Intent::SetLayerVisible {
                layer,
                visible: false,
            })
            .expect("edit");
        assert!(c.contains(Changed::DOCUMENT));

        let (tx, rx) = mpsc::channel();
        let watch = FatalWatch::global();
        watch.set_waker(Box::new(move || {
            let _ = tx.send(());
        }));
        let mut render = RenderThread::spawn(Box::new(|| {})).expect("a render thread");
        let view = xarast_render::ViewParams {
            viewport: xarast_render::DeviceRect::from_size(8, 8),
            ..xarast_render::ViewParams::default()
        };
        render.submit(FrameJob {
            doc: DocumentId(1),
            scene: Arc::new(xarast_render::Scene::new()),
            scene_epoch: 1,
            ink: view.viewport,
            resolver: Arc::new(xarast_render::Resolver::new()),
            view,
            background: [0, 0, 0, 255],
            page: None,
            generation: 0,
            cpu_rescale: true,
        });
        rx.recv_timeout(Duration::from_secs(30))
            .expect("the render thread's panic raised the watch");
        assert_eq!(watch.thread().as_deref(), Some("xarast-render"));
        assert_eq!(app.emergency_shutdown(), 1);
        std::process::exit(i32::from(crash::PANIC_EXIT_CODE));
    }

    #[test]
    fn a_render_panic_reports_autosaves_and_the_next_start_offers_recovery_and_safe_mode() {
        let state = scratch("render");
        assert_eq!(crash_once(&state), Some(101));

        // The report: where the panic was, what the shell registered, and
        // nothing of the document or its folder.
        let reports: Vec<PathBuf> = std::fs::read_dir(state.join("crashes"))
            .expect("a crash directory")
            .flatten()
            .map(|e| e.path())
            .collect();
        assert_eq!(reports.len(), 1, "{reports:?}");
        let text = std::fs::read_to_string(&reports[0]).expect("the report");
        assert!(text.contains("thread = \"xarast-render\""), "{text}");
        assert!(text.contains(crash::FORCE_PANIC_ENV), "{text}");
        assert!(text.contains("gpu_adapter = \"test adapter\""), "{text}");
        assert!(text.contains("…/work.xarast"), "{text}");
        assert!(!text.contains(PRIVATE), "a folder leaked:\n{text}");
        assert!(!text.contains(&*state.to_string_lossy()), "{text}");

        // The next start.
        let (check, sentinel) = crash::begin_session(&state, false, SystemTime::now());
        assert_eq!(check.crashed, 1);
        assert_eq!(check.safe_mode, SafeMode::Offered);
        assert_eq!(check.report.as_deref(), Some(reports[0].as_path()));
        let mut app = AppState::new()
            .with_autosave(state.join("autosave"), policy())
            .with_startup_check(&check);
        assert_eq!(app.recoverable().len(), 1, "the autosave is recoverable");
        assert!(!app.safe_mode());

        app.offer_recovery();
        let p = app.prompt().expect("the crash is reported first").clone();
        assert_eq!(p.title, "Xarast closed unexpectedly");
        assert!(p.message.contains(&*reports[0].to_string_lossy()));
        assert!(p.offers(PromptAnswer::SafeMode) && p.offers(PromptAnswer::CopyReport));

        app.apply(Intent::AnswerPrompt(PromptAnswer::CopyReport))
            .unwrap();
        assert_eq!(
            app.take_requests(),
            [PlatformRequest::SetClipboardText(
                reports[0].display().to_string()
            )]
        );
        assert!(app.prompt().is_some(), "still asking");

        app.apply(Intent::AnswerPrompt(PromptAnswer::SafeMode))
            .unwrap();
        assert!(app.safe_mode());
        assert_eq!(app.take_requests(), [PlatformRequest::EnterSafeMode]);
        let p = app.prompt().expect("then recovery").clone();
        assert!(p.offers(PromptAnswer::Recover), "{p:?}");
        app.apply(Intent::AnswerPrompt(PromptAnswer::Recover))
            .unwrap();
        let s = app.active().expect("recovered");
        assert!(s.is_modified());

        drop(app);
        if let Some(s) = sentinel {
            s.finish();
        }
        let _ = std::fs::remove_dir_all(&state);
    }

    #[test]
    fn three_crashes_in_ten_minutes_force_safe_mode_without_asking() {
        let state = scratch("loop");
        for _ in 0..3 {
            assert_eq!(crash_once(&state), Some(101));
        }
        let (check, sentinel) = crash::begin_session(&state, false, SystemTime::now());
        assert_eq!(check.recent_crashes, 3);
        assert_eq!(check.safe_mode, SafeMode::Forced);
        let mut app = AppState::new()
            .with_autosave(state.join("autosave"), policy())
            .with_startup_check(&check);
        assert!(app.safe_mode(), "on from the start");
        app.offer_recovery();
        let p = app.prompt().expect("it says why").clone();
        assert!(!p.offers(PromptAnswer::SafeMode), "not a question: {p:?}");
        assert!(p.message.contains("runs in safe mode"), "{}", p.message);
        if let Some(s) = sentinel {
            s.finish();
        }
        let _ = std::fs::remove_dir_all(&state);
    }
}
