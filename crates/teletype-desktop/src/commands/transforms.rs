//! Transform commands: CRUD, shortcuts, selection transform and preview.
//!
//! Split out of `commands.rs` (roadmap S1): pure move, no behavior change.
use super::*;
use crate::AppState;
use tauri::{AppHandle, Manager, State};
use teletype_core::transforms::TransformDefinition;
// ---- Transforms ----
/// A shortcut that is only modifier keys ("Alt", "Cmd+Shift") can never be
/// registered as a global hotkey — the global-shortcut plugin rejects it with
/// "Couldn't recognize X as a valid key". Such values used to be saved by the
/// old capture panel (which committed on modifier release), leaving a binding
/// that looks set in the UI but fires nowhere. Reject them at the save
/// boundary so the store and the OS bindings can never drift.
fn reject_bare_modifier_shortcut(shortcut: &str) -> Result<(), String> {
    let trimmed = shortcut.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    let parts: Vec<&str> = trimmed.split('+').collect();
    let bare_mods = ["Cmd", "Ctrl", "Alt", "Shift", "Fn"];
    if parts.iter().all(|p| bare_mods.contains(p)) {
        Err(format!(
            "\"{trimmed}\" is only modifier keys — add a regular key (e.g. \"{trimmed}+Space\")."
        ))
    } else {
        Ok(())
    }
}
/// (Re)registers the global shortcut for every transform that has one.
/// Idempotent: it unregisters every transform's shortcut before registering,
/// so changing a shortcut from "Alt+2" to "Alt+1" does not leave the old
/// Carbon hotkey alive (which would make the new one appear dead).
pub fn sync_transform_shortcuts(app: &AppHandle, state: &AppState) {
    // macOS: transforms run through the unified InputEngine's ShortcutManager,
    // which consumes the keystroke so an Alt/Option combo like Alt+1 never
    // reaches the focused app. Carbon RegisterEventHotKey does not swallow,
    // so it cannot be used for Alt combos.
    #[cfg(target_os = "macos")]
    {
        // Clear any legacy Carbon registrations from before the tap existed,
        // so a changed binding cannot leave the old hotkey alive.
        let transforms = state.transforms.get().transforms.clone();
        for t in &transforms {
            if !t.shortcut.trim().is_empty() {
                state
                    .controller
                    .unregister_transform_shortcut(app, &t.shortcut);
            }
        }
        if crate::input_engine::rebuild(app, state) {
            return;
        }
        // Tap failed (usually missing Accessibility): fall back to Carbon so
        // Cmd/Ctrl chords still work. Alt combos will type their character
        // in this degraded mode, which the log makes explicit.
        crate::log_entry(
            crate::LogLevel::Warn,
            "input engine unavailable, falling back to Carbon (Alt combos will type)".to_string(),
        );
    }
    {
        let transforms = state.transforms.get().transforms.clone();
        // Unregister every non-empty shortcut first, so a changed binding does
        // not leave the old Carbon hotkey registered alongside the new one.
        // On macOS this is a second unregister when the tap failed; harmless.
        // On macOS tap success we already returned, so this Carbon path only
        // runs for non-macOS or degraded fallback, never alongside the tap.
        for t in &transforms {
            if !t.shortcut.trim().is_empty() {
                state
                    .controller
                    .unregister_transform_shortcut(app, &t.shortcut);
            }
        }
        for t in &transforms {
            if t.enabled && !t.shortcut.trim().is_empty() {
                // macOS tap success already returned, so reaching here on macOS
                // means degraded Carbon fallback. Non-macOS always uses Carbon.
                #[cfg(target_os = "macos")]
                {
                    // Skip Carbon registration when the tap owns the binding:
                    // the tap path already returned on success, so this is
                    // fallback-only. Still register via Carbon for visibility.
                }
                if let Err(e) =
                    state
                        .controller
                        .register_transform_shortcut(app, &t.shortcut, &t.id)
                {
                    crate::log_entry(
                        crate::LogLevel::Warn,
                        format!("transform shortcut for '{}' not registered: {e}", t.name),
                    );
                    tracing::warn!("transform shortcut {} not registered: {e}", t.name);
                } else {
                    crate::log_entry(
                        crate::LogLevel::Info,
                        format!(
                            "transform shortcut for '{}' registered: {}",
                            t.name, t.shortcut
                        ),
                    );
                }
            }
        }
    }
}
/// (Re)registers the Quick Add global shortcut (item 8): the selected word
/// in the focused app is added to the dictionary. Idempotent — it unregisters
/// the previous binding first, so saving a new keybinding (or clearing it)
/// keeps the OS bindings in sync.
pub fn sync_quick_add_shortcut(app: &AppHandle, state: &AppState) {
    // macOS: Quick Add shares the unified engine's ShortcutManager with
    // transforms, so an Alt combo never types into the focused app. Rebuild
    // covers both.
    #[cfg(target_os = "macos")]
    {
        let hotkey = state.settings().quick_add_hotkey.clone();
        if !hotkey.trim().is_empty() {
            // Clear any legacy Carbon registration for migration.
            use tauri_plugin_global_shortcut::GlobalShortcutExt;
            let _ = app.global_shortcut().unregister(hotkey.as_str());
        }
        crate::input_engine::rebuild(app, state);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let hotkey = state.settings().quick_add_hotkey.clone();
        state.controller.register_quick_add_shortcut(app, &hotkey);
    }
}
#[tauri::command]
pub async fn list_transforms(
    state: State<'_, AppState>,
) -> CommandResult<Vec<TransformDefinition>> {
    Ok(state.transforms.get().transforms.clone())
}
#[tauri::command]
pub async fn create_transform(
    app: AppHandle,
    state: State<'_, AppState>,
    transform: TransformDefinition,
) -> CommandResult<TransformDefinition> {
    reject_bare_modifier_shortcut(&transform.shortcut)?;
    let mut transforms = (*state.transforms.get()).clone();
    transforms.insert(transform.clone())?;
    state.transforms.update(|s| *s = transforms)?;
    sync_transform_shortcuts(&app, &state);
    Ok(transform)
}
#[tauri::command]
pub async fn update_transform(
    app: AppHandle,
    state: State<'_, AppState>,
    transform: TransformDefinition,
) -> CommandResult<TransformDefinition> {
    reject_bare_modifier_shortcut(&transform.shortcut)?;
    let mut transforms = (*state.transforms.get()).clone();
    transforms.update(transform.clone())?;
    state.transforms.update(|s| *s = transforms)?;
    sync_transform_shortcuts(&app, &state);
    Ok(transform)
}
#[tauri::command]
pub async fn delete_transform(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<()> {
    let mut transforms = (*state.transforms.get()).clone();
    transforms.remove(&id)?;
    state.transforms.update(|s| *s = transforms)?;
    sync_transform_shortcuts(&app, &state);
    Ok(())
}
#[tauri::command]
pub async fn reset_transforms(app: AppHandle, state: State<'_, AppState>) -> CommandResult<usize> {
    let mut store = (*state.transforms.get()).clone();
    let n = store.reset_built_ins();
    state.transforms.update(|s| *s = store)?;
    sync_transform_shortcuts(&app, &state);
    Ok(n)
}
/// What the "Show me" preview needs: the output plus whether the model
/// actually produced it. A silent fallback (skip reason) or a model that
/// echoes its input must not be presented as a successful polish.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestTransformResult {
    pub text: String,
    pub transformed: bool,
    pub skip_message: Option<String>,
}

#[tauri::command]
pub async fn test_transform(
    state: State<'_, AppState>,
    transform_id: String,
    input: String,
) -> CommandResult<TestTransformResult> {
    let transforms = (*state.transforms.get()).clone();
    let transform = transforms
        .get(&transform_id)
        .ok_or("Transform not found")?
        .clone();
    let engine = state
        .inference
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .ok_or("No model loaded")?;
    let provider = engine.as_ref();
    let profile = (*state.profile.get()).clone();
    let app_ctx = state.platform.active_application().unwrap_or_default();
    let packet = teletype_core::personalization::packet::resolve(&profile, &app_ctx);
    let prompt_ctx = teletype_core::transforms::prompt::PromptContext {
        app: Some(app_ctx),
        preferences: packet.style,
        preferred_terms: packet.terms,
        user_instruction: None,
        language: profile.language.clone(),
        s1_control: profile.s1_control,
        list_style: state.settings().list_style.resolve(),
    };
    drop(profile);
    let result = teletype_core::transforms::engine::run_transform_blocking(
        provider,
        &transform,
        &input,
        &prompt_ctx,
        &mut None,
    );
    let skip_message = result
        .metrics
        .skip_reason
        .as_ref()
        .map(teletype_core::state::skip_message);
    Ok(TestTransformResult {
        text: result.text,
        transformed: result.transformed,
        skip_message,
    })
}
/// Wispr Flow style selection transform: read the text currently selected in
/// the focused app, run `transform_id` on it, and write the result back over
/// the selection. Used by the per-transform global shortcuts (⌥1 Polish, etc.).
///
/// The whole read → transform → write runs on a blocking thread so the local
/// LLM call (up to ~20 s) never wedges the tokio runtime or the main thread.
/// Returns an error the UI can surface when there is no selection, no model,
/// or the write is refused.
#[tauri::command]
pub async fn transform_selection(
    app: AppHandle,
    state: State<'_, AppState>,
    transform_id: String,
) -> CommandResult<String> {
    // Read the selection on the main thread (AX calls must run there).
    let selected = crate::text_selection::selected_text();
    transform_selection_with_text(app, state, transform_id, selected).await
}
/// Variant of `transform_selection` that accepts a pre-captured selection.
/// Used by the global-shortcut callback, which reads the selection
/// synchronously before the async task runs (so the keystroke hasn't yet
/// corrupted the selection in the focused app). Not a Tauri command — called
/// directly from the shortcut handler.
pub async fn transform_selection_with_text(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    transform_id: String,
    selected: Option<String>,
) -> CommandResult<String> {
    let Some(selected) = selected.filter(|s| !s.trim().is_empty()) else {
        return Err(CommandError::user(
            "No text is selected. Highlight some text first.",
        ));
    };
    let transform = {
        let transforms = state.transforms.get();
        transforms
            .get(&transform_id)
            .ok_or_else(|| "Transform not found".to_string())?
            .clone()
    };
    let handle = tauri::async_runtime::spawn_blocking(move || {
        // Ensure a local model is loaded (no-op if one already is).
        ensure_local_provider(&app);
        let state = app.state::<AppState>();
        let engine = state
            .inference
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or("No AI polish model loaded")?;
        let provider = engine.as_ref();
        let settings = state.settings();
        let profile = (*state.profile.get()).clone();
        let app_ctx = state.platform.active_application().unwrap_or_default();
        let packet = teletype_core::personalization::packet::resolve(&profile, &app_ctx);
        let prompt_ctx = teletype_core::transforms::prompt::PromptContext {
            app: Some(app_ctx),
            preferences: packet.style,
            preferred_terms: packet.terms,
            user_instruction: None,
            language: profile.language.clone(),
            s1_control: profile.s1_control,
            list_style: settings.list_style.resolve(),
        };
        let result = teletype_core::transforms::engine::run_transform_blocking(
            provider,
            &transform,
            &selected,
            &prompt_ctx,
            &mut None,
        );
        Ok::<String, String>(result.text)
    })
    .await
    .map_err(|e| e.to_string())?;
    let polished = handle?;
    // Write the result back over the selection.
    //
    // Fast path: a direct Accessibility write (no clipboard round trip). This
    // works in apps that expose the focused editor as a writable AX role
    // (AXTextField / AXTextArea / AXComboBox) - e.g. Notes, plain text fields.
    //
    // Fallback: Chromium-based apps (Brave, Chrome, Electron, VS Code's web
    // views) do NOT expose a writable AX text element, so the direct write is
    // refused. Route through the clipboard instead: put the polished text on
    // the pasteboard and fire Cmd+V, which replaces the *selection* in those
    // apps - the same primitive the dictation delivery cascade uses. The
    // user's previous clipboard contents are snapshotted first and restored
    // after, so nothing they had copied is lost.
    if !crate::text_selection::insert(&polished) {
        crate::log_entry(
            crate::LogLevel::Info,
            "transform: direct AX write refused; falling back to clipboard paste",
        );
        match paste_replace_over_selection(&polished) {
            true => return Ok(polished),
            false => {
                return Err(CommandError::internal(
                    "Couldn't replace the selection in that app.",
                ));
            }
        }
    }
    Ok(polished)
}

/// Clipboard-paste replace used when the direct Accessibility write is refused
/// (Chromium/Electron apps that don't expose a writable AX text element).
///
/// Sequence: snapshot the general pasteboard -> publish `text` as its current
/// item -> fire Cmd+V (replaces the selection in the focused app) -> restore
/// the snapshot. Runs on a worker thread; enigo and arboard are both safe to
/// drive off the main thread, and the AX read of the selection already
/// happened on the main thread before this runs. Returns `true` when the
/// paste keystroke was sent; the selection is expected to still be live
/// (the user has not clicked away during the LLM round trip).
#[cfg(target_os = "macos")]
fn paste_replace_over_selection(text: &str) -> bool {
    use enigo::{Direction, Enigo, Key, Keyboard, Settings as EnigoSettings};
    use teletype_core::injector::ClipboardGuard;

    let guard = crate::clipboard::MacClipboardGuard::new();
    let snapshot = guard.snapshot();

    // Publish the polished text as the current pasteboard item.
    let set_ok = match arboard::Clipboard::new() {
        Ok(mut clip) => clip.set_text(text.to_string()).is_ok(),
        Err(e) => {
            crate::log_entry(
                crate::LogLevel::Warn,
                format!("transform paste fallback: clipboard unavailable: {e}"),
            );
            false
        }
    };
    if !set_ok {
        crate::log_entry(
            crate::LogLevel::Error,
            "transform paste fallback: could not set the clipboard text",
        );
        return false;
    }

    // Let the pasteboard publish settle, then fire Cmd+V to replace the
    // selection in the focused app.
    std::thread::sleep(std::time::Duration::from_millis(60));
    let paste_sent = match Enigo::new(&EnigoSettings::default()) {
        Ok(mut enigo) => {
            let r = (|| {
                enigo.key(Key::Meta, Direction::Press)?;
                let p = enigo.key(Key::Other(9), Direction::Click);
                enigo.key(Key::Meta, Direction::Release)?;
                p
            })();
            match r {
                Ok(()) => true,
                Err(e) => {
                    crate::log_entry(
                        crate::LogLevel::Error,
                        format!("transform paste fallback: Cmd+V failed: {e}"),
                    );
                    false
                }
            }
        }
        Err(e) => {
            crate::log_entry(
                crate::LogLevel::Error,
                format!("transform paste fallback: enigo unavailable: {e}"),
            );
            false
        }
    };

    // Give the target app a moment to apply the paste before we take the
    // clipboard back, so the paste isn't starved.
    std::thread::sleep(std::time::Duration::from_millis(120));
    if let Some(snap) = snapshot {
        guard.restore(&snap, None);
    }

    if paste_sent {
        crate::log_entry(
            crate::LogLevel::Success,
            format!(
                "transform paste fallback: replaced selection via clipboard ({} chars)",
                text.len()
            ),
        );
    }
    paste_sent
}

/// Non-macOS: there is no clipboard-paste fallback; the AX write is the only
/// path. Kept so the caller compiles on every platform.
#[cfg(not(target_os = "macos"))]
fn paste_replace_over_selection(_text: &str) -> bool {
    false
}

// ---- Command-mode decision provider (P2) ----
/// Loads the decision provider (TinyDecide for classification + Needle 3
/// for command extraction and embedding) if the model files are present.
/// Called at startup and after model downloads. P2.2: no settings gate —
/// intent routing benefits every take (code/question skip polish too), and
/// command takes are armed per-take via TakeKind.
/// P2.7: should a stored decision provider be rebuilt?
///
/// `ensure_decision_provider` is otherwise once-per-session; a model that
/// appears later (needle3 is a catalog download; TinyDecide files can be
/// dropped into `models_dir` at any time) would stay invisible until the
/// next launch. Rebuild only when the stored provider does not already
/// cover everything currently on disk, so repeated calls stay cheap and a
/// complete provider is never reloaded.
#[cfg(test)]
pub(crate) fn decision_upgrade_needed(
    current_model_id: &str,
    tinydecide_available: bool,
    needle_available: bool,
) -> bool {
    (tinydecide_available && !current_model_id.contains("tinydecide"))
        || (needle_available && !current_model_id.contains("needle"))
}
