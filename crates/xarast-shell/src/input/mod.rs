//! Input: the platform-neutral event model and its translation.
//!
//! The layering is deliberate and is what phase 14 depends on:
//!
//! | Module | Platform-specific | Replaced in phase 14 |
//! |---|---|---|
//! | [`event`] | no | no |
//! | [`keyboard`] | no | no |
//! | [`ime`](crate::ime) | no | no |
//! | [`tablet`] | no | no |
//! | [`coalesce`] | no | no |
//! | [`translate`] | **yes** | **yes** |
//!
//! Only [`translate`] mentions the windowing backend. Everything a tool, a
//! panel or a command touches is on the neutral side of that line, so the
//! Windows and macOS shells are one module each rather than a fork of the
//! application.

pub mod coalesce;
pub mod event;
pub mod keyboard;
pub mod momentary;
pub mod tablet;
pub mod translate;
