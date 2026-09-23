//! Durability (`research/06 §10`): atomic save and the document lock.
//!
//! Autosave, the operation journal and crash recovery (F6.5–F6.7) build on
//! these two and arrive with the document layer, because they serialise a
//! model snapshot rather than bytes.

mod atomic;
mod lock;

pub use atomic::{AtomicOptions, write_atomic, write_atomic_with};
pub use lock::{DocumentLock, LockError, LockHolder, lock_path};
