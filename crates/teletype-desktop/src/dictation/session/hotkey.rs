use crate::dictation::*;

impl Session {
    pub(in crate::dictation) fn hotkey_down_event(&mut self) {
        // P0.2: the press is the first stage boundary of a take.
        self.ensure_trace();
        // A press means any release waiting on the bounce watchdog was the
        // spurious up edge of a bounce, not the user letting go.
        self.bounce_settle_seq = self.bounce_settle_seq.wrapping_add(1);
        let mode = self.recording_mode();
        // Bounce guard: a `HotkeyDown` this close to a release is a spurious
        // re-edge from the Fn key (down-up-down on one press), not a new take.
        // The hands-free double-tap below is longer (350ms) and phase-gated,
        // so it still works; this only suppresses sub-100ms re-presses while
        // idle, which is always a bounce.
        if self.phase == Phase::Idle
            && self
                .last_release
                .is_some_and(|t| t.elapsed() <= HOTKEY_BOUNCE_DEBOUNCE)
        {
            return;
        }
        // Double-tap detection (Hold mode): a press shortly after a release
        // while still recording switches to hands-free instead of stopping.
        if self.phase == Phase::Listening && mode == RecordingMode::Hold && !self.hands_free {
            if let Some(t) = self.last_release {
                if t.elapsed() <= DOUBLE_TAP_WINDOW {
                    self.hands_free = true;
                    self.last_release = None;
                    return;
                }
            }
        }
        self.input_with_kind(Input::HotkeyDown, TakeKind::Dictation);
    }

    pub(in crate::dictation) fn hotkey_up_event(&mut self) {
        self.ensure_trace();
        let measured = take_hotkey_hold_ms();
        self.hotkey_up_event_measured(measured);
    }

    /// The release path with the hold measurement supplied. Production passes
    /// the event-tap value (`None` when the edge did not come from the tap);
    /// tests pass it directly so a simulated bounce never has to plant the
    /// process-wide atomic.
    pub(in crate::dictation) fn hotkey_up_event_measured(&mut self, measured: Option<u64>) {
        let held_ms = measured
            .unwrap_or_else(|| teletype_core::storage::now_ms().saturating_sub(self.started_at_ms));
        // A release measured inside the bounce window is the Fn key's
        // spurious up edge, not the user letting go. It must never tap-paste:
        // that throws the take away and pastes a foreign clipboard. It must
        // not stop the take either, because the physical key usually sends
        // another down within milliseconds. Arm a watchdog instead: if no
        // press follows, the edge was a genuine short release and the take
        // ends normally.
        if measured.is_some_and(|ms| ms < HOTKEY_BOUNCE_DEBOUNCE.as_millis() as u64) {
            crate::log_entry(
                crate::LogLevel::Info,
                "hotkey: ignored a release inside the bounce window",
            );
            self.defer_bounced_release(held_ms);
            return;
        }
        self.finish_hotkey_release(held_ms);
    }

    /// The shared release verdict. A quick release is an ordinary stop: the
    /// take stops and is transcribed, and it is discarded silently (no
    /// note, no paste, clipboard untouched) only when its visible recording
    /// is under [`MIN_TAKE_VISIBLE_MS`] (a mistap). Every other release
    /// stops and transcribes the take.
    pub(in crate::dictation) fn finish_hotkey_release(&mut self, held_ms: u64) {
        if self.quick_tap_due(held_ms) {
            let visible_ms = teletype_core::storage::now_ms().saturating_sub(self.started_at_ms);
            if visible_ms < MIN_TAKE_VISIBLE_MS {
                crate::log_entry(
                    crate::LogLevel::Info,
                    "quick tap: take under the visible floor, discarded silently",
                );
                self.tap_discard();
            } else {
                crate::log_entry(crate::LogLevel::Info, "quick tap: ordinary stop");
                self.input(Input::HotkeyUp);
            }
        } else {
            self.input(Input::HotkeyUp);
        }
        self.last_release = Some(Instant::now());
    }

    /// Arms the bounce watchdog for a release that arrived inside
    /// [`HOTKEY_BOUNCE_DEBOUNCE`]. The sequence is bumped again by any press,
    /// so a bounce's follow-up down cancels the timer and the take keeps
    /// recording.
    pub(in crate::dictation) fn defer_bounced_release(&mut self, held_ms: u64) {
        // Do NOT set `last_release` here: the release was spurious, and a
        // fresh stamp would make the follow-up down look like a double-tap
        // (release-then-press within DOUBLE_TAP_WINDOW) and flip the take to
        // hands-free. The real release (via `finish_hotkey_release`) stamps
        // `last_release` when it actually ends the take.
        self.bounce_settle_seq = self.bounce_settle_seq.wrapping_add(1);
        let seq = self.bounce_settle_seq;
        let Some(tx) = self.controller.tx.clone() else {
            // Test controller: no event loop to receive the settle. Tests
            // drive `hotkey_bounce_settled` directly.
            return;
        };
        let spawned = thread::Builder::new()
            .name("teletype-hotkey-bounce".into())
            .spawn(move || {
                thread::sleep(HOTKEY_BOUNCE_DEBOUNCE + Duration::from_millis(20));
                let _ = tx.send(Event::HotkeyBounceSettled { seq, held_ms });
            })
            .is_ok();
        if !spawned {
            // The watchdog could not be armed. End the take now rather than
            // risk leaving the microphone open; a short take is recoverable.
            self.finish_hotkey_release(held_ms);
        }
    }

    /// The bounce watchdog fired. A press since arming bumped the sequence,
    /// which means the take is still held and the timer must do nothing.
    pub(in crate::dictation) fn hotkey_bounce_settled(&mut self, seq: u64, held_ms: u64) {
        if seq != self.bounce_settle_seq || self.phase != Phase::Listening {
            return;
        }
        self.finish_hotkey_release(held_ms);
    }

    /// The release is a quick tap. The hold is the one measured at the
    /// event source; the enqueue-time estimate (`started_at_ms`, stamped
    /// when the controller dequeues the press) is only a fallback and runs
    /// long whenever the controller was busy.
    pub(in crate::dictation) fn quick_tap_due(&self, held_ms: u64) -> bool {
        is_quick_tap(
            self.recording_mode(),
            self.phase,
            self.hands_free,
            self.take_kind,
            held_ms,
        )
    }

    /// A quick tap whose take is under the visible-recording floor: throw
    /// the take away exactly as Escape would (mic, audio duck, recovery
    /// spool and pill all reset via the shared cancel path) but with no
    /// note at all. No keystroke, the clipboard is not touched, and nothing
    /// is written to `last_text`.
    pub(in crate::dictation) fn tap_discard(&mut self) {
        self.hotkey_down = false;
        self.hands_free = false;
        // The shared cancel path does the reset (mic, duck, spool, trace,
        // idle-unload timer) and, with an empty note, no pill text at all.
        // The headless session reaches it too: the platform-touching calls
        // inside guard on `headless_flash`, so the test runs the real path.
        self.stop_with_note(true, "");
    }

    /// P2.4/P2.6: the dedicated Command hotkey was pressed. Mirrors
    /// `hotkey_down_event` but tags the take as a command, and captures
    /// the focused app's selection (if any) for selection-aware commands:
    /// the keystroke that follows would corrupt the selection, so it is
    /// read synchronously here, on press.
    pub(in crate::dictation) fn command_hotkey_down_event(&mut self) {
        self.ensure_trace();
        self.pending_selection =
            crate::text_selection::selected_text().filter(|s| !s.trim().is_empty());
        let mode = self.recording_mode();
        // Same bounce guard as `hotkey_down_event`: a command press within
        // the bounce window of a release is a spurious Fn re-edge, not a new
        // command take. Suppressed while idle.
        if self.phase == Phase::Idle
            && self
                .last_release
                .is_some_and(|t| t.elapsed() <= HOTKEY_BOUNCE_DEBOUNCE)
        {
            return;
        }
        if self.phase == Phase::Listening && mode == RecordingMode::Hold && !self.hands_free {
            if let Some(t) = self.last_release {
                if t.elapsed() <= DOUBLE_TAP_WINDOW {
                    self.hands_free = true;
                    self.last_release = None;
                    return;
                }
            }
        }
        // CMD-2: a bare-modifier dictation hotkey makes the command chord
        // unreachable. The Control of the chord arms the dictation tap
        // first, so by the time the chord fires the take is already Listening
        // and `decide` has no `Start` to give: the command press is swallowed
        // and the take delivers the words as dictated text. The chord wins.
        // The recording is live, so re-tagging the take keeps everything the
        // user has said since the press, and the release that follows stops it
        // through the same hotkey latch. Two details carry the fix:
        //   - the added modifier is read by the tap as a release inside the
        //     bounce window, which armed the settle watchdog; this press is
        //     that follow-up press, so bump the sequence or the watchdog ends
        //     the command take 120 ms after the chord.
        //   - only a take younger than COMMAND_TAKEOVER_WINDOW is re-routed.
        //     Past the window the buffer holds a dictation the user is
        //     deliberately giving, and stealing it would silently throw it
        //     away.
        if self.phase == Phase::Listening
            && self.take_kind == TakeKind::Dictation
            && self.started_at_ms != 0
            && teletype_core::storage::now_ms().saturating_sub(self.started_at_ms)
                <= COMMAND_TAKEOVER_WINDOW.as_millis() as u64
        {
            self.bounce_settle_seq = self.bounce_settle_seq.wrapping_add(1);
            self.take_kind = TakeKind::Command;
            if let Some(key) = self.trace_key {
                // P2.9: the recording began at the dictation press, so this
                // adds `armed` (the command route starts here) without a
                // second `started`.
                self.trace_take_kind(key, TakeKind::Command);
                self.trace_command_event(key, "armed(chord takeover)");
            }
            crate::log_entry(
                crate::LogLevel::Info,
                "command chord took over the in-flight dictation take",
            );
            return;
        }
        self.input_with_kind(Input::HotkeyDown, TakeKind::Command);
    }

    /// P2.8: the tray's "Run Voice Command" item. Starts a Command take
    /// from Idle, stops the running take otherwise, following the same
    /// toggle semantics as the dictation tray item. The kind is only
    /// applied when the state machine starts a take, so using it as a stop
    /// never re-tags an in-flight dictation take.
    pub(in crate::dictation) fn command_toggle_event(&mut self) {
        self.ensure_trace();
        self.input_with_kind(Input::Toggle, TakeKind::Command);
    }

    /// P8.4: the Paste-Last hotkey was pressed. Re-delivers the last final
    /// text through the same delivery path as a normal paste. If there is
    /// no stored text, flash "Nothing to paste".
    pub(in crate::dictation) fn paste_last_event(&mut self) {
        #[cfg(test)]
        if self.headless_flash {
            return;
        }
        let Some(state) = self.state() else {
            return;
        };
        let text = state
            .last_text
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let Some(text) = text else {
            self.flash_note("Nothing to paste");
            return;
        };
        // Run the same delivery path as a normal paste: build the IO bundle
        // and call deliver(). The text is already final (no smart insertion,
        // no polish) — it is exactly what was last delivered.
        let platform: &dyn teletype_core::platform::Platform = state.platform.as_ref();
        let app_id = platform
            .active_application()
            .map(|a| a.application_id)
            .filter(|id| !id.is_empty());
        // macOS: real clipboard guard; other platforms: compile-safe stub
        // via `delivery_io::macos_io`'s non-mac twin (`Arc<()>`).
        // Item 3: `keep_text_on_clipboard` is read here, per delivery, so a
        // settings change applies to the next paste without a restart.
        let keep_text = state.settings().keep_text_on_clipboard;
        #[cfg(target_os = "macos")]
        let io = crate::delivery_io::macos_io(
            std::sync::Arc::new(crate::clipboard::MacClipboardGuard::new()),
            keep_text,
        );
        #[cfg(target_os = "windows")]
        let io = crate::delivery_io::windows_io(keep_text);
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let io = crate::delivery_io::macos_io(std::sync::Arc::new(()), keep_text);
        let policy = DeliveryPolicy::builtin();
        let outcome = delivery::deliver(&text, app_id.as_deref(), &policy, &io);
        // F-16 / F-17 / F-19: the paste-shortcut label in the user-facing
        // message. macOS uses Cmd+V; Windows uses Ctrl+V.
        #[cfg(target_os = "macos")]
        let paste_shortcut_label = "Cmd+V";
        #[cfg(not(target_os = "macos"))]
        let paste_shortcut_label = "Ctrl+V";
        match outcome {
            DeliveryOutcome::PastedVerified
            | DeliveryOutcome::PastedUnverified
            | DeliveryOutcome::Typed
            | DeliveryOutcome::SetValue => {
                crate::log_entry(
                    crate::LogLevel::Info,
                    format!("paste-last: delivered ({outcome:?})"),
                );
            }
            DeliveryOutcome::HeldForPasteLast => {
                crate::log_entry(crate::LogLevel::Warn, "paste-last: held (delivery failed)");
                self.flash("Paste failed. Text kept for later");
            }
            DeliveryOutcome::Refused { reason } => {
                crate::log_entry(
                    crate::LogLevel::Warn,
                    format!("paste-last refused: {reason}"),
                );
                self.flash(&format!("Not pasted: {reason}"));
            }
            DeliveryOutcome::ClipboardOnly => {
                crate::log_entry(crate::LogLevel::Info, "paste-last: clipboard only");
                self.flash(&format!("Copied. Press {paste_shortcut_label} to paste."));
            }
            DeliveryOutcome::AccessibilityDenied => {
                crate::log_entry(
                    crate::LogLevel::Warn,
                    "paste-last: accessibility not trusted",
                );
                self.flash(&format!(
                    "Auto-paste needs Accessibility. Text copied: press {paste_shortcut_label}."
                ));
            }
        }
    }

    /// Headless release for P2.1 tests: the take is mid-recording (Listening)
    /// and the mic was never opened, so `stop()` has no take to finish.
    /// The state machine still decides `Stop`, and the session transitions
    /// to the post-release phase (Transforming) exactly as a real release
    /// would, without touching the recording slot.
    ///
    /// Unlike `hotkey_up_event`, this does NOT call `decide` (which would
    /// clear the `hotkey_down` latch that the test already asserted). It
    /// directly applies the `Stop` decision the state machine would produce.
    #[cfg(test)]
    pub(in crate::dictation) fn headless_release(&mut self, kind: TakeKind) {
        self.ensure_trace();
        // The state machine decides `Stop` (asserted by the test); apply
        // the decision without re-deriving it. Mirror `hotkey_up_event` by
        // clearing the hotkey latch, so the next take starts clean —
        // a real release always clears it through `decide`.
        self.hotkey_down = false;
        if self.phase == Phase::Listening {
            self.phase = Phase::Transforming;
            self.take_kind = kind;
        }
        self.last_release = Some(Instant::now());
    }

    /// The kind of the in-flight take (P2.2). Test-only accessor;
    /// production code reads the field directly.
    #[cfg(test)]
    pub(in crate::dictation) fn take_kind(&self) -> TakeKind {
        self.take_kind
    }

    pub(in crate::dictation) fn recording_mode(&self) -> RecordingMode {
        #[cfg(test)]
        if self.headless_flash {
            return RecordingMode::Hold;
        }
        let Some(settings) = self.state().map(|s| s.settings()) else {
            return RecordingMode::Hold;
        };
        settings.recording_mode.resolve()
    }
}
