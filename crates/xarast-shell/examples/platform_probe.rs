//! A window that reports every platform event and obeys commands on stdin.
//!
//! This is the harness for the compositor experiments (E1–E7 in
//! `docs/phases/phase-05-shell-and-ui.md`) and for the platform checks
//! that `ScriptedSource` cannot reach: portal dialogs, the clipboard, drag
//! and drop, cursor shapes and the input-method caret. It draws nothing but
//! the shell's backdrop, so what it prints is the shell's behaviour and
//! nothing of the application's.
//!
//! ```text
//! cargo run -p xarast-shell --example platform_probe
//! ```
//!
//! Every line on stdout is either `event <ShellEvent>` or `reply <...>`.
//! Commands, one per line on stdin:
//!
//! | Command | Effect |
//! |---|---|
//! | `open` / `open-many` / `save` | a portal file dialog |
//! | `copy-text TEXT` / `paste-text` | text through the system clipboard |
//! | `copy-image W H` / `paste-image` | a generated RGBA image, and back |
//! | `clipboard-info` | the clipboard backend and its persistence claim |
//! | `cursor SHAPE` | a [`CursorShape`] by its `Debug` name |
//! | `ime on` / `ime off` / `ime-area X Y W H` | the input method |
//! | `size` | the surface size and scale |
//! | `title TEXT` | the window title |
//! | `quit` | close |

use std::collections::VecDeque;
use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use xarast_shell::{
    ClipboardImage, CursorShape, FileFilter, FrameRequest, OpenFileRequest, SaveFileRequest,
    ShellApp, ShellConfig, ShellCtx, ShellEvent, init_tracing,
};

type Inbox = Arc<Mutex<VecDeque<String>>>;

struct Probe {
    start: Instant,
    inbox: Inbox,
    reader_started: bool,
    quit: bool,
}

impl Probe {
    fn say(&self, kind: &str, text: &str) {
        let ms = self.start.elapsed().as_millis();
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{ms:>7} {kind} {text}");
        let _ = out.flush();
    }

    fn start_reader(&mut self, ctx: &ShellCtx<'_>) {
        if self.reader_started {
            return;
        }
        self.reader_started = true;
        let inbox = self.inbox.clone();
        let waker = ctx.waker();
        std::thread::spawn(move || {
            for line in std::io::stdin().lock().lines() {
                let Ok(line) = line else { break };
                if let Ok(mut q) = inbox.lock() {
                    q.push_back(line);
                }
                waker.wake();
            }
        });
    }

    fn run(&mut self, line: &str, ctx: &mut ShellCtx<'_>) {
        let (cmd, rest) = line.trim().split_once(' ').unwrap_or((line.trim(), ""));
        let filters = vec![FileFilter::new("Xara drawings", &["xar", "web"])];
        match cmd {
            "" => {}
            "open" | "open-many" => {
                let id = ctx.portal().open_files(OpenFileRequest {
                    title: "Open a drawing".to_owned(),
                    filters,
                    multiple: cmd == "open-many",
                    directory: None,
                });
                self.say("reply", &format!("requested {id:?}"));
            }
            "save" => {
                let id = ctx.portal().save_file(SaveFileRequest {
                    title: "Save the drawing".to_owned(),
                    filters,
                    file_name: Some("untitled.xar".to_owned()),
                    directory: None,
                });
                self.say("reply", &format!("requested {id:?}"));
            }
            "copy-text" => {
                let r = ctx.clipboard().set_text(rest);
                self.say("reply", &format!("copy-text {r:?}"));
            }
            "paste-text" => {
                let r = ctx.clipboard().text();
                self.say("reply", &format!("paste-text {r:?}"));
            }
            "copy-image" => {
                let mut dims = rest.split_whitespace().filter_map(|v| v.parse().ok());
                let w: usize = dims.next().unwrap_or(64);
                let h: usize = dims.next().unwrap_or(32);
                let rgba = test_image(w, h);
                let r = ClipboardImage::new(w, h, rgba).map_or_else(
                    || Err("bad size".to_owned()),
                    |img| ctx.clipboard().set_image(&img).map_err(|e| e.to_string()),
                );
                self.say("reply", &format!("copy-image {w}x{h} {r:?}"));
            }
            "paste-image" => match ctx.clipboard().image() {
                Ok(img) => {
                    let intact = img.rgba == test_image(img.width, img.height);
                    self.say(
                        "reply",
                        &format!(
                            "paste-image {}x{} bytes={} matches_generated={intact}",
                            img.width,
                            img.height,
                            img.rgba.len()
                        ),
                    );
                }
                Err(e) => self.say("reply", &format!("paste-image Err({e})")),
            },
            "clipboard-info" => {
                let persists = ctx.clipboard().persists_after_focus_loss();
                self.say(
                    "reply",
                    &format!(
                        "clipboard {:?} persists_after_focus_loss={persists}",
                        ctx.clipboard()
                    ),
                );
            }
            "cursor" => match cursor(rest) {
                Some(shape) => {
                    ctx.set_cursor(shape);
                    self.say("reply", &format!("cursor {shape:?}"));
                }
                None => self.say("reply", &format!("cursor: unknown shape {rest:?}")),
            },
            "ime" => {
                let on = rest == "on";
                ctx.set_ime_allowed(on);
                self.say("reply", &format!("ime allowed={on}"));
            }
            "ime-area" => {
                let v: Vec<f64> = rest
                    .split_whitespace()
                    .filter_map(|v| v.parse().ok())
                    .collect();
                if let [x, y, w, h] = v[..] {
                    ctx.set_ime_cursor_area(x, y, w, h);
                    self.say("reply", &format!("ime-area {x} {y} {w} {h}"));
                } else {
                    self.say("reply", "ime-area needs X Y W H");
                }
            }
            "size" => {
                self.say(
                    "reply",
                    &format!("size {:?} scale {:?}", ctx.surface_size(), ctx.scale()),
                );
            }
            "title" => {
                ctx.set_title(rest);
                self.say("reply", "title set");
            }
            "quit" => {
                self.quit = true;
                ctx.exit();
            }
            other => self.say("reply", &format!("unknown command {other:?}")),
        }
    }
}

/// A deterministic RGBA pattern with every alpha value in use, so a
/// premultiplication or channel swap anywhere on the way shows up.
fn test_image(w: usize, h: usize) -> Vec<u8> {
    let mut v = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            // Truncation to a byte is the point: the pattern wraps.
            #[allow(clippy::cast_possible_truncation)]
            v.extend_from_slice(&[(x * 4) as u8, (y * 8) as u8, (x ^ y) as u8, (x + y) as u8]);
        }
    }
    v
}

fn cursor(name: &str) -> Option<CursorShape> {
    use CursorShape as C;
    Some(match name {
        "Default" => C::Default,
        "Hidden" => C::Hidden,
        "Text" => C::Text,
        "Hand" => C::Hand,
        "Grab" => C::Grab,
        "Grabbing" => C::Grabbing,
        "Move" => C::Move,
        "Crosshair" => C::Crosshair,
        "NotAllowed" => C::NotAllowed,
        "ResizeHorizontal" => C::ResizeHorizontal,
        "ResizeVertical" => C::ResizeVertical,
        "ResizeNwSe" => C::ResizeNwSe,
        "ResizeNeSw" => C::ResizeNeSw,
        "Wait" => C::Wait,
        "Help" => C::Help,
        "ZoomIn" => C::ZoomIn,
        "ZoomOut" => C::ZoomOut,
        _ => return None,
    })
}

impl ShellApp for Probe {
    fn on_event(&mut self, event: ShellEvent, ctx: &mut ShellCtx<'_>) {
        self.say("event", &format!("{event:?}"));
        if matches!(event, ShellEvent::CloseRequested) {
            ctx.exit();
        }
    }

    fn on_frame(&mut self, ctx: &mut ShellCtx<'_>) -> FrameRequest {
        self.start_reader(ctx);
        let lines: Vec<String> = self
            .inbox
            .lock()
            .map(|mut q| q.drain(..).collect())
            .unwrap_or_default();
        for line in lines {
            self.run(&line, ctx);
            if self.quit {
                break;
            }
        }
        FrameRequest::Idle
    }
}

fn main() {
    init_tracing();
    let probe = Probe {
        start: Instant::now(),
        inbox: Arc::default(),
        reader_started: false,
        quit: false,
    };
    probe.say(
        "reply",
        &format!("platform {}", xarast_shell::platform_summary()),
    );
    let config = ShellConfig {
        title: "Xarast platform probe".to_owned(),
        size: (800, 500),
        ..ShellConfig::default()
    };
    if let Err(e) = xarast_shell::run_app(config, probe) {
        eprintln!("platform_probe: {e}");
        std::process::exit(1);
    }
}
