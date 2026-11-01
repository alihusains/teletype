//! Test-only fault injector (HARD-3).
//!
//! Compiled only under `#[cfg(feature = "fault-injection")]`, which is OFF
//! by default: a release build contains no injector and no injection point
//! (every production call site is a `#[cfg(feature = "fault-injection")]`
//! block that compiles to nothing otherwise).
//!
//! A named registry of armed faults. Tests arm a fault, drive the real
//! code path, assert the user-visible outcome, and disarm it again to prove
//! the next take is normal. Production code never arms or inspects faults
//! except through the narrow `hit(...)` probes at the injection points.

use std::collections::BTreeSet;
use std::sync::Mutex;

/// The ASR engine's model load fails.
pub const ENGINE_LOAD_ERROR: &str = "engine-load-error";
/// SQLite refuses a write (locked database).
pub const SQLITE_LOCK: &str = "sqlite-lock";
/// A command-mode tool fails (app not found, bad URL, ...).
pub const COMMAND_TOOL_ERROR: &str = "command-tool-error";

static ARMED: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// Process-wide lock that makes fault-injection tests mutually exclusive.
/// Held by the RAII guard returned by [`scoped`]; no other fault test can
/// arm or disarm while a guard is live.
static EXCLUSIVE: Mutex<()> = Mutex::new(());

/// RAII guard that takes the process-wide fault-test lock and clears the
/// registry on drop. Every `#[cfg(feature = "fault-injection")]` test must
/// acquire this as its first statement so no other fault test can interleave
/// and no armed fault leaks into the next test.
pub struct Guard {
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl Drop for Guard {
    fn drop(&mut self) {
        disarm_all();
    }
}

/// Acquires the process-wide fault-test lock and clears the registry.
/// Returns a guard that releases the lock and disarms all faults on drop.
/// Test-only: production code never calls this.
pub fn scoped() -> Guard {
    let lock = EXCLUSIVE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    disarm_all();
    Guard { _lock: lock }
}

fn set(name: &str, armed: bool) {
    let mut guard = ARMED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if armed {
        guard.insert(name.to_owned());
    } else {
        guard.remove(name);
    }
}

/// Arms a named fault. Test-only: production code never arms faults.
pub fn arm(name: &str) {
    set(name, true);
}

/// Disarms a named fault. Test-only.
pub fn disarm(name: &str) {
    set(name, false);
}

/// Disarms every armed fault. Test-only (test teardown safety).
pub fn disarm_all() {
    *ARMED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = BTreeSet::new();
}

/// Whether a named fault is armed.
pub fn is_armed(name: &str) -> bool {
    ARMED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .contains(name)
}

/// Probes a named fault. Returns `true` when armed; the probe never
/// consumes the fault (it stays armed until disarmed).
pub fn hit(name: &str) -> bool {
    is_armed(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arm_hit_disarm_round_trip() {
        // The registry is process-global and the test binary runs tests in
        // parallel threads, so a foreign test may arm a fault at any moment:
        // take the exclusive guard first, then verify the round trip.
        let _guard = scoped();
        assert!(!is_armed(ENGINE_LOAD_ERROR));
        arm(ENGINE_LOAD_ERROR);
        assert!(is_armed(ENGINE_LOAD_ERROR));
        assert!(hit(ENGINE_LOAD_ERROR));
        disarm(ENGINE_LOAD_ERROR);
        assert!(!hit(ENGINE_LOAD_ERROR));
        // A different fault is unaffected.
        arm(SQLITE_LOCK);
        assert!(!hit(ENGINE_LOAD_ERROR));
        assert!(hit(SQLITE_LOCK));
        // The guard's drop disarms everything.
        drop(_guard);
        assert!(!hit(SQLITE_LOCK));
    }
}
