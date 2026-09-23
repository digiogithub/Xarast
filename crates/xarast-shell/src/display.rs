//! Which display server we are on, and what it can actually do.
//!
//! Xarast is Wayland-first, but it has to run under X11 and under XWayland
//! too, and the three differ in ways the application can see. Rather than
//! scatter `cfg!` and environment lookups through the shell, the differences
//! are collected here as data, so they can be asserted in a test on a machine
//! with no compositor at all.

use std::fmt;

/// The display server this process is talking to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DisplayServer {
    /// A native Wayland compositor.
    Wayland,
    /// A real X11 server, or XWayland with the Wayland socket hidden.
    X11,
    /// X11 spoken to XWayland while a Wayland compositor is also reachable.
    ///
    /// Worth distinguishing: the window manager is a Wayland compositor, so
    /// fractional scaling and decorations behave as on Wayland, while input
    /// arrives through the X11 protocol and loses the tablet axes.
    XWayland,
    /// No display server. Headless CI, a container, an SSH session.
    None,
}

impl fmt::Display for DisplayServer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Wayland => "Wayland",
            Self::X11 => "X11",
            Self::XWayland => "XWayland",
            Self::None => "none",
        };
        f.write_str(s)
    }
}

/// What the current display server offers the shell.
///
/// This is the X11-versus-Wayland difference table, in executable form. It
/// drives the diagnostics the status bar and `--selftest-window` print, and it
/// is what stops the application from silently pretending it has pressure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlatformCapabilities {
    /// `wp_fractional_scale_v1`: a non-integer scale is reported and honoured.
    ///
    /// On X11 the scale is a single integer derived from Xft.dpi for the whole
    /// screen, so a 1.25 display is rendered at 1× and scaled by the server,
    /// or at 2× and downsampled. Neither is crisp.
    pub fractional_scale: bool,
    /// The application is expected to draw its own title bar and borders.
    pub client_side_decorations: bool,
    /// Stylus pressure, tilt and twist can reach the application.
    pub tablet_axes: bool,
    /// Trackpad pinch, pan and rotation gestures are delivered.
    pub gestures: bool,
    /// Clipboard contents survive the application exiting.
    ///
    /// A copy always survives the window losing focus — the selection
    /// belongs to its source until something replaces it (measured on
    /// GNOME 46, `docs/memory/ui.md`). What does not survive, unless a
    /// clipboard manager takes the data over, is the source process
    /// exiting. X11 desktops normally run one. On Wayland it depends on
    /// the compositor, so the generic answer is the cautious one; GNOME is
    /// refined in `clipboard::SystemClipboard`.
    pub clipboard_survives_exit: bool,
    /// The window can set its own position.
    pub can_position_window: bool,
}

impl PlatformCapabilities {
    /// Capabilities of a native Wayland session.
    ///
    /// `tablet_axes` is `false` and not an oversight: it reflects the pinned
    /// `winit` 0.30, which has no tablet API on any platform. See
    /// `docs/memory/ui.md` for the evaluation that pinned it.
    pub const WAYLAND: Self = Self {
        fractional_scale: true,
        client_side_decorations: true,
        tablet_axes: false,
        gestures: true,
        clipboard_survives_exit: false,
        can_position_window: false,
    };

    /// Capabilities of an X11 session, XWayland included.
    pub const X11: Self = Self {
        fractional_scale: false,
        client_side_decorations: false,
        tablet_axes: false,
        gestures: false,
        clipboard_survives_exit: true,
        can_position_window: true,
    };

    /// Capabilities with no display server: everything is off.
    pub const HEADLESS: Self = Self {
        fractional_scale: false,
        client_side_decorations: false,
        tablet_axes: false,
        gestures: false,
        clipboard_survives_exit: false,
        can_position_window: false,
    };

    /// The capabilities of a given server.
    #[must_use]
    pub const fn of(server: DisplayServer) -> Self {
        match server {
            DisplayServer::Wayland => Self::WAYLAND,
            DisplayServer::X11 | DisplayServer::XWayland => Self::X11,
            DisplayServer::None => Self::HEADLESS,
        }
    }
}

/// The desktop environment, where it changes our behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Desktop {
    /// GNOME and its derivatives. Never offers server-side decorations.
    Gnome,
    /// KDE Plasma. Offers server-side decorations.
    Kde,
    /// A wlroots compositor: sway, Hyprland, river, labwc.
    Wlroots,
    /// Something else, or nothing.
    Other,
}

/// Everything the shell learned about the session from the environment.
///
/// Captured once at start-up. Environment variables are the only portable way
/// to know this before a window exists, and the window is exactly what we need
/// the answer for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DisplayEnvironment {
    /// `WAYLAND_DISPLAY`.
    pub wayland_display: Option<String>,
    /// `DISPLAY`.
    pub x11_display: Option<String>,
    /// `XDG_SESSION_TYPE`.
    pub session_type: Option<String>,
    /// `XDG_CURRENT_DESKTOP`.
    pub current_desktop: Option<String>,
    /// `WINIT_UNIX_BACKEND`, which forces one backend over the other.
    pub forced_backend: Option<String>,
}

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

impl DisplayEnvironment {
    /// Reads the environment.
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            wayland_display: var("WAYLAND_DISPLAY"),
            x11_display: var("DISPLAY"),
            session_type: var("XDG_SESSION_TYPE"),
            current_desktop: var("XDG_CURRENT_DESKTOP"),
            forced_backend: var("WINIT_UNIX_BACKEND"),
        }
    }

    /// Which server `winit` will end up talking to.
    ///
    /// The precedence matches `winit`'s own: an explicit `WINIT_UNIX_BACKEND`
    /// wins, then Wayland, then X11. Getting this wrong means the shell
    /// reports capabilities for a backend it is not using.
    #[must_use]
    pub fn server(&self) -> DisplayServer {
        let forced = self.forced_backend.as_deref();
        match forced {
            Some("wayland") => {
                if self.wayland_display.is_some() {
                    DisplayServer::Wayland
                } else {
                    DisplayServer::None
                }
            }
            Some("x11") => match (&self.x11_display, &self.wayland_display) {
                (Some(_), Some(_)) => DisplayServer::XWayland,
                (Some(_), None) => DisplayServer::X11,
                (None, _) => DisplayServer::None,
            },
            _ => match (&self.wayland_display, &self.x11_display) {
                (Some(_), _) => DisplayServer::Wayland,
                (None, Some(_)) => {
                    // A Wayland session whose compositor socket is not visible
                    // to us but whose X11 socket is: that is XWayland.
                    if self.session_type.as_deref() == Some("wayland") {
                        DisplayServer::XWayland
                    } else {
                        DisplayServer::X11
                    }
                }
                (None, None) => DisplayServer::None,
            },
        }
    }

    /// The desktop environment, as far as it matters to us.
    #[must_use]
    pub fn desktop(&self) -> Desktop {
        let Some(list) = self.current_desktop.as_deref() else {
            return Desktop::Other;
        };
        for entry in list.split(':') {
            match entry.to_ascii_uppercase().as_str() {
                "GNOME" | "UBUNTU" | "POP" => return Desktop::Gnome,
                "KDE" => return Desktop::Kde,
                "SWAY" | "HYPRLAND" | "RIVER" | "LABWC" | "WLROOTS" | "COSMIC" => {
                    return Desktop::Wlroots;
                }
                _ => {}
            }
        }
        Desktop::Other
    }

    /// Capabilities implied by this environment.
    #[must_use]
    pub fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities::of(self.server())
    }

    /// True when there is no display server to talk to.
    #[must_use]
    pub fn is_headless(&self) -> bool {
        self.server() == DisplayServer::None
    }

    /// One line describing the session, for logs and bug reports.
    #[must_use]
    pub fn summary(&self) -> String {
        let desktop = match self.desktop() {
            Desktop::Gnome => "GNOME",
            Desktop::Kde => "KDE",
            Desktop::Wlroots => "wlroots",
            Desktop::Other => "unknown desktop",
        };
        format!("{} / {desktop}", self.server())
    }
}

/// Why a test or a start-up step that needs a compositor cannot run.
///
/// Returns `None` when a display server is present. Tests use it to **skip**:
/// a machine without a compositor must never turn into a red build, or CI
/// stops telling us anything.
#[must_use]
pub fn headless_skip_reason() -> Option<String> {
    let env = DisplayEnvironment::from_env();
    if env.is_headless() {
        Some(
            "no display server: WAYLAND_DISPLAY and DISPLAY are both unset, \
             so there is no compositor to open a window on"
                .to_owned(),
        )
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(wayland: Option<&str>, x11: Option<&str>, session: Option<&str>) -> DisplayEnvironment {
        DisplayEnvironment {
            wayland_display: wayland.map(str::to_owned),
            x11_display: x11.map(str::to_owned),
            session_type: session.map(str::to_owned),
            ..DisplayEnvironment::default()
        }
    }

    #[test]
    fn wayland_wins_when_both_sockets_exist() {
        assert_eq!(
            env(Some("wayland-0"), Some(":0"), Some("wayland")).server(),
            DisplayServer::Wayland
        );
    }

    #[test]
    fn forcing_x11_inside_a_wayland_session_is_xwayland() {
        let e = DisplayEnvironment {
            forced_backend: Some("x11".to_owned()),
            ..env(Some("wayland-0"), Some(":0"), Some("wayland"))
        };
        assert_eq!(e.server(), DisplayServer::XWayland);
        // XWayland loses the tablet axes and fractional scaling, which is
        // exactly the degradation the user needs to be told about.
        assert!(!e.capabilities().fractional_scale);
    }

    #[test]
    fn a_bare_x11_session_is_x11() {
        assert_eq!(
            env(None, Some(":0"), Some("x11")).server(),
            DisplayServer::X11
        );
    }

    #[test]
    fn an_x11_socket_in_a_wayland_session_is_xwayland() {
        assert_eq!(
            env(None, Some(":0"), Some("wayland")).server(),
            DisplayServer::XWayland
        );
    }

    #[test]
    fn nothing_at_all_is_headless() {
        let e = env(None, None, None);
        assert_eq!(e.server(), DisplayServer::None);
        assert!(e.is_headless());
        assert_eq!(e.capabilities(), PlatformCapabilities::HEADLESS);
    }

    #[test]
    fn forcing_wayland_without_a_socket_is_headless_not_x11() {
        let e = DisplayEnvironment {
            forced_backend: Some("wayland".to_owned()),
            ..env(None, Some(":0"), None)
        };
        assert_eq!(e.server(), DisplayServer::None);
    }

    #[test]
    fn the_desktop_is_read_out_of_a_colon_separated_list() {
        let mut e = env(Some("wayland-0"), None, Some("wayland"));
        e.current_desktop = Some("ubuntu:GNOME".to_owned());
        assert_eq!(e.desktop(), Desktop::Gnome);
        e.current_desktop = Some("sway".to_owned());
        assert_eq!(e.desktop(), Desktop::Wlroots);
        e.current_desktop = None;
        assert_eq!(e.desktop(), Desktop::Other);
    }

    #[test]
    fn x11_and_wayland_differ_where_we_say_they_do() {
        let w = PlatformCapabilities::WAYLAND;
        let x = PlatformCapabilities::X11;
        assert!(w.fractional_scale && !x.fractional_scale);
        assert!(w.client_side_decorations && !x.client_side_decorations);
        assert!(w.gestures && !x.gestures);
        assert!(!w.clipboard_survives_exit && x.clipboard_survives_exit);
        assert!(!w.can_position_window && x.can_position_window);
    }

    #[test]
    fn the_summary_names_the_server() {
        let e = env(Some("wayland-0"), None, Some("wayland"));
        assert!(e.summary().starts_with("Wayland"));
    }
}
