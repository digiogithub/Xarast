//! A small least-recently-used cache of resident system faces.
//!
//! Capacity is tens of faces, so a linear scan on eviction is cheaper than
//! the bookkeeping of a linked list and keeps this dependency-free.

use std::collections::HashMap;

use super::{FaceData, FaceId};

pub(crate) struct FaceLru {
    capacity: usize,
    tick: u64,
    map: HashMap<FaceId, (FaceData, u64)>,
}

impl FaceLru {
    pub(crate) fn new(capacity: usize) -> FaceLru {
        FaceLru {
            capacity,
            tick: 0,
            map: HashMap::new(),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.map.len()
    }

    pub(crate) fn get(&mut self, id: FaceId) -> Option<FaceData> {
        self.tick = self.tick.wrapping_add(1);
        let tick = self.tick;
        self.map.get_mut(&id).map(|(d, t)| {
            *t = tick;
            d.clone()
        })
    }

    pub(crate) fn insert(&mut self, id: FaceId, data: FaceData) {
        self.tick = self.tick.wrapping_add(1);
        self.map.insert(id, (data, self.tick));
        while self.map.len() > self.capacity {
            let Some(oldest) = self
                .map
                .iter()
                .min_by_key(|(_, (_, t))| *t)
                .map(|(k, _)| *k)
            else {
                break;
            };
            self.map.remove(&oldest);
        }
    }
}
