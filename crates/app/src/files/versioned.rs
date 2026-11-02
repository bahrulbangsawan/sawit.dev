//! A value that counts its changes, so what is worked out from it is redone only after one.

use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicU64, Ordering};

/// The next version handed out: no two values, nor two states of one, ever share one.
static NEXT: AtomicU64 = AtomicU64::new(1);

fn next() -> u64 {
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// `T` with a version that changes on each borrow that may change it (`&mut`).
pub(super) struct Versioned<T> {
    value: T,
    version: u64,
}

impl<T> Versioned<T> {
    pub(super) fn version(&self) -> u64 {
        self.version
    }
}

impl<T: Default> Default for Versioned<T> {
    fn default() -> Self {
        Self {
            value: T::default(),
            version: next(),
        }
    }
}

impl<T> Deref for Versioned<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T> DerefMut for Versioned<T> {
    fn deref_mut(&mut self) -> &mut T {
        self.version = next();
        &mut self.value
    }
}
