//! The clipboard.
//!
//! One behaviour is worth stating rather than discovering: **on Wayland the
//! clipboard belongs to the focused client.** Copy something, close the
//! window, and the data is gone unless the compositor implements a
//! data-control manager or a clipboard manager is running. That is not a bug
//! we can fix; it is a property of the protocol, and the honest response is
//! to say so in the status bar instead of pretending the copy persisted. The
//! `wayland-data-control` backend is enabled precisely so that the cases
//! where it *can* persist actually do.
//!
//! Every operation returns a [`ClipboardError`] rather than panicking. A
//! headless build gets [`NullClipboard`], whose errors name the reason.

use std::fmt;

/// Why a clipboard operation did not work.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ClipboardError {
    /// There is no clipboard to talk to.
    #[error("no clipboard: {0}")]
    Unavailable(String),
    /// The clipboard holds nothing of the requested kind.
    #[error("the clipboard holds no {0}")]
    Empty(&'static str),
    /// The platform refused.
    #[error("clipboard error: {0}")]
    Platform(String),
}

/// An image on the clipboard, as straight 8-bit RGBA.
///
/// Not premultiplied: every clipboard protocol in use hands over
/// straight alpha, and converting here would mean converting back for the
/// renderer.
#[derive(Clone, PartialEq, Eq)]
pub struct ClipboardImage {
    /// Width in pixels.
    pub width: usize,
    /// Height in pixels.
    pub height: usize,
    /// `width * height * 4` bytes of straight RGBA.
    pub rgba: Vec<u8>,
}

impl ClipboardImage {
    /// An image, if the buffer length matches the dimensions.
    #[must_use]
    pub fn new(width: usize, height: usize, rgba: Vec<u8>) -> Option<Self> {
        if width.checked_mul(height)?.checked_mul(4)? != rgba.len() {
            return None;
        }
        Some(Self {
            width,
            height,
            rgba,
        })
    }
}

impl fmt::Debug for ClipboardImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClipboardImage")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("bytes", &self.rgba.len())
            .finish()
    }
}

/// Read and write access to the system clipboard.
pub trait Clipboard: fmt::Debug + Send {
    /// The clipboard's text.
    ///
    /// # Errors
    /// When there is no clipboard, it holds no text, or the platform refused.
    fn text(&mut self) -> Result<String, ClipboardError>;

    /// Puts text on the clipboard.
    ///
    /// # Errors
    /// When there is no clipboard or the platform refused.
    fn set_text(&mut self, text: &str) -> Result<(), ClipboardError>;

    /// The clipboard's image.
    ///
    /// # Errors
    /// When there is no clipboard, it holds no image, or the platform refused.
    fn image(&mut self) -> Result<ClipboardImage, ClipboardError>;

    /// Puts an image on the clipboard.
    ///
    /// # Errors
    /// When there is no clipboard or the platform refused.
    fn set_image(&mut self, image: &ClipboardImage) -> Result<(), ClipboardError>;

    /// Whether data put here survives this window losing focus.
    ///
    /// The status bar uses it to warn before the user finds out the hard way.
    fn persists_after_focus_loss(&self) -> bool;
}

/// The clipboard on a machine that has none.
#[derive(Debug, Clone)]
pub struct NullClipboard {
    reason: String,
}

impl NullClipboard {
    /// A null clipboard that explains itself.
    #[must_use]
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }

    fn err<T>(&self) -> Result<T, ClipboardError> {
        Err(ClipboardError::Unavailable(self.reason.clone()))
    }
}

impl Clipboard for NullClipboard {
    fn text(&mut self) -> Result<String, ClipboardError> {
        self.err()
    }
    fn set_text(&mut self, _text: &str) -> Result<(), ClipboardError> {
        self.err()
    }
    fn image(&mut self) -> Result<ClipboardImage, ClipboardError> {
        self.err()
    }
    fn set_image(&mut self, _image: &ClipboardImage) -> Result<(), ClipboardError> {
        self.err()
    }
    fn persists_after_focus_loss(&self) -> bool {
        false
    }
}

#[cfg(feature = "clipboard")]
mod system {
    use super::{Clipboard, ClipboardError, ClipboardImage};
    use crate::display::{DisplayEnvironment, DisplayServer};

    /// The system clipboard, through `arboard`.
    pub struct SystemClipboard {
        inner: arboard::Clipboard,
        persists: bool,
    }

    // `arboard::Clipboard` holds a platform connection and is not `Debug`.
    // The trait requires one, so the handle is described rather than dumped.
    impl std::fmt::Debug for SystemClipboard {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("SystemClipboard")
                .field("persists_after_focus_loss", &self.persists)
                .finish_non_exhaustive()
        }
    }

    impl SystemClipboard {
        /// Connects to the system clipboard.
        ///
        /// # Errors
        /// When no clipboard is reachable — no display server, or the
        /// platform refused the connection.
        pub fn new() -> Result<Self, ClipboardError> {
            let env = DisplayEnvironment::from_env();
            let inner = arboard::Clipboard::new()
                .map_err(|e| ClipboardError::Unavailable(e.to_string()))?;
            Ok(Self {
                inner,
                // X11 sessions normally run a clipboard manager that takes
                // ownership when a client exits; Wayland has no such
                // guarantee without a data-control manager.
                persists: env.capabilities().clipboard_survives_focus_loss,
            })
        }
    }

    impl Clipboard for SystemClipboard {
        fn text(&mut self) -> Result<String, ClipboardError> {
            self.inner.get_text().map_err(map_err)
        }

        fn set_text(&mut self, text: &str) -> Result<(), ClipboardError> {
            self.inner.set_text(text).map_err(map_err)
        }

        fn image(&mut self) -> Result<ClipboardImage, ClipboardError> {
            let img = self.inner.get_image().map_err(map_err)?;
            ClipboardImage::new(img.width, img.height, img.bytes.into_owned()).ok_or(
                ClipboardError::Platform(
                    "the clipboard image dimensions do not match its buffer".to_owned(),
                ),
            )
        }

        fn set_image(&mut self, image: &ClipboardImage) -> Result<(), ClipboardError> {
            self.inner
                .set_image(arboard::ImageData {
                    width: image.width,
                    height: image.height,
                    bytes: std::borrow::Cow::Borrowed(&image.rgba),
                })
                .map_err(map_err)
        }

        fn persists_after_focus_loss(&self) -> bool {
            self.persists
        }
    }

    fn map_err(e: arboard::Error) -> ClipboardError {
        match e {
            arboard::Error::ContentNotAvailable => ClipboardError::Empty("data of that kind"),
            arboard::Error::ClipboardNotSupported => {
                ClipboardError::Unavailable("the platform has no clipboard".to_owned())
            }
            other => ClipboardError::Platform(other.to_string()),
        }
    }

    /// True when a clipboard could plausibly be reached.
    pub fn plausible() -> bool {
        DisplayEnvironment::from_env().server() != DisplayServer::None
    }
}

#[cfg(feature = "clipboard")]
pub use system::SystemClipboard;

/// The best clipboard this machine can offer.
///
/// Never fails and never blocks start-up: with no display server, or with the
/// `clipboard` feature off, it returns a [`NullClipboard`] that explains
/// itself on every call.
#[must_use]
pub fn system_clipboard() -> Box<dyn Clipboard> {
    #[cfg(feature = "clipboard")]
    {
        if system::plausible() {
            match SystemClipboard::new() {
                Ok(c) => Box::new(c),
                Err(e) => {
                    tracing::warn!(error = %e, "falling back to a clipboard that does nothing");
                    Box::new(NullClipboard::new(e.to_string()))
                }
            }
        } else {
            Box::new(NullClipboard::new(
                "no display server, so there is no clipboard to connect to",
            ))
        }
    }
    #[cfg(not(feature = "clipboard"))]
    {
        Box::new(NullClipboard::new(
            "this build was compiled without the `clipboard` feature",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_image_must_match_its_dimensions() {
        assert!(ClipboardImage::new(2, 2, vec![0; 16]).is_some());
        assert!(ClipboardImage::new(2, 2, vec![0; 15]).is_none());
        assert!(ClipboardImage::new(0, 0, Vec::new()).is_some());
    }

    #[test]
    fn absurd_dimensions_do_not_overflow() {
        assert!(ClipboardImage::new(usize::MAX, usize::MAX, vec![0; 4]).is_none());
    }

    #[test]
    fn the_image_debug_output_does_not_dump_the_pixels() {
        let img = ClipboardImage::new(2, 2, vec![7; 16]).unwrap();
        let s = format!("{img:?}");
        assert!(s.contains("bytes: 16"), "{s}");
        assert!(!s.contains("7, 7, 7"), "{s}");
    }

    #[test]
    fn the_null_clipboard_explains_itself_on_every_operation() {
        let mut c = NullClipboard::new("no display server");
        for e in [
            c.text().err(),
            c.set_text("x").err(),
            c.image().err(),
            c.set_image(&ClipboardImage::new(1, 1, vec![0; 4]).unwrap())
                .err(),
        ] {
            let e = e.expect("a null clipboard must refuse, not succeed");
            assert!(e.to_string().contains("no display server"), "{e}");
        }
        assert!(!c.persists_after_focus_loss());
    }

    #[test]
    fn a_headless_machine_gets_a_working_object_rather_than_a_panic() {
        // The point of this test is that it runs at all: constructing a
        // clipboard must never be a start-up failure.
        let mut c = system_clipboard();
        if crate::display::headless_skip_reason().is_some() {
            assert!(
                c.text().is_err(),
                "a headless build must not claim to have clipboard text"
            );
            assert!(!c.persists_after_focus_loss());
        }
    }

    #[test]
    fn wayland_is_not_claimed_to_persist_after_focus_loss() {
        // The capability table is what the status bar reads; if this ever
        // flips to true, users will be told their copy survived when it did
        // not.
        const { assert!(!crate::display::PlatformCapabilities::WAYLAND.clipboard_survives_focus_loss) }
    }
}
