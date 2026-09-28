# Windows testing guide

For whoever is testing the Windows build. You do not need to be a developer.
Everything here is click-and-observe.

## What you are actually testing, and why it matters

The Windows version of Teletype was written but had **never been run by
anyone**. I found and fixed two problems that would have hit you immediately:

1. **The app did not compile for Windows at all.** A stray
   `#[cfg(target_os = "macos")]` was hiding the module that holds every app
   command. Fixed.
2. **The installer shipped a 74-byte text placeholder instead of the real
   polish program.** So anyone who selected a local "Polish" model got the
   error *"llama-server binary not found. Run scripts/build-llama-server.sh"*
   - a bash instruction, on Windows. Fixed, plus a build step that now refuses
   to package if the placeholder is still there.

I cannot cross-compile Windows from a Mac (a C library in the dependency chain
blocks it), so **everything about Windows behaviour is unverified**. You are
the first person to see it work, or not.

## What to install

- Windows 10 (build 19041+) or Windows 11, 64-bit
- [Microsoft C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
  - Tick **Desktop development with C++**
  - Include the **Windows 10/11 SDK**
  - This is only needed to *build* from source. If you get a `.exe` from
    someone, skip this.
- Git, CMake, and Node.js 18+ **only if building from source**

## How to get a build

Either download the installer from the releases page, or build it:

```powershell
git clone https://github.com/alihusains/teletype.git
cd teletype
cd ui; npm install; cd ..
cargo tauri build --bundles nsis
```

The installer lands in `crates/teletype-desktop/target/release/bundle/nsis/`.

If the build fails at `ring`, that is a known environment problem, not a bug in
the app. Try installing the Build Tools and making sure the Developer Command
Prompt is open.

## First launch

- [ ] Launch. Windows may warn about an unsigned app: click **More info** then
      **Run anyway**. That is expected until signing is set up.
- [ ] Turn on **Microphone** access when prompted.
- [ ] Turn on **Accessibility** access. In Windows this is
      Settings > Privacy & security > Accessibility, and you may need to
      restart the app after granting it.
- [ ] Confirm the app window opens and the sidebar navigation works.
- [ ] Confirm the model list loads. On Windows, **Parakeet does not work**
      (it is Apple Silicon only). Use a **Whisper** model, e.g. `small-en` or
      `base`. This is by design, not a bug.

## The core test: does dictation work at all?

- [ ] Open Notepad.
- [ ] Click in the text area.
- [ ] Hold **Ctrl+Shift+Space**, say *"hello this is a test"*, release.
- [ ] Text appears. **This is the single most important check.**

If nothing happens, in order of likelihood:
1. Accessibility permission was not granted, or the app needs a restart after
   you granted it.
2. The hotkey is already taken by another app. Change it in Settings.
3. No speech model is selected. Pick a Whisper model in **Models**.

## Everything else worth trying

| What to do | What you expect |
|---|---|
| Dictate into Notepad, Word, Edge address bar | Text lands in each |
| Dictate a sentence with a number in it | *"it will cost fifty dollars"* becomes `it will cost $50`, **not** `it will cost$50` |
| Say *"she is twenty one"* | `she is 21`, **not** `she is 2001` |
| Type `/` in Notepad | Your AutoText snippets expand |
| Settings > toggle Reduce Motion | The checkbox stays ticked (it unticks itself on macOS; known Mac-only bug, but check Windows too) |
| Open Insights after ~10 dictations | No freeze, and the speed figure looks believable (100-250 wpm) or is honestly absent |
| Close the window, check the tray icon | Present if you enabled it in Settings |
| Quit and relaunch | Settings, dictionary, AutoText and history all still there |

## Known Windows gaps (not your fault, but do confirm)

- **Parakeet / Neural Engine:** Apple Silicon only. Windows uses Whisper.
- **Crash recovery:** the audio-recovery spool is not wired to the capture path
  on the default settings, so "recover last dictation" will not offer anything.
  Known, not fixed.
- **VAD auto-stop** ("stop after a pause"): the session bookkeeping is
  misaligned, so it never fires. Known, not fixed.
- **Personalization screen** is not reachable from the sidebar. Known, not
  fixed.
- The app must be told the **hotkey** explicitly if the default is taken.

## What to send back

For each row, write **PASS** or **FAIL + exactly what you saw**, with the text
quoted. The most valuable things you can send are:

1. Whether dictation inserts text at all.
2. Any number/date that came out mangled, quoted exactly.
3. Whether the app ever got stuck on "Transcribing..." or "Transforming...".
4. The last ~50 lines of the log if anything crashed. The log is in the
   **Developer** tab inside the app (enable it in Settings, then restart).
