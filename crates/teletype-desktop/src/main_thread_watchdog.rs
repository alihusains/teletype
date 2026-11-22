//! Main-thread responsiveness watchdog.
//!
//! A wedged main thread is the worst failure this app can have: the window
//! stops repainting, the Dock icon does nothing, and Force Quit is the only way
//! out. v0.2.21 shipped one (a tap teardown that joined a thread whose run loop
//! had not started, see learning L095) and nothing in the product said so —
//! the freeze was only visible as a report from a user.
//!
//! This thread makes the condition observable. Every
//! [`probe_interval`] it queues a no-op closure with `run_on_main_thread`
//! and waits [`probe_timeout`] for the closure to run. A healthy main thread
//! serves the closure between event-loop iterations, in well under a
//! millisecond, so an unserved probe after the timeout means the main thread
//! is not reaching its event loop at all.
//!
//! The "unresponsive" line goes straight to stderr, not through
//! [`crate::log_entry`]: the wedged main thread may be holding the log
//! mutex, and a watchdog that blocks on the lock is worthless. The Developer
//! tab gets the summary once the probe is finally served.
//!
//! Two env seams exist for the release gate
//! (`scripts/check-main-thread-responsive.sh`), which needs a fast cadence
//! instead of the production one and positive proof that probes are still
//! being answered: `TELETYPE_MAIN_THREAD_PROBE_MS`,
//! `TELETYPE_MAIN_THREAD_TIMEOUT_MS` and
//! `TELETYPE_MAIN_THREAD_HEARTBEAT_PROBES` (0, the default, logs nothing per
//! probe; the production log stays quiet until something is wrong).

use std::io::Write as _;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use tauri::AppHandle;

use crate::{log_entry, LogLevel};

/// Default gap between probes.
pub const DEFAULT_PROBE_INTERVAL: Duration = Duration::from_secs(5);
/// Default grace period before a queued probe counts as unserved.
pub const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// A served probe that took longer than this is logged even though it landed.
pub const SLOW_PROBE: Duration = Duration::from_secs(1);
/// Report the first missed probe at once, then once a minute while the episode
/// lasts (12 probes at the production cadence), so a long freeze cannot flood
/// the log.
const REPEAT_EVERY: u32 = 12;
/// Lower bound for the env seams: below this the probe would measure timer
/// granularity rather than the main thread.
const MIN_SEAM: Duration = Duration::from_millis(100);

/// Number of probes the main thread has served, plus the condvar every probe
/// bumps so the watchdog wakes the moment its closure ran.
static SERVED: Mutex<u64> = Mutex::new(0);
static SERVED_CV: Condvar = Condvar::new();

/// Starts the watchdog on its own thread. Safe to call once per process, from
/// the Tauri `setup` hook.
pub fn spawn(app: AppHandle) {
    let spawned = std::thread::Builder::new()
        .name("teletype-main-thread-watchdog".into())
        .spawn(move || run(app));
    if let Err(e) = spawned {
        tracing::warn!("main-thread watchdog did not start: {e}");
    }
}

/// Probe cadence: the production default, overridable for the gate.
pub fn probe_interval() -> Duration {
    seam("TELETYPE_MAIN_THREAD_PROBE_MS", DEFAULT_PROBE_INTERVAL)
}

/// Grace period before a queued probe counts as unserved.
pub fn probe_timeout() -> Duration {
    seam("TELETYPE_MAIN_THREAD_TIMEOUT_MS", DEFAULT_PROBE_TIMEOUT)
}

/// Log a "still responsive" line every this many answered probes. Off (0) by
/// default: the production log says nothing while the main thread is healthy.
pub fn heartbeat_probes() -> u64 {
    std::env::var("TELETYPE_MAIN_THREAD_HEARTBEAT_PROBES")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

fn seam(name: &str, default: Duration) -> Duration {
    match std::env::var(name) {
        Ok(raw) => match raw.trim().parse::<u64>() {
            Ok(ms) => Duration::from_millis(ms).max(MIN_SEAM),
            Err(_) => {
                tracing::warn!("{name}={raw:?} is not a millisecond count; using the default");
                default
            }
        },
        Err(_) => default,
    }
}

fn run(app: AppHandle) {
    let interval = probe_interval();
    let timeout = probe_timeout();
    let heartbeat_every = heartbeat_probes();
    log_entry(
        LogLevel::Info,
        format!(
            "main-thread watchdog: armed (probe every {} ms, timeout {} ms)",
            interval.as_millis(),
            timeout.as_millis()
        ),
    );
    let mut served_count: u64 = 0;
    let mut episode_start: Option<Instant> = None;
    let mut missed_in_episode: u32 = 0;
    loop {
        std::thread::sleep(interval);
        let target = *lock_served() + 1;
        let posted = Instant::now();
        // A closed event loop means the app is exiting: stop, there is nothing
        // left to watch.
        if app
            .run_on_main_thread(|| {
                let mut served = lock_served();
                *served += 1;
                SERVED_CV.notify_all();
            })
            .is_err()
        {
            return;
        }
        if wait_for(target, timeout) {
            served_count += 1;
            let elapsed = posted.elapsed();
            match episode_start.take() {
                Some(start) => {
                    let total = start.elapsed();
                    log_entry(
                        LogLevel::Warn,
                        format!(
                            "main-thread watchdog: main thread answered again after {:.1} s unresponsive ({} missed probe(s))",
                            total.as_secs_f64(),
                            missed_in_episode
                        ),
                    );
                    missed_in_episode = 0;
                }
                None if served_count == 1 => log_entry(
                    LogLevel::Info,
                    format!(
                        "main-thread watchdog: first probe answered in {} ms",
                        elapsed.as_millis()
                    ),
                ),
                None if elapsed >= SLOW_PROBE => log_entry(
                    LogLevel::Warn,
                    format!(
                        "main-thread watchdog: main thread took {} ms to answer a probe",
                        elapsed.as_millis()
                    ),
                ),
                None => {}
            }
            if heartbeat_every > 0 && served_count.is_multiple_of(heartbeat_every) {
                log_entry(
                    LogLevel::Info,
                    format!(
                        "main-thread watchdog: {served_count} probes answered, main thread responsive"
                    ),
                );
            }
            continue;
        }
        missed_in_episode = missed_in_episode.saturating_add(1);
        let start = *episode_start.get_or_insert(posted);
        if should_report(missed_in_episode) {
            report_unresponsive(start.elapsed(), missed_in_episode);
        }
    }
}

fn lock_served() -> std::sync::MutexGuard<'static, u64> {
    SERVED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Waits until the main thread has served `target` probes, or `timeout`
/// expires. Returns whether the probe was served.
fn wait_for(target: u64, timeout: Duration) -> bool {
    wait_on(&SERVED, &SERVED_CV, target, timeout)
}

/// The latch wait itself, on a caller-supplied latch pair. The process has
/// exactly one pair ([`SERVED`] / [`SERVED_CV`]); the tests pass their own so
/// parallel tests cannot satisfy each other's probe.
fn wait_on(latch: &Mutex<u64>, cv: &Condvar, target: u64, timeout: Duration) -> bool {
    let served = latch
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (served, _timed_out) = cv
        .wait_timeout_while(served, timeout, |served| *served < target)
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // The wait ends either because the latch reached `target` or because the
    // timeout expired; the latch is the only thing that answers the question.
    *served >= target
}

/// Report the first missed probe of an episode, then every [`REPEAT_EVERY`]
/// missed probes. Pure so the cadence is testable without a Tauri app.
fn should_report(missed: u32) -> bool {
    missed == 1 || missed.is_multiple_of(REPEAT_EVERY)
}

/// Writes the unresponsive marker to stderr, bypassing the log ring buffer:
/// the wedged main thread may be holding that mutex, and the first miss of an
/// episode is also persisted so the next launch can show it.
fn report_unresponsive(episode: Duration, missed: u32) {
    let line = format!(
        "teletype-watchdog: main thread unresponsive for {} ms ({} probe(s) unserved)\n",
        episode.as_millis(),
        missed
    );
    let mut stderr = std::io::stderr();
    let _ = stderr.write_all(line.as_bytes());
    let _ = stderr.flush();
    if missed == 1 {
        persist_hang(episode, missed);
    }
}

/// Writes one hang report into the same local store as panic reports, so a
/// freeze that the app survives is still visible after the fact. This is what
/// turns a vague "it stopped working" into a timestamp, a version and a
/// measured freeze length.
///
/// `try_lock` on purpose: if the wedged main thread holds that lock, the
/// watchdog must skip the file rather than join the freeze.
fn persist_hang(episode: Duration, missed: u32) {
    let Ok(slot) = crate::CRASH_DATA_DIR.try_lock() else {
        return;
    };
    let Some(data_dir) = slot.clone() else {
        return; // setup has not run yet, or an e2e run (no reports by design)
    };
    drop(slot);
    if let Err(e) = crate::crash::write_hang_report(
        &crate::crash::crash_dir(&data_dir),
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        episode.as_millis() as u64,
        missed,
    ) {
        eprintln!("teletype-watchdog: hang report not written: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_the_first_miss_then_once_a_minute() {
        assert!(should_report(1));
        // Quiet while the episode is young: one line is the signal, more is noise.
        assert!(!should_report(2));
        assert!(!should_report(11));
        assert!(should_report(12));
        assert!(!should_report(13));
        assert!(should_report(24));
    }

    #[test]
    fn a_served_probe_wins_the_wait() {
        // The "main thread" here is a helper thread pressing the same kind of
        // latch the event loop presses. Own latch, so a parallel test cannot
        // satisfy this probe for us.
        let latch = Mutex::new(0u64);
        let cv = Condvar::new();
        let served = std::thread::scope(|scope| {
            let latch = &latch;
            let cv = &cv;
            scope.spawn(move || {
                std::thread::sleep(Duration::from_millis(20));
                *latch
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = 1;
                cv.notify_all();
            });
            wait_on(latch, cv, 1, Duration::from_secs(2))
        });
        assert!(served);
    }

    #[test]
    fn an_unserved_probe_times_out() {
        let latch = Mutex::new(0u64);
        let cv = Condvar::new();
        let started = Instant::now();
        assert!(!wait_on(&latch, &cv, 1, Duration::from_millis(150)));
        assert!(started.elapsed() >= Duration::from_millis(150));
    }

    #[test]
    fn env_seams_clamp_and_default() {
        // Unset: the production cadence.
        assert_eq!(
            seam(
                "TELETYPE_MAIN_THREAD_PROBE_MS_UNSET_CASE",
                DEFAULT_PROBE_INTERVAL
            ),
            DEFAULT_PROBE_INTERVAL
        );
        // Garbage: also the production cadence, not a panic.
        std::env::set_var("TELETYPE_MAIN_THREAD_TEST_SEAM_MS", "soon");
        assert_eq!(
            seam("TELETYPE_MAIN_THREAD_TEST_SEAM_MS", DEFAULT_PROBE_INTERVAL),
            DEFAULT_PROBE_INTERVAL
        );
        // Below the floor the value is clamped, not honored verbatim.
        std::env::set_var("TELETYPE_MAIN_THREAD_TEST_SEAM_MS", "1");
        assert_eq!(
            seam("TELETYPE_MAIN_THREAD_TEST_SEAM_MS", DEFAULT_PROBE_INTERVAL),
            MIN_SEAM
        );
        std::env::remove_var("TELETYPE_MAIN_THREAD_TEST_SEAM_MS");
    }
}
