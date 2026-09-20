//! Preferences: the settings that outlive a session.
//!
//! They live here rather than in the UI because the headless tools need
//! the same units and the same cache budget as the window does, and
//! because the shell persists them without knowing what they mean.
//! Serialisation is deliberately absent: Phase 6 owns the on-disk form,
//! and inventing one here would be a format to migrate later.

use std::time::Duration;

/// The unit the rulers, the status bar and every numeric field use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum Unit {
    /// Millimetres.
    #[default]
    Millimetre,
    /// Centimetres.
    Centimetre,
    /// Inches.
    Inch,
    /// Points.
    Point,
    /// Pixels at 96 dpi.
    Pixel,
}

impl Unit {
    /// How many millipoints one of these is.
    #[must_use]
    pub fn millipoints(self) -> f64 {
        match self {
            Unit::Millimetre => xarast_geom::Mp::PER_MM,
            Unit::Centimetre => xarast_geom::Mp::PER_CM,
            Unit::Inch => f64::from(xarast_geom::Mp::PER_INCH),
            Unit::Point => f64::from(xarast_geom::Mp::PER_PT),
            Unit::Pixel => f64::from(xarast_geom::Mp::PER_PX96),
        }
    }

    /// The suffix shown after a value.
    #[must_use]
    pub const fn suffix(self) -> &'static str {
        match self {
            Unit::Millimetre => "mm",
            Unit::Centimetre => "cm",
            Unit::Inch => "in",
            Unit::Point => "pt",
            Unit::Pixel => "px",
        }
    }

    /// Converts millipoints into this unit.
    #[must_use]
    pub fn from_mp(self, v: xarast_geom::Mp) -> f64 {
        v.to_f64() / self.millipoints()
    }
}

/// Which colour scheme the interface should use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum ThemePref {
    /// Follow `org.freedesktop.appearance color-scheme`.
    #[default]
    FollowSystem,
    /// Always dark.
    Dark,
    /// Always light.
    Light,
}

/// Which renderer the shell should try first.
///
/// `xarast-shell` owns the capability ladder; this is only the user's
/// stated preference, persisted here so the headless tools and the
/// window agree on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum RendererPref {
    /// Pick the best tier that initialises.
    #[default]
    Auto,
    /// Insist on the GPU backend.
    ForceGpu,
    /// GPU compositing over CPU rasterisation.
    ForceHybrid,
    /// The deterministic CPU backend.
    ForceCpu,
}

/// Everything the application remembers between runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Preferences {
    /// The unit rulers and fields use.
    pub units: Unit,
    /// The colour scheme.
    pub theme: ThemePref,
    /// The renderer the user asked for.
    pub renderer: RendererPref,
    /// How often to autosave. `None` disables it.
    pub autosave: Option<Duration>,
    /// How many bytes the render cache may hold.
    pub cache_budget_bytes: usize,
    /// How long the view must be still before the Draft render is
    /// replaced by a Final one.
    pub draft_settle: Duration,
    /// How many bytes of undo history to keep.
    pub history_budget_bytes: usize,
}

impl Default for Preferences {
    fn default() -> Preferences {
        Preferences {
            units: Unit::default(),
            theme: ThemePref::default(),
            renderer: RendererPref::default(),
            autosave: Some(Duration::from_secs(300)),
            cache_budget_bytes: 256 << 20,
            // The phase's Draft→Final timer.
            draft_settle: Duration::from_millis(120),
            history_budget_bytes: 128 << 20,
        }
    }
}
