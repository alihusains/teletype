//! Saved take audio for the Dictation tab's player.
//!
//! Each take with `save_recordings` on is written as `<entry id>.wav`
//! (16 kHz mono 16-bit, about 1.9 MB per minute) in `AppState::recordings_dir`
//! (`<local app data>/recordings`, never the roaming profile on Windows).
//! The folder is owner-only (0700) and each file 0600 on Unix, the same
//! policy as the escape-recovery spool, since both hold the user's voice.
//! The history row stores the file name only. Every path is built through
//! [`resolve`], which accepts nothing but a uuid-shaped `.wav` name and
//! refuses anything that canonicalises outside the folder, so no path from
//! the UI or a tampered row can reach another file.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use teletype_core::history::DictationEntry;

/// Held while a finished take's recording link is published (snapshot and
/// SQLite) and while "Delete all recordings" runs. A take's WAV is written
/// before its entry is published, so without this a Delete all landing in
/// between would leave the new row pointing at a file it just removed.
pub static PUBLISH_LOCK: Mutex<()> = Mutex::new(());

/// The sample rate every take is stored at (the ASR buffer's rate).
pub const SAMPLE_RATE: u32 = 16_000;

/// Refuse to hand the UI a recording bigger than this (about 35 minutes).
pub const MAX_PLAYBACK_BYTES: u64 = 64 * 1024 * 1024;

/// Total stored recordings, for the settings row.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingsUsage {
    pub count: u32,
    pub bytes: u64,
}

/// `<base>/recordings`. `base` is the local app data folder.
pub fn dir(base: &Path) -> PathBuf {
    base.join("recordings")
}

/// Creates the recordings folder, owner-only on Unix (an existing folder is
/// tightened too).
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(dir)
    }
}

/// Writes `bytes` to a new file that only the owner can read on Unix.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    // A stale file from a crash would make `create_new` fail forever.
    let _ = std::fs::remove_file(path);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// The path as Explorer's `/select,` accepts it: without the `\\?\`
/// verbatim prefix `canonicalize` adds on Windows (Explorer ignores such a
/// path and opens its default view instead).
#[cfg_attr(not(windows), allow(dead_code))] // called by the Windows reveal; tested everywhere
pub fn explorer_path(path: &str) -> String {
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = path.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        path.to_string()
    }
}

/// The entry's recording name, only while its file is still on disk.
pub fn still_present(recordings_dir: &Path, stored: Option<String>) -> Option<String> {
    stored.filter(|name| is_recording_name(name) && recordings_dir.join(name).is_file())
}

/// True for a 36-character uuid-shaped stem (lowercase hex and dashes).
fn is_uuid_stem(stem: &str) -> bool {
    stem.len() == 36
        && stem
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b) || b == b'-')
}

/// True for a stored recording name: `^[0-9a-f-]{36}\.wav$`.
pub fn is_recording_name(name: &str) -> bool {
    name.strip_suffix(".wav").is_some_and(is_uuid_stem)
}

/// True for a half-written recording left by a crash.
fn is_stale_tmp(name: &str) -> bool {
    name.strip_suffix(".wav.tmp").is_some_and(is_uuid_stem)
}

/// The only way a recording path is built. `stored` must be a bare
/// `<uuid>.wav` name, and the file must canonicalise inside `dir` (a
/// symlink pointing elsewhere is refused). The error never echoes a path.
pub fn resolve(dir: &Path, stored: &str) -> Result<PathBuf, String> {
    if !is_recording_name(stored) {
        return Err("This recording has an invalid file name.".into());
    }
    let canonical_dir = dir
        .canonicalize()
        .map_err(|_| "The recordings folder is missing.".to_string())?;
    let file = dir
        .join(stored)
        .canonicalize()
        .map_err(|_| "This recording file is missing.".to_string())?;
    if !file.starts_with(&canonical_dir) {
        return Err("This recording is outside the recordings folder.".into());
    }
    Ok(file)
}

/// Writes `samples` as a 16-bit mono WAV through `<path>.tmp` and a rename,
/// so a crash never leaves a half-written file that a row points at.
pub fn write_wav16(path: &Path, samples: &[f32], rate: u32) -> std::io::Result<()> {
    let data_len = samples.len() * 2;
    let mut buf = Vec::with_capacity(44 + data_len);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
    buf.extend_from_slice(b"WAVE");
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes()); // PCM chunk size
    buf.extend_from_slice(&1u16.to_le_bytes()); // audio format = PCM
    buf.extend_from_slice(&1u16.to_le_bytes()); // channels = mono
    buf.extend_from_slice(&rate.to_le_bytes());
    buf.extend_from_slice(&(rate * 2).to_le_bytes()); // byte rate
    buf.extend_from_slice(&2u16.to_le_bytes()); // block align
    buf.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&(data_len as u32).to_le_bytes());
    for &s in samples {
        buf.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    write_private(&tmp, &buf)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Saves the take audio for `entry` and sets its `recording_path` on
/// success. On any failure the entry keeps no audio and a warning is logged:
/// the dictation is never lost because of its recording.
pub fn attach(
    recordings_dir: &Path,
    mut entry: DictationEntry,
    audio: Option<&[f32]>,
) -> DictationEntry {
    let Some(samples) = audio.filter(|s| !s.is_empty()) else {
        return entry;
    };
    let name = format!("{}.wav", entry.id);
    if !is_recording_name(&name) {
        tracing::warn!("recordings: entry id is not a uuid; audio not saved");
        return entry;
    }
    let result = create_private_dir(recordings_dir)
        .and_then(|()| write_wav16(&recordings_dir.join(&name), samples, SAMPLE_RATE));
    match result {
        Ok(()) => entry.recording_path = Some(name),
        Err(e) => tracing::warn!("recordings: could not save take audio: {e}"),
    }
    entry
}

/// Deletes one stored recording. A missing file is not an error.
pub fn delete(recordings_dir: &Path, stored: &str) -> Result<(), String> {
    if !is_recording_name(stored) {
        return Err("This recording has an invalid file name.".into());
    }
    let path = match resolve(recordings_dir, stored) {
        Ok(p) => p,
        // Nothing on disk (or no folder): already gone.
        Err(_) if !recordings_dir.join(stored).exists() => return Ok(()),
        Err(e) => return Err(e),
    };
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Could not delete the recording: {}", e.kind())),
    }
}

/// Recording files directly in `recordings_dir` (no recursion), with sizes.
fn list(recordings_dir: &Path) -> Vec<(PathBuf, String, u64)> {
    let Ok(read) = std::fs::read_dir(recordings_dir) else {
        return vec![];
    };
    read.flatten()
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            if !meta.is_file() {
                return None;
            }
            let name = e.file_name().to_str()?.to_owned();
            Some((e.path(), name, meta.len()))
        })
        .collect()
}

/// How many recordings are stored and how much space they use.
pub fn usage(recordings_dir: &Path) -> RecordingsUsage {
    list(recordings_dir)
        .into_iter()
        .filter(|(_, name, _)| is_recording_name(name))
        .fold(RecordingsUsage::default(), |acc, (_, _, len)| {
            RecordingsUsage {
                count: acc.count + 1,
                bytes: acc.bytes + len,
            }
        })
}

/// Deletes every stored recording and stale `.wav.tmp`, touching nothing
/// else: other files and subfolders are left alone. Returns what remains.
pub fn delete_all(recordings_dir: &Path) -> RecordingsUsage {
    for (path, name, _) in list(recordings_dir) {
        if is_recording_name(&name) || is_stale_tmp(&name) {
            if let Err(e) = std::fs::remove_file(&path) {
                tracing::warn!("recordings: could not delete a recording: {}", e.kind());
            }
        }
    }
    usage(recordings_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0b8f5f8e-7c2a-4d5e-9a1b-2c3d4e5f6a7b";

    #[test]
    fn write_wav16_header_and_size() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join(format!("{ID}.wav"));
        let samples = vec![0.0f32, 0.5, -0.5, 1.0, -1.0];
        write_wav16(&path, &samples, SAMPLE_RATE).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes.len(), 44 + 2 * samples.len());
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(u16::from_le_bytes([bytes[22], bytes[23]]), 1, "mono");
        assert_eq!(
            u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]),
            16_000
        );
        assert_eq!(u16::from_le_bytes([bytes[34], bytes[35]]), 16, "16-bit");
        assert_eq!(&bytes[36..40], b"data");
        // The tmp file was renamed away.
        let names: Vec<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec![format!("{ID}.wav")]);
    }

    #[test]
    fn resolve_rejects_anything_but_a_contained_uuid_wav() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().join("recordings");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{ID}.wav")), b"x").unwrap();
        for bad in [
            "../x.wav",
            "/etc/passwd",
            "C:\\x.wav",
            "a/b.wav",
            "not-a-uuid.wav",
            "0B8F5F8E-7C2A-4D5E-9A1B-2C3D4E5F6A7B.wav",
            &format!("{ID}.mp3"),
            &format!("../{ID}.wav"),
        ] {
            assert!(resolve(&dir, bad).is_err(), "{bad} must be refused");
        }
        let ok = resolve(&dir, &format!("{ID}.wav")).unwrap();
        assert!(ok.ends_with(format!("{ID}.wav")));
    }

    #[cfg(unix)]
    #[test]
    fn resolve_rejects_a_symlink_out_of_the_folder() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().join("recordings");
        std::fs::create_dir_all(&dir).unwrap();
        let outside = tmp.path().join("secret.wav");
        std::fs::write(&outside, b"x").unwrap();
        std::os::unix::fs::symlink(&outside, dir.join(format!("{ID}.wav"))).unwrap();
        let err = resolve(&dir, &format!("{ID}.wav")).unwrap_err();
        assert!(!err.contains("secret"), "error must not echo a path");
    }

    #[test]
    fn delete_of_a_missing_file_is_ok() {
        let tmp = tempfile::TempDir::new().unwrap();
        assert!(delete(tmp.path(), &format!("{ID}.wav")).is_ok());
        assert!(delete(&tmp.path().join("nope"), &format!("{ID}.wav")).is_ok());
        assert!(delete(tmp.path(), "../x.wav").is_err());
        std::fs::write(tmp.path().join(format!("{ID}.wav")), b"x").unwrap();
        delete(tmp.path(), &format!("{ID}.wav")).unwrap();
        assert!(!tmp.path().join(format!("{ID}.wav")).exists());
    }

    #[test]
    fn delete_all_leaves_other_files_and_folders() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join(format!("{ID}.wav")), b"1234").unwrap();
        std::fs::write(
            dir.join("11111111-2222-3333-4444-555555555555.wav.tmp"),
            b"x",
        )
        .unwrap();
        std::fs::write(dir.join("notes.txt"), b"keep").unwrap();
        std::fs::write(dir.join("other.wav"), b"keep").unwrap();
        std::fs::create_dir(dir.join("sub")).unwrap();
        std::fs::write(
            dir.join("sub")
                .join("22222222-2222-3333-4444-555555555555.wav"),
            b"keep",
        )
        .unwrap();
        assert_eq!(usage(dir), RecordingsUsage { count: 1, bytes: 4 });
        assert_eq!(delete_all(dir), RecordingsUsage::default());
        assert!(dir.join("notes.txt").exists());
        assert!(dir.join("other.wav").exists());
        assert!(dir
            .join("sub")
            .join("22222222-2222-3333-4444-555555555555.wav")
            .exists());
        assert!(!dir.join(format!("{ID}.wav")).exists());
    }

    fn entry() -> DictationEntry {
        DictationEntry {
            id: ID.into(),
            text: "hi".into(),
            ..Default::default()
        }
    }

    #[test]
    fn attach_without_audio_or_with_a_failed_write_keeps_the_entry() {
        let tmp = tempfile::TempDir::new().unwrap();
        // Recordings off: no audio reaches the job.
        let e = attach(&tmp.path().join("recordings"), entry(), None);
        assert!(e.recording_path.is_none());
        assert_eq!(e.text, "hi");
        // The folder path is a file, so the write fails.
        let blocker = tmp.path().join("blocked");
        std::fs::write(&blocker, b"x").unwrap();
        let e = attach(&blocker, entry(), Some(&[0.1, 0.2]));
        assert!(e.recording_path.is_none());
        assert_eq!(e.text, "hi");
    }

    #[cfg(unix)]
    #[test]
    fn recordings_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().join("recordings");
        let e = attach(&dir, entry(), Some(&[0.1; 16]));
        let file = dir.join(e.recording_path.unwrap());
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&file), 0o600, "recording file");
        assert_eq!(mode(&dir), 0o700, "recordings folder");

        // A folder an older build created world-readable is tightened.
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut second = entry();
        second.id = "11111111-2222-3333-4444-555555555555".into();
        attach(&dir, second, Some(&[0.1; 16]));
        assert_eq!(mode(&dir), 0o700);
    }

    #[test]
    fn explorer_path_drops_the_verbatim_prefix() {
        assert_eq!(
            explorer_path(r"\\?\C:\Users\Some User\rec\a.wav"),
            r"C:\Users\Some User\rec\a.wav"
        );
        assert_eq!(
            explorer_path(r"\\?\UNC\server\share\a.wav"),
            r"\\server\share\a.wav"
        );
        assert_eq!(explorer_path(r"C:\x\a.wav"), r"C:\x\a.wav");
    }

    #[test]
    fn still_present_drops_a_deleted_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let name = format!("{ID}.wav");
        assert_eq!(still_present(tmp.path(), Some(name.clone())), None);
        std::fs::write(tmp.path().join(&name), b"x").unwrap();
        assert_eq!(still_present(tmp.path(), Some(name.clone())), Some(name));
        assert_eq!(still_present(tmp.path(), Some("../x.wav".into())), None);
        assert_eq!(still_present(tmp.path(), None), None);
    }

    #[test]
    fn attach_saves_and_names_the_file_after_the_entry() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().join("recordings");
        let e = attach(&dir, entry(), Some(&[0.1; 160]));
        assert_eq!(
            e.recording_path.as_deref(),
            Some(format!("{ID}.wav").as_str())
        );
        let path = resolve(&dir, e.recording_path.as_deref().unwrap()).unwrap();
        assert_eq!(std::fs::metadata(path).unwrap().len(), 44 + 320);
    }
}
