//! Supervised restart for llama-server child processes.
//!
//! When a llama-server child dies while a take needs it, the provider
//! restarts it once. A second death falls back to the raw transcript with
//! a message. This module tracks whether a restart has already been
//! attempted so the provider does not restart in a loop.

use std::sync::atomic::{AtomicBool, Ordering};

/// Tracks whether a supervised restart has already been attempted.
///
/// Created per-provider at construction. The provider checks
/// [`may_restart`] before attempting a restart; if it returns `true`,
/// the provider restarts the child and calls [`mark_restarted`].
/// Subsequent calls to [`may_restart`] return `false`, so the provider
/// falls back to the raw transcript instead of restarting again.
#[derive(Debug, Default)]
pub struct ChildRestartTracker {
    restarted: AtomicBool,
}

impl ChildRestartTracker {
    /// Creates a new tracker (no restart attempted yet).
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` if a restart has not yet been attempted.
    /// Consumes the restart budget atomically: returns `true` exactly
    /// once per tracker lifetime.
    pub fn may_restart(&self) -> bool {
        !self.restarted.swap(true, Ordering::SeqCst)
    }

    /// Whether a restart has already been attempted.
    #[cfg(test)]
    pub fn is_restarted(&self) -> bool {
        self.restarted.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_exactly_one_restart() {
        let tracker = ChildRestartTracker::new();
        assert!(tracker.may_restart(), "first call allows restart");
        assert!(!tracker.may_restart(), "second call denies restart");
        assert!(tracker.is_restarted());
    }

    #[test]
    fn default_is_not_restarted() {
        let tracker = ChildRestartTracker::default();
        assert!(!tracker.is_restarted());
        assert!(tracker.may_restart());
    }
}
