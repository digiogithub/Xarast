//! Serialising millipoints.
//!
//! `xarast_geom::Mp` has no serde support of its own, and giving it one is
//! the geometry crate's decision, not the interface's. Until it does, the
//! few settings this crate persists — grid spacing, guide positions — go
//! through these adapters, which write the raw millipoint count and nothing
//! else, so the stored form is stable whatever the geometry crate later
//! chooses.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use xarast_geom::Mp;

/// Serialises one millipoint value as its raw count.
pub fn serialize<S: Serializer>(v: &Mp, s: S) -> Result<S::Ok, S::Error> {
    v.raw().serialize(s)
}

/// Reads one millipoint value from its raw count.
pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Mp, D::Error> {
    Ok(Mp::new(i32::deserialize(d)?))
}

/// The same, for the `(x, y)` pairs a grid origin and a page corner use.
pub mod pair {
    use super::*;

    /// Serialises a pair of millipoint values as a pair of raw counts.
    pub fn serialize<S: Serializer>(v: &(Mp, Mp), s: S) -> Result<S::Ok, S::Error> {
        (v.0.raw(), v.1.raw()).serialize(s)
    }

    /// Reads a pair of millipoint values from a pair of raw counts.
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<(Mp, Mp), D::Error> {
        let (x, y) = <(i32, i32)>::deserialize(d)?;
        Ok((Mp::new(x), Mp::new(y)))
    }
}
