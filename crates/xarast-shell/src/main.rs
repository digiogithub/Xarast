//! Entry point for the Xarast desktop application.

use std::path::PathBuf;
use std::process::ExitCode;

use xarast_shell::{
    APP_ID, ShellConfig, cold_start_ms, init_tracing, install_panic_hook, mark_process_start,
    platform_summary,
};

const USAGE: &str = "\
Xarast — a vector illustration and photo editor

USAGE:
    xarast [OPTIONS] [FILE.xar ...]

Opens each FILE; the last one is shown. Drop a .xar on the window to open
it too. On the canvas: wheel scrolls, Shift+wheel scrolls sideways,
Ctrl+wheel zooms about the pointer, middle-drag pans, a trackpad pinch
zooms; +/- zoom, 1 is 100 %, 0 or Home fits the page, d fits the drawing.

OPTIONS:
    -h, --help              Print this help and exit
    -V, --version           Print the version and exit
        --verbose           With --version, also print the platform summary
        --selftest          Check that the build is internally consistent, without
                            opening a window, and exit
        --selftest-window   Open a window, present one frame, report the graphics
                            adapter and exit. This is the liveness check; run it
                            first when reporting a rendering problem. With no
                            display server it reports that and exits 0, because
                            the absence of a compositor is a fact, not a fault.
        --frames <N>        Exit after presenting N frames
        --screenshot <PNG>  Once the document has rendered, write the window's
                            content to PNG and exit

ENVIRONMENT:
    XARAST_LOG              Log filter, e.g. `info`, `xarast_shell=debug`
    WGPU_BACKEND            Force a wgpu backend, e.g. `vulkan`, `gl`
    WINIT_UNIX_BACKEND      Force `wayland` or `x11`; X11 loses fractional
                            scaling, trackpad gestures and tablet axes
";

fn main() -> ExitCode {
    mark_process_start();
    init_tracing();
    install_panic_hook();

    let mut config = ShellConfig::default();
    let mut selftest = false;
    let mut selftest_window = false;
    let mut version = false;
    let mut verbose = false;
    let mut files: Vec<PathBuf> = Vec::new();
    let mut screenshot: Option<PathBuf> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "-V" | "--version" => version = true,
            "--verbose" => verbose = true,
            "--selftest" => selftest = true,
            "--selftest-window" => selftest_window = true,
            "--frames" => match args.next().and_then(|v| v.parse().ok()) {
                Some(n) => config.exit_after_frames = Some(n),
                None => {
                    eprintln!("--frames needs a number");
                    return ExitCode::FAILURE;
                }
            },
            "--screenshot" => match args.next() {
                Some(p) => screenshot = Some(PathBuf::from(p)),
                None => {
                    eprintln!("--screenshot needs a file name");
                    return ExitCode::FAILURE;
                }
            },
            other if other.starts_with('-') => {
                eprintln!("unknown argument: {other}\n\n{USAGE}");
                return ExitCode::FAILURE;
            }
            file => files.push(PathBuf::from(file)),
        }
    }

    if version {
        println!("xarast {}", env!("CARGO_PKG_VERSION"));
        if verbose {
            println!("platform: {}", platform_summary());
        }
        return ExitCode::SUCCESS;
    }

    if selftest {
        println!("xarast {}", env!("CARGO_PKG_VERSION"));
        println!("app id: {APP_ID}");
        println!("platform: {}", platform_summary());
        println!("selftest: ok");
        return ExitCode::SUCCESS;
    }

    if selftest_window {
        return match xarast_shell::selftest_window(&config) {
            Ok(report) => {
                println!("adapter: {report}");
                if let Some(ms) = cold_start_ms() {
                    println!("cold start: {ms:.1} ms");
                }
                println!("selftest-window: ok");
                ExitCode::SUCCESS
            }
            // No compositor is not a failing build. CI runs this check on
            // machines that have no display at all, and a red result there
            // would teach everyone to ignore it.
            Err(e) if e.is_missing_display() => {
                println!("selftest-window: skipped ({e})");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("selftest-window: {e}");
                ExitCode::FAILURE
            }
        };
    }

    let mut viewer = xarast_shell::viewer::Viewer::new(files);
    if let Some(path) = screenshot {
        viewer = viewer.with_screenshot(path);
    }
    match xarast_shell::run_app(config, viewer) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xarast: {e}");
            ExitCode::FAILURE
        }
    }
}
