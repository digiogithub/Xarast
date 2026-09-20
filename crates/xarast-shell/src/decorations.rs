//! Window decorations, which on Wayland are the application's problem.
//!
//! There is no Wayland protocol that guarantees a title bar. `xdg-decoration`
//! lets a client *ask* for server-side decorations, and the compositor may
//! refuse: GNOME always refuses, KDE and most wlroots compositors agree.
//! When it refuses, the client must draw its own frame or the window has no
//! title bar, no close button and no resize border at all.
//!
//! `winit` handles the drawing, through SCTK and `sctk-adwaita`, which is why
//! the `wayland-csd-adwaita` feature is not optional. What is left to us is
//! knowing which mode we are in — the canvas geometry and the fullscreen
//! behaviour differ — and being able to say so in a bug report.

use crate::display::{Desktop, DisplayEnvironment, DisplayServer};

/// Who draws the window frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecorationMode {
    /// The compositor or window manager draws it.
    ServerSide,
    /// We draw it, through `sctk-adwaita`.
    ClientSide,
    /// There is no frame: no display server, or decorations were switched off.
    None,
}

/// What the shell expects to happen to this window's frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecorationPlan {
    /// Whether to ask for decorations at all.
    pub request_decorations: bool,
    /// What we expect to get. An expectation, not a promise: only the
    /// compositor decides, and it decides after the window exists.
    pub expected: DecorationMode,
    /// True when client-side decorations are the only possible outcome, so
    /// a missing `wayland-csd-adwaita` feature would leave a bare surface.
    pub client_side_is_mandatory: bool,
}

impl DecorationPlan {
    /// Works out the plan from the session.
    #[must_use]
    pub fn for_environment(env: &DisplayEnvironment) -> Self {
        match env.server() {
            DisplayServer::None => Self {
                request_decorations: false,
                expected: DecorationMode::None,
                client_side_is_mandatory: false,
            },
            // XWayland windows are decorated by the X11 side of the
            // compositor, so they behave like X11 here.
            DisplayServer::X11 | DisplayServer::XWayland => Self {
                request_decorations: true,
                expected: DecorationMode::ServerSide,
                client_side_is_mandatory: false,
            },
            DisplayServer::Wayland => {
                let gnome = env.desktop() == Desktop::Gnome;
                Self {
                    request_decorations: true,
                    expected: if gnome {
                        DecorationMode::ClientSide
                    } else {
                        DecorationMode::ServerSide
                    },
                    client_side_is_mandatory: gnome,
                }
            }
        }
    }

    /// One line for the log and for `--selftest-window`.
    #[must_use]
    pub fn summary(&self) -> &'static str {
        match self.expected {
            DecorationMode::ServerSide => "server-side decorations expected",
            DecorationMode::ClientSide => "client-side decorations expected (sctk-adwaita)",
            DecorationMode::None => "no decorations (no display server)",
        }
    }
}

/// Whether this build can draw its own decorations.
///
/// Answered from the target rather than guessed at run time. `winit`'s
/// `wayland-csd-adwaita` feature is enabled unconditionally in
/// `Cargo.toml`, because a build without it produces a window with no title
/// bar on GNOME — the kind of packaging mistake that only shows up on one
/// desktop.
#[must_use]
pub const fn client_side_decorations_available() -> bool {
    cfg!(all(
        unix,
        not(target_os = "macos"),
        not(target_os = "android")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(wayland: bool, desktop: &str) -> DisplayEnvironment {
        DisplayEnvironment {
            wayland_display: wayland.then(|| "wayland-0".to_owned()),
            x11_display: (!wayland).then(|| ":0".to_owned()),
            session_type: Some(if wayland { "wayland" } else { "x11" }.to_owned()),
            current_desktop: Some(desktop.to_owned()),
            forced_backend: None,
        }
    }

    #[test]
    fn gnome_on_wayland_must_draw_its_own_frame() {
        let plan = DecorationPlan::for_environment(&env(true, "ubuntu:GNOME"));
        assert_eq!(plan.expected, DecorationMode::ClientSide);
        assert!(plan.client_side_is_mandatory);
        assert!(plan.request_decorations);
    }

    #[test]
    fn kde_on_wayland_can_expect_the_compositor_to_draw_it() {
        let plan = DecorationPlan::for_environment(&env(true, "KDE"));
        assert_eq!(plan.expected, DecorationMode::ServerSide);
        assert!(!plan.client_side_is_mandatory);
    }

    #[test]
    fn wlroots_expects_server_side_but_must_survive_a_refusal() {
        let plan = DecorationPlan::for_environment(&env(true, "sway"));
        assert_eq!(plan.expected, DecorationMode::ServerSide);
        // The expectation is not a promise, so the client-side path must
        // still be compiled in.
        assert!(client_side_decorations_available());
    }

    #[test]
    fn x11_always_gets_a_server_side_frame() {
        let plan = DecorationPlan::for_environment(&env(false, "XFCE"));
        assert_eq!(plan.expected, DecorationMode::ServerSide);
        assert!(!plan.client_side_is_mandatory);
    }

    #[test]
    fn xwayland_behaves_like_x11_for_decorations() {
        let mut e = env(true, "GNOME");
        e.forced_backend = Some("x11".to_owned());
        let plan = DecorationPlan::for_environment(&e);
        assert_eq!(plan.expected, DecorationMode::ServerSide);
    }

    #[test]
    fn a_headless_build_asks_for_nothing() {
        let e = DisplayEnvironment::default();
        let plan = DecorationPlan::for_environment(&e);
        assert!(!plan.request_decorations);
        assert_eq!(plan.expected, DecorationMode::None);
        assert!(plan.summary().contains("no display server"));
    }
}
