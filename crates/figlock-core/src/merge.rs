//! Field-level last-writer-wins merge (`sync.md` 6).
//!
//! The state is a pure function of the *set* of writes: applying them in any order, with any duplicates,
//! gives the same result (commutative, associative, idempotent). A write wins if its
//! `(wall, counter, device_id)` is greatest. Snapshots are merged in the same way.

use std::collections::{BTreeMap, BTreeSet};

use crate::hlc::Stamp;

pub type DeviceId = [u8; 16];

/// One field value with the stamp and device that wrote it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Register<V> {
    pub stamp: Stamp,
    pub device: DeviceId,
    pub value: V,
}

impl<V> Register<V> {
    fn key(&self) -> (Stamp, DeviceId) {
        (self.stamp, self.device)
    }
}

/// A write to one field of one item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Write<I, F, V> {
    pub item: I,
    pub field: F,
    pub stamp: Stamp,
    pub device: DeviceId,
    pub value: V,
}

/// Merged state: item -> field -> winning register.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State<I: Ord, F: Ord, V> {
    items: BTreeMap<I, BTreeMap<F, Register<V>>>,
    purge_horizon: Option<Stamp>,
    /// Items known when the snapshot was loaded; ops for other items older than the horizon are ghosts.
    known: BTreeSet<I>,
}

impl<I: Ord + Clone, F: Ord + Clone, V: Clone + PartialEq> Default for State<I, F, V> {
    fn default() -> Self {
        State {
            items: BTreeMap::new(),
            purge_horizon: None,
            known: BTreeSet::new(),
        }
    }
}

impl<I: Ord + Clone, F: Ord + Clone, V: Clone + PartialEq> State<I, F, V> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start from a snapshot: its items are known, and writes for other items stamped before
    /// `purge_horizon` are dropped (`sync.md` 6.4).
    pub fn with_snapshot_scope(
        purge_horizon: Stamp,
        known_items: impl IntoIterator<Item = I>,
    ) -> Self {
        State {
            items: BTreeMap::new(),
            purge_horizon: Some(purge_horizon),
            known: known_items.into_iter().collect(),
        }
    }

    pub fn apply(&mut self, w: Write<I, F, V>) {
        if let Some(h) = self.purge_horizon
            && !self.known.contains(&w.item)
            && w.stamp < h
        {
            return;
        }
        let fields = self.items.entry(w.item).or_default();
        let incoming = Register {
            stamp: w.stamp,
            device: w.device,
            value: w.value,
        };
        match fields.get(&w.field) {
            Some(cur) if cur.key() >= incoming.key() => {}
            _ => {
                fields.insert(w.field, incoming);
            }
        }
    }

    pub fn apply_all(&mut self, writes: impl IntoIterator<Item = Write<I, F, V>>) {
        for w in writes {
            self.apply(w);
        }
    }

    /// Fold another state in (used to apply a snapshot on top of existing state).
    pub fn merge_state(&mut self, other: &Self) {
        for (item, fields) in &other.items {
            for (field, r) in fields {
                self.apply(Write {
                    item: item.clone(),
                    field: field.clone(),
                    stamp: r.stamp,
                    device: r.device,
                    value: r.value.clone(),
                });
            }
        }
    }

    pub fn get(&self, item: &I, field: &F) -> Option<&Register<V>> {
        self.items.get(item)?.get(field)
    }

    pub fn items(&self) -> &BTreeMap<I, BTreeMap<F, Register<V>>> {
        &self.items
    }

    /// Writes that reproduce this state (a snapshot's content).
    pub fn to_writes(&self) -> Vec<Write<I, F, V>> {
        self.items
            .iter()
            .flat_map(|(i, fs)| {
                fs.iter().map(move |(f, r)| Write {
                    item: i.clone(),
                    field: f.clone(),
                    stamp: r.stamp,
                    device: r.device,
                    value: r.value.clone(),
                })
            })
            .collect()
    }
}
