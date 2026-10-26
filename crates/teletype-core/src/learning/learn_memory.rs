//! Learn memory: who the user said "no" to (Phase: EW port rows 1–3).
//!
//! When the user undoes a learned correction, the (misheard, canonical) pair
//! is remembered here. Two refusals block the pair permanently; promotion
//! honours the block (see `promote::consider_mishearing`), and the Forget
//! control wipes everything.
//!
//! Storage: one `learn_memory` table in the existing SQLite [`LearningStore`]
//! (no new file, no new lock). All policy lives in this module; the store
//! methods are thin row writes.

use super::store::{LearnMemoryRow, LearningStore, StoreResult};

/// Refusals on one pair at or above which the pair becomes permanently blocked.
pub const REFUSALS_TO_BLOCK: i64 = 2;
/// Cap on total refused pairs (refusals + blocks combined).
pub const REFUSED_PAIR_CAP: usize = 2_000;
/// Cap on permanently blocked pairs.
pub const BLOCKED_PAIR_CAP: usize = 10_000;
/// Plain (non-blocking) refusals older than this are treated as absent.
pub const REFUSAL_TTL_DAYS: i64 = 90;

const DAY_MS: u64 = 24 * 60 * 60 * 1000;

/// Normalizes a learn-memory key half: lowercase, trimmed.
pub fn normalize_key(s: &str) -> String {
    s.trim().to_lowercase()
}

fn key_pair(misheard: &str, canonical: &str) -> (String, String) {
    (normalize_key(misheard), normalize_key(canonical))
}

/// Reads the refusal state for a pair, treating a plain refusal older than
/// [`REFUSAL_TTL_DAYS`] as absent (and sweeping it while here).
fn effective_row(store: &LearningStore, misheard: &str, canonical: &str, now_ms: u64) -> StoreResult<Option<LearnMemoryRow>> {
    let (m, c) = key_pair(misheard, canonical);
    if m.is_empty() || c.is_empty() {
        return Ok(None);
    }
    let Some(row) = store.learn_memory_row(&m, &c)? else {
        return Ok(None);
    };
    // TTL sweep: a plain refusal is not "blocked at two", so only count < 2
    // rows expire.
    if !row.blocked && row.no_count < REFUSALS_TO_BLOCK {
        let cutoff = now_ms.saturating_sub((REFUSAL_TTL_DAYS as u64).saturating_mul(DAY_MS));
        if row.last_no < cutoff {
            store.clear_learn_memory(&m, &c)?;
            return Ok(None);
        }
    }
    Ok(Some(row))
}

/// Records one refusal ("no") for `(misheard, canonical)`.
///
/// Increments the pair's refusal count; returns `true` when this call
/// crosses into blocked (`no_count >= 2`). An empty `misheard` or
/// `canonical` is ignored (nothing stored, returns `false`). Blocked pairs
/// are permanent: further refusals refresh the timestamp but keep the pair
/// blocked. At a cap, the oldest plain refusal is dropped to make room.
pub fn record_refusal(
    store: &LearningStore,
    misheard: &str,
    canonical: &str,
    now_ms: u64,
) -> StoreResult<bool> {
    let (m, c) = key_pair(misheard, canonical);
    if m.is_empty() || c.is_empty() {
        return Ok(false);
    }
    let existing = effective_row(store, &m, &c, now_ms)?;
    let mut row = match existing {
        Some(r) => LearnMemoryRow {
            no_count: r.no_count.saturating_add(1),
            blocked: r.blocked || r.no_count.saturating_add(1) >= REFUSALS_TO_BLOCK,
            last_no: now_ms,
            ..r
        },
        None => LearnMemoryRow {
            misheard: m.clone(),
            canonical: c.clone(),
            no_count: 1,
            blocked: false,
            last_no: now_ms,
        },
    };

    // Caps: drop the oldest plain refusal rather than refusing the new one.
    let total = store.learn_memory_count()?;
    if total >= REFUSED_PAIR_CAP && !row.blocked {
        store.delete_oldest_learn_memory()?;
    }
    if store.blocked_learn_memory_count()? >= BLOCKED_PAIR_CAP && row.blocked {
        // Blocked cap reached and this refusal would create a new block:
        // keep the refusal visible (still counted) but do not escalate it.
        // In practice the total cap above already bounds the table; this is
        // belt-and-braces for the 10k blocked bound.
        row.blocked = false;
        row.no_count = row.no_count.min(REFUSALS_TO_BLOCK - 1);
    }

    store.upsert_learn_memory(&row)?;
    Ok(row.blocked)
}

/// True when the pair is permanently blocked by user refusals.
pub fn is_blocked(store: &LearningStore, misheard: &str, canonical: &str, now_ms: u64) -> StoreResult<bool> {
    Ok(effective_row(store, misheard, canonical, now_ms)?.is_some_and(|r| r.blocked))
}

/// Number of permanently blocked pairs (the Forget row's "{N} blocked").
pub fn blocked_count(store: &LearningStore) -> StoreResult<usize> {
    store.blocked_learn_memory_count()
}

/// Deletes every refusal and block. Returns the number of rows removed.
pub fn forget_memory(store: &LearningStore) -> StoreResult<usize> {
    store.delete_all_learn_memory()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_store() -> (tempfile::TempDir, LearningStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = LearningStore::open(&dir.path().join("learning.sqlite")).unwrap();
        (dir, store)
    }

    const T0: u64 = 1_000_000_000_000; // fixed "now" for deterministic tests
    const DAY: u64 = DAY_MS;

    #[test]
    fn two_refusals_block_one_does_not() {
        let (_dir, store) = tmp_store();
        let b1 = record_refusal(&store, "pie torch", "PyTorch", T0).unwrap();
        assert!(!b1, "first refusal must not block");
        assert!(!is_blocked(&store, "pie torch", "PyTorch", T0).unwrap());

        let b2 = record_refusal(&store, "PIE TORCH", "pytorch", T0 + 1000).unwrap();
        assert!(b2, "second refusal must block");
        assert!(is_blocked(&store, "pie torch", "PyTorch", T0 + 2000).unwrap());
        assert_eq!(blocked_count(&store).unwrap(), 1);

        // Key is normalized: uppercased/trimmed input hits the same row.
        let row = store
            .learn_memory_row("pie torch", "pytorch")
            .unwrap()
            .unwrap();
        assert_eq!(row.no_count, 2);
        assert!(row.blocked);
    }

    #[test]
    fn empty_keys_are_ignored() {
        let (_dir, store) = tmp_store();
        assert!(!record_refusal(&store, "", "PyTorch", T0).unwrap());
        assert!(!record_refusal(&store, "  ", "PyTorch", T0).unwrap());
        assert!(!record_refusal(&store, "pie torch", "   ", T0).unwrap());
        assert_eq!(store.learn_memory_count().unwrap(), 0);
    }

    #[test]
    fn blocked_pair_is_permanent() {
        let (_dir, store) = tmp_store();
        record_refusal(&store, "a", "b", T0).unwrap();
        record_refusal(&store, "a", "b", T0 + 1).unwrap();
        // A third refusal keeps it blocked and does not unblock.
        let b3 = record_refusal(&store, "a", "b", T0 + 2).unwrap();
        assert!(b3);
        assert!(is_blocked(&store, "a", "b", T0 + 3).unwrap());
        assert_eq!(store.learn_memory_count().unwrap(), 1);
    }

    #[test]
    fn forget_memory_removes_everything_and_returns_count() {
        let (_dir, store) = tmp_store();
        record_refusal(&store, "a", "b", T0).unwrap();
        record_refusal(&store, "a", "b", T0 + 1).unwrap(); // blocks
        record_refusal(&store, "c", "d", T0).unwrap();
        assert_eq!(store.learn_memory_count().unwrap(), 2);

        let n = forget_memory(&store).unwrap();
        assert_eq!(n, 2);
        assert_eq!(store.learn_memory_count().unwrap(), 0);
        assert!(!is_blocked(&store, "a", "b", T0).unwrap());
        assert_eq!(blocked_count(&store).unwrap(), 0);
        // Forget is idempotent.
        assert_eq!(forget_memory(&store).unwrap(), 0);
    }

    #[test]
    fn refusal_cap_drops_oldest_instead_of_erroring() {
        let (_dir, store) = tmp_store();
        // Fill to the cap with plain (unblocked) refusals, oldest first.
        for i in 0..REFUSED_PAIR_CAP as u64 {
            let blocked =
                record_refusal(&store, &format!("mh{i}"), &format!("cn{i}"), T0 + i).unwrap();
            assert!(!blocked);
        }
        assert_eq!(store.learn_memory_count().unwrap(), REFUSED_PAIR_CAP);

        // One more refusal: succeeds, and the oldest plain refusal drops.
        let blocked = record_refusal(&store, "newest", "newest", T0 + 100_000).unwrap();
        assert!(!blocked);
        assert_eq!(store.learn_memory_count().unwrap(), REFUSED_PAIR_CAP);
        assert!(!is_blocked(&store, "mh0", "cn0", T0 + 200_000).unwrap());
        assert!(store.learn_memory_row("newest", "newest").unwrap().is_some());
    }

    #[test]
    fn old_plain_refusal_does_not_block_and_is_swept() {
        let (_dir, store) = tmp_store();
        let stale = T0.saturating_sub((REFUSAL_TTL_DAYS as u64 + 1) * DAY);
        // Refuse once 91 days ago: treated as absent on read.
        record_refusal(&store, "old", "pair", stale).unwrap();
        assert!(store.learn_memory_row("old", "pair").unwrap().is_some());
        assert!(!is_blocked(&store, "old", "pair", T0).unwrap());
        // The read swept it.
        assert!(store.learn_memory_row("old", "pair").unwrap().is_none());
    }
}
