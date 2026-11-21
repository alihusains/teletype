//! Stale-edge protection for bare-modifier event taps.
//!
//! A CGEventTap can be disabled and re-enabled without the app being told
//! about the key's release edge. When that happens the Rust-side "is down"
//! latch would stay set forever and every later press would be discarded as
//! a duplicate. The latch is reset whenever the tap is (re)started, and a
//! down state older than [`STALE_DOWN_MS`] is treated as a fresh press.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// How long a down latch may survive without a matching edge before the next
/// down is accepted as a new press. Longer than any realistic PTT gesture.
pub const STALE_DOWN_MS: u64 = 30_000;

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Edge detector shared by the Fn and bare-modifier taps.
pub struct EdgeLatch {
    down: AtomicBool,
    down_at_ms: AtomicU64,
}

impl EdgeLatch {
    pub const fn new() -> Self {
        Self {
            down: AtomicBool::new(false),
            down_at_ms: AtomicU64::new(0),
        }
    }

    /// Called when the tap starts or stops. The native latches are reset at
    /// the same time; keeping the Rust latch in sync prevents a lost release
    /// from swallowing the first press after a re-registration.
    pub fn reset(&self) {
        self.down.store(false, Ordering::SeqCst);
        self.down_at_ms.store(0, Ordering::SeqCst);
    }

    /// Returns true only for an edge the controller should see.
    pub fn accept(&self, now_down: bool, now_ms: u64) -> bool {
        let was_down = self.down.load(Ordering::SeqCst);
        if now_down == was_down {
            if now_down {
                let pressed_at = self.down_at_ms.load(Ordering::SeqCst);
                if pressed_at != 0 && now_ms.saturating_sub(pressed_at) >= STALE_DOWN_MS {
                    self.down_at_ms.store(now_ms, Ordering::SeqCst);
                    return true;
                }
            }
            return false;
        }
        self.down.store(now_down, Ordering::SeqCst);
        self.down_at_ms
            .store(if now_down { now_ms } else { 0 }, Ordering::SeqCst);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_resets_the_edge_latch() {
        let latch = EdgeLatch::new();
        assert!(latch.accept(true, now_ms()));
        assert!(!latch.accept(true, now_ms()));
        latch.reset();
        assert!(!latch.accept(false, now_ms())); // stray release after restart
        assert!(latch.accept(true, now_ms()));
        assert!(latch.accept(false, now_ms()));
    }

    #[test]
    fn stale_down_after_lost_release_is_recovered() {
        let latch = EdgeLatch::new();
        let pressed_at = now_ms();
        assert!(latch.accept(true, pressed_at));
        assert!(!latch.accept(true, pressed_at + STALE_DOWN_MS - 1));
        assert!(latch.accept(true, pressed_at + STALE_DOWN_MS));
        assert!(latch.accept(false, pressed_at + STALE_DOWN_MS + 10));
    }
}
