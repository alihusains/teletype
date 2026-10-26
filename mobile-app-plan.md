# Teletype mobile app plan

Status: decision document. Version 0.2.7, branch `release/v0.2.7`.
Date: 2026-07-09. Author: engineering (this review). No code was changed.

## 0. Scope and method

This document decides whether Teletype can be published as a mobile app, what the
mobile product should be, and how to get there. It is written to stand alone:
someone who has never seen the desktop app should be able to follow the argument
and the evidence.

Teletype is a Tauri 2 desktop application. The Rust workspace holds the logic:
`teletype-core` is platform independent (AutoText, learning, persistence, the
pipeline), `teletype-inference` runs a local llama-server for LLM polish,
`teletype-speech` loads ASR models, and `teletype-desktop` wires everything to the
OS (mic, global hotkey, system wide typing, clipboard paste). The `ui/` directory
is a React app served in a Tauri webview, plus a floating pill overlay.

Evidence for every claim is drawn from the current tree, cited as a path. Where a
fact depends on Apple or Google platform behaviour that I could not verify from the
repository, it is marked `UNVERIFIED`. Where a number is my estimate, it is marked
ESTIMATE. The constraint set from the task is applied throughout: British plain
English, no em or en dashes, no hype language, and no unverifiable platform claims.

## 1. Verdict

The verdict, stated plainly: Teletype is not a mobile app. It is a desktop
product, and mobile requires a new product, not a port. I recommend building it,
but as a separate app with a deliberately smaller feature set, and I do not
recommend shipping it on a schedule that competes with the desktop roadmap.

The reasoning is in three parts.

First, the desktop app's two defining features are desktop only. Global hotkeys
(`Cmd+Shift+Space` on macOS, `Ctrl+Shift+Space` on Windows, from
`platform/macos_impl.rs:95` and `platform/windows_impl.rs:57`) and system wide
text injection (the typing tap and the AXUIElement paste in
`crates/teletype-desktop/src/`) cannot be replicated on a phone or tablet. You
cannot make a global keyboard hook or a background text injector on mobile the way
the desktop app does. That is a hard platform limit, not an engineering shortfall.

Second, the local first privacy promise is the product's identity. The whole app is
built around running the ASR and the LLM on device, with no cloud (the README and
the brain both state it, and there is no remote backend in the code). On mobile
hardware, the current model weights do not run at acceptable speed. So a mobile
app that keeps the promise and the speed cannot both be true with today's models.

Third, the feature set does not port. Of the 16 screens (the 15 named in the task
plus onboarding), a large share is only meaningful when you can paste into whatever
application has focus. On a phone there is no focus to paste into in the same way.
So most of the value has to be redesigned, not moved.

My recommendation, in one sentence: do not port the desktop app to mobile. Build a
smaller, mobile native app whose core is dictation into the device keyboard, and
defer or drop the desktop only features. Treat the decision as "build the mobile
keyboard dictation product", and treat everything else as a follow on decision that
depends on that one.

The verdict is conditional on two open questions that the owner must answer before
any work starts, in Section 11. If the answer to "do we need mobile at all" is no,
or "do we want to spend a quarter on this", then the rest of this document is a
reference, not a plan.

## 2. What mobile actually is

I need to be explicit about the two mobile form factors, because the whole plan
hinges on choosing one, and they are not the same product.

### 2.1 Form factor A: a system keyboard input method (IME)

An input method, or IME, is a keyboard the user installs and switches to, the same
way you install Gboard on Android or a third party keyboard on iOS. The user taps
the keyboard switcher and selects Teletype. Teletype then owns the keyboard view.
When the user taps a mic button on that keyboard, Teletype records audio, runs ASR,
and types the result into the text field that the keyboard is attached to.

This is the only form factor where a mobile app can approximate the desktop
experience. The desktop user does not think about where the text goes: they press
the hotkey, speak, and the text appears in the focused app. On mobile the keyboard
already has focus, so the text goes into the field by construction. The mic button
on the keyboard is the mobile equivalent of the global hotkey.

The limit of form factor A: the user must switch to the Teletype keyboard to use it.
That is a real friction the desktop app does not have. The desktop hotkey works from
any app without touching the keyboard. On mobile the user must change keyboard first,
which is why many dictation keyboards ship a hardware key or a shortcut mapping.
`UNVERIFIED`: the exact mechanism for a one tap mic shortcut on iOS versus Android
varies by OS version and is not settled here.

### 2.2 Form factor B: a standalone app

A standalone app is a normal app with its own window. The user opens Teletype, taps
record, and the app shows the transcript. To get the text elsewhere the user taps
"copy" and then pastes it into whatever app they were using.

Form factor B is simpler to build and has none of the keyboard integration problems.
It is also the weakest product. It does not give the "speak anywhere and the text
appears" behaviour that defines the desktop app. It is essentially a dictation
recorder with a copy button. It is a reasonable starting point for a prototype, but
it is not the product the desktop users expect.

### 2.3 Which one

I recommend form factor A (the IME) as the target, and form factor B (a standalone
dictation app) as the prototype to prove the model stack first. The reason is
sequencing, not preference. You cannot ship a keyboard before you have a model that
transcribes on a phone in a few seconds, because the keyboard has to feel instant or
people will switch back to the system keyboard. So the honest order is:

1. Build the standalone app (B) to prove that the ASR and the small LLM run on real
   phones at acceptable latency. This also proves the data flow and the privacy
   story on device.
2. Once the model stack is proven on hardware, build the IME (A) around it. The IME
   is the product. The standalone app becomes the "full experience" companion or is
   retired.

This ordering means the first thing we ship to a phone is the least valuable form
factor, but it is the safest way to de risk the model question before spending the
quarter on keyboard integration. That trade is deliberate and I want it visible.

## 3. Feature triage

The task names 15 screens. The code has 16 nav screens plus a developer screen
(`ui/src/App.tsx` NAV_GROUPS_BASE). I triage each one. The categories are:

- Keep: survives on mobile with little or no change.
- Redesign: the idea works but the desktop interaction does not exist on mobile,
  so it must be rebuilt for a phone.
- Drop: does not make sense on mobile. Drop it.

The single biggest driver is focus. The desktop app pastes into whatever has focus.
On a phone there is no cross app focus for a standalone app, and even the IME only
types into the current field. So every feature whose whole point is "insert text
elsewhere" has to be re thought.

### 3.1 Home

Purpose on desktop: overview and quick actions. Triage: Redesign. A mobile home is
just the keyboard or the dictation surface. On a standalone app, home is the record
button. There is no dashboard to port.

### 3.2 Insights

Purpose: shows what the user's dictation and polishing activity means over time.
Triage: Redesign. The data (history, usage stats) already exists and is stored
locally in SQLite (`teletype.db`, from `crates/teletype-core/src/persist.rs`). The
mobile app can read the same store and show the same charts. The redesign is only in
the layout, not the data. Keep the idea, rebuild the view.

### 3.3 Dictation

Purpose: the core record, transcribe, paste flow. Triage: Keep in the standalone
app, and it becomes the entire IME in form factor A. This is the one screen that
must work. Everything else is optional around it.

### 3.4 Scratchpad

Purpose: a scratch space that accumulates dictated lines (`scratchpad.rs`, capped,
one dictation per line, `combined()` gives the whole pad as one block). Triage:
Redesign. On desktop it is a separate window you dictation into. On mobile it is
just a text field inside the app. Keep the concept, but it is a screen not a
window. It is the natural place for "record a few thoughts, then copy them all".

### 3.5 Transforms (AI Polish)

Purpose: run the LLM to clean or restructure the dictated text. Triage: Keep, but
constrained. This is the feature that depends most on the LLM model. It only
survives if the small LLM runs on the phone. See Section 4. If the model does not
run, this screen drops with it and the app is "dictation with no polish", which is
a much thinner product. I am flagging this as the single largest risk to the whole
mobile effort.

### 3.6 AutoText

Purpose: stored triggers and spoken phrases that expand to stored replacement text
(`autotext/mod.rs`, `AutoTextEntry`). Triage: Keep, and it is a strong mobile
feature. On the IME, an AutoText trigger that expands as the user types is exactly
what a mobile keyboard does natively, so it fits. The spoken phrase expansion (the
`snippet` field) also works, because the app already has the transcript. This is one
of the few desktop features that is arguably better on mobile. The import via QR
code is Section 6.

### 3.7 Dictionary

Purpose: brand and acronym words the user has taught the ASR. Triage: Keep. The
dictionary improves transcription and is purely local data. It needs no redesign
beyond a list UI. It also feeds the same QR import path as AutoText, or a
separate one.

### 3.8 Style

Purpose: named style profiles that change how the LLM polishes. Triage: Redesign or
drop. On desktop, style profiles are a real differentiator (the brain notes the
competitor has none). On mobile, if the small LLM runs, the user can still pick a
style. If it does not, style is meaningless. Keep only if Section 4 says the LLM
runs.

### 3.9 Personalization

Purpose: the learning that makes AutoText and polishing better over time
(`learning/`, `personalization/`). Triage: Keep the data, hide the machinery. The
mobile app can use the same on device learning, but the user will not want to tune
it on a phone. Ship it silently. Do not build a tuning screen for a phone.

### 3.10 Settings

Purpose: all the configuration. Triage: Redesign. On mobile, strip it to what the
IM or the app actually uses: selected speech model, language, selected LLM, and the
privacy toggles. Most of the desktop settings (hotkey, tray icon, pill position,
typing tap, clipboard behaviour) do not exist on mobile and should not appear, even
disabled, because they are confusing.

### 3.11 Models

Purpose: choose and download the ASR and LLM models. Triage: Keep, but the catalog
is different. The desktop catalog lists models sized for a laptop or a Mac. The
mobile catalog must list only models that run on the phone. This is a separate
catalog, not the same one with a filter. See Section 4. The download and hash
verification machinery (`catalog.rs`) is reusable.

### 3.12 Command Mode

Purpose: speech to tool call, driven by the NeuDecide ONNX decider. Triage: Drop on
mobile. Command Mode is a desktop feature where a spoken phrase fires a desktop
tool call. On a phone there is no system to fire those calls into in the same way,
and the decider model plus the whole flow is a heavy dependency for a feature the
core mobile product does not need. This is a clean drop.

### 3.13 Keybinds

Purpose: record the global hotkey. Triage: Drop. There are no global hotkeys on
mobile. The IME has a mic button instead. This screen does not exist.

### 3.14 Developer

Purpose: debug and dev tools. Triage: Keep behind the same hidden toggle, or drop.
Not user facing. Keep it if it helps the mobile team debug, drop it if it adds no
value. Low priority either way.

### 3.15 Onboarding

Purpose: first run setup. Triage: Redesign. Mobile onboarding must cover permissions
(microphone, keyboard access, and on iOS the "add keyboard" flow) which the desktop
onboarding does not have. The existing onboarding screen is a starting point, not a
port.

### 3.16 Summary table

| Screen | Triage | Why |
|---|---|---|
| Home | Redesign | Just the record surface on mobile |
| Insights | Redesign | Same local data, new layout |
| Dictation | Keep | The core; becomes the IME |
| Scratchpad | Redesign | A field, not a window |
| Transforms | Keep, constrained | Depends on the LLM running on device |
| AutoText | Keep | Strong mobile fit, QR import |
| Dictionary | Keep | Local data, list UI |
| Style | Keep or drop | Only if the LLM runs |
| Personalization | Keep, hidden | Use it, do not tune it |
| Settings | Redesign | Strip to mobile relevant fields |
| Models | Keep | Different, smaller catalog |
| Command Mode | Drop | Desktop only, heavy dependency |
| Keybinds | Drop | No global hotkeys on mobile |
| Developer | Keep or drop | Not user facing |
| Onboarding | Redesign | New permissions flow |

The honest read of the table: of 16 screens, roughly 6 keep, 6 redesign, 4 drop.
The product on mobile is a dictation and AutoText keyboard with a local polish,
not the desktop app.

## 4. Model feasibility

This is the riskiest section, and I will be plain about what I can and cannot
claim. I have not run any of these models on a phone. I am reading the model sizes
from the two catalogs and reasoning about what fits on phone hardware. Every
latency number in this section is an ESTIMATE, and the store policy question is
open.

### 4.1 What the desktop catalog actually lists

From `crates/teletype-inference/src/catalog.rs`, the LLM rows are:

- s1-mini, 0.6B, 462 MB, recommended, runs on CPU
- speakoflow-mini, 0.8B, 834 MB, recommended, runs on CPU
- qwen3-0.6b, 0.6B, 610 MB, recommended, runs on CPU
- qwen3.5-2b, 2B, 1173 MB, recommended, not CPU only
- fast (Qwen3 1.7B), 1.7B, 1400 MB, recommended, not CPU only
- qwen2.5-3b, 3B, 1900 MB, recommended, not CPU only
- quality (Qwen3 4B), 4B, 2600 MB, recommended, not CPU only
- qwen2.5-7b, 7B, 4900 MB, recommended, not CPU only
- gemma2-2b, 2B, 1500 MB, mixed, not CPU only

The CPU safe set (the `runs_on_cpu` flag) is s1-mini, speakoflow-mini, and
qwen3-0.6b. The CPU cutoff is a parameter count heuristic, not a measured per host
latency (the code says so, and the test `runs_on_cpu_withholds_exactly_the_multi_billion_parameter_rows` enforces that the CPU set stays under 900 MB).

From `crates/teletype-speech/src/catalog.rs`, the ASR rows are:

- parakeet-ctc-110m-coreml, 100 MB, Core ML, Apple Silicon
- base-en, 142 MB, whisper, macOS only
- parakeet-tdt-v3-q4, 356 MB, parakeet, macOS only
- parakeet-ultra-coreml, 600 MB, Core ML
- large-v3-turbo-q5, 547 MB, whisper
- sherpa-parakeet-v3-int8, 465 MB, sherpa-onnx CPU, "Windows, macOS, Linux"

A few things stand out.

First, most of the ASR engines are macOS only in the catalog (whisper.cpp,
parakeet, parakeet.cpp all mark `is_macos()`). The one that is cross platform is
sherpa-onnx, which is a CPU ONNX engine. That is the one with a realistic chance of
running on a phone. The Core ML models are Apple Silicon only and would need to be
re packaged as iOS Core ML, which is a different artifact, not the same file.

Second, the LLMs are GGUF files run through a bundled llama-server
(`crates/teletype-inference/src/server.rs` spawns `llama-server` with
`--host 127.0.0.1`). That is a desktop process model. On mobile you cannot spawn a
child process the same way, and you cannot serve HTTP to localhost the same way.
This is a real architectural break, not a packaging detail.

### 4.2 What runs on a phone

I am going to state what I think is true and mark it clearly, because I cannot
verify it from the repo.

ASR on a phone, ESTIMATE and UNVERIFIED: the 100 to 550 MB ASR models are in the
range that run on modern phones. The 100 MB Core ML CTC model is at the small end.
The 465 MB sherpa-onnx INT8 model is the most portable, because ONNX has an
official mobile runtime (ONNX Runtime Mobile) and sherpa-onnx is built to run on
CPUs and mobile accelerators. `UNVERIFIED`: I have not measured sherpa-onnx on an
iPhone or an Android device, and the exact throughput in words per second on a
specific phone model is unknown. I would expect a 10 to 30 second dictation to
transcribe in a few seconds on a current generation phone, but that is an estimate,
not a measurement.

The Core ML path on iOS is worth calling out separately. iOS apps can use Apple's
Neural Engine, and the Core ML models in the catalog are already Core ML artifacts.
The question is whether the desktop Core ML packages run as iOS Core ML packages or
need conversion. `UNVERIFIED`: I do not know the exact Core ML version and whether
these specific models are compatible with the iOS runtime. This needs a spike.

LLM on a phone, ESTIMATE and UNVERIFIED: this is the hard one. The CPU safe desktop
set is 0.6B to 0.8B models at 462 to 834 MB. On a phone, a 0.6B model at Q4 to Q8
quantization is at the small end of what runs, but inference on a phone CPU or
Apple's Neural Engine is much slower than on a laptop. My estimate is that a 0.6B
model can produce a short polished result in a handful of seconds to tens of
seconds on a current phone, but the tail is long and the experience is not the
instant polish the desktop user has. The 2B and larger models should be treated as
unrealistic for mobile for now. `UNVERIFIED`: no measured tokens per second on
phone hardware exists in this repo.

There is also the process model problem. The desktop runs llama-server as a child
process and talks to it over localhost HTTP (`server.rs`). On mobile, the idiomatic
approach is to run the model in process, either via a Rust mobile binding to
llama.cpp directly, or via a mobile inference framework. That means writing or
adopting a different inference path, not reusing `server.rs`. `UNVERIFIED`: whether
llama.cpp builds cleanly for iOS and Android targets and runs acceptably on the
Neural Engine or the phone NPU is a spike, not an assumption.

### 4.3 Store policy

This is a hard constraint I am flagging, not a number. Apple's App Store has rules
about machine learning models and about what can be downloaded after purchase. I
believe downloading model weights after install is generally permitted, but there
are rules about in app purchases, about what triggers them, and about app size
limits. `UNVERIFIED`: I have not checked the current App Store review guidelines,
and I am not going to claim a specific policy without reading it. The practical
consequence is that the model download flow, the "this adds 600 MB to your device"
message, and the possibility that Apple requires the models to be in the app bundle
or in a paid in app product are all open. This must be resolved with the actual
store guidelines before we design the download UX. Android is less restrictive
here, but Play Store has its own size and permission rules that also need checking.

### 4.4 The honest feasibility summary

ASR is feasible on phone hardware, with the sherpa-onnx INT8 model as the most
portable candidate and the Core ML models as the best iOS native option, both
subject to a spike. LLM is the risk. It is feasible for a small 0.6B model to do a
degraded, slower polish, but the mobile polish will not match the desktop polish in
speed or possibly in the models we can offer. If the owner is not willing to accept
a slower, smaller model polish on mobile, then the Transforms screen drops and the
product is dictation and AutoText only. That is a real product decision, and it
changes what we are building. I am putting it here so it is not discovered in week
eight.

## 5. What the desktop features mean on mobile

I want to be explicit about the two features that define the desktop app, because
the task names them and the whole plan depends on knowing what is lost.

Global hotkey. On desktop, `Cmd+Shift+Space` (macOS) or `Ctrl+Shift+Space`
(Windows) starts dictation from any app (`platform/macos_impl.rs:95`,
`platform/windows_impl.rs:57`), registered via `tauri-plugin-global-shortcut`. On
mobile there is no global hotkey that a third party app can register. The IME
replaces it with a mic button on the keyboard, but only when the Teletype keyboard
is active. Some phones let you map a hardware key or a gesture to start dictation,
but that is OS specific and varies. `UNVERIFIED`: the exact one touch start on iOS
versus Android is not settled here. The net effect is that the "speak anywhere"
behaviour becomes "speak when the Teletype keyboard is open". That is a real loss
and the user has to be told it.

System wide text injection. On desktop the app pastes into the focused application
via the typing tap and the AXUIElement paste (`crates/teletype-desktop/src/`). On a
standalone mobile app there is no cross app paste at all: the app can only put text
on the system clipboard and the user has to paste it. On the IME, the app types into
the active field by construction, which is the mobile equivalent of injection, but
only into the field the keyboard is attached to. So form factor A recovers most of
the injection behaviour, and form factor B recovers none of it. This is the single
strongest argument for building the IME rather than the standalone app as the
real product.

The combination of these two losses is why I call this a new product. A mobile app
that cannot do "speak anywhere" or "paste anywhere" is not the same product, even
if it shares the same ASR, the same AutoText, and the same privacy promise.

## 6. Dictionary and AutoText import via QR code

The desktop app already has a versioned AutoText export. From
`crates/teletype-core/src/autotext/mod.rs`, `export_to_json` writes
`{"version": 1, "entries": [...]}` where each entry is an `AutoTextEntry` with the
fields `id`, `trigger`, `replacement`, `description`, `enabled`, `scope`,
`created_at`, `updated_at`, `snippet`, and `system`. The version constant is
`AUTOTEXT_EXPORT_VERSION = 1`. Import (`import_from_file`, `merge_entries`) updates
existing triggers in place, adds new ones, and skips invalid rows. The dictionary
is separate local data.

The mobile use case for QR code is: the user sets up AutoText and dictionary entries
on the desktop, and wants to get them onto the phone without typing. A QR code is
one way to move that data across the two devices, and it fits the privacy model
because the data goes from one of the user's devices to another, not to a server.

I want to be honest about the size problem. A QR code has a hard capacity limit.
The maximum payload for the most common QR error correction level is on the order
of a few kilobytes of binary, and much less for a readable code that a phone camera
can decode at arm's length. `UNVERIFIED`: the exact byte capacity depends on the
error correction level and the version, and I am not going to state a number I have
not computed. The practical consequence is that a full AutoText library with dozens
of entries will not fit in a single QR code.

So the design has to be one of three things, and I will lay them out rather than
pick silently:

1. A single QR code per entry. Each trigger, replacement, and snippet is its own
   QR code. The user generates many codes and the phone decodes them one at a time.
   This works for a small, curated set (say the ten most used entries) but is
   tedious for a large library.
2. A QR code to a short URL, with a local transfer as the real channel. The QR code
   points to a URL, and the actual data moves over a local network or a local
   server that the desktop runs for a short window. This keeps the QR code small
   (it only carries a pointer) and moves the data over a local connection. It is
   the most robust for large libraries, but it adds a local network dependency and
   a short lived local server, which has its own security questions.
3. A file transfer that is not QR at all. The desktop exports the versioned JSON
   file, the user moves it by any means (AirDrop, a cable, a shared folder), and the
   mobile app imports the same `{"version":1,"entries":[...]}` envelope. This is
   the simplest and most robust, and it reuses the existing import path with no new
   format. The QR code then only carries a hint or a pairing token, not the data.

My recommendation is option 3, with option 1 as a convenience for a small set. The
reason is that the existing export and import already define a stable versioned
format. Reusing it means the phone app can import the exact file the desktop
produces, and the user can keep the two in sync by exporting and importing the same
file. A QR code that tries to carry the whole library is a fragile solution to a
problem that a file already solves. If the owner specifically wants the QR code to
be the channel, then option 2 is the design, but it is more work and I want that
decision made explicitly.

Whatever the channel, the data that moves is the existing `AutoTextEntry` shape, and
the dictionary uses its own existing export. I am not proposing a new data format.
I am proposing a transfer channel for a format that already exists.

## 7. Persistence and privacy

The desktop app stores everything locally. From `crates/teletype-core/src/persist.rs`,
history and usage live in a SQLite database at `teletype.db` in the app data
directory, with a one time migration from the older JSON files. AutoText and the
learning store are local files in the same data directory. There is no cloud, no
remote backend, and no account. This is the product's identity and the mobile app
has to keep it.

On mobile, the equivalent local storage is the app sandbox on iOS and the app
data directory on Android. The same SQLite database and the same JSON files can
live there. `UNVERIFIED`: the exact sandbox paths and the persistence guarantees
across app updates differ between iOS and Android and I am not going to state them
without checking. The design implication is that the `teletype-core` persistence
layer should be portable, and it largely is, because it already writes to a
directory it is given. The mobile app passes its own sandbox directory and the same
code works. That is the single biggest reuse win in this whole plan: the core
persistence, the AutoText store, and the learning store can be shared across
desktop and mobile with little change.

The privacy promise on mobile has to be stated as clearly as on desktop. The models
run on device. The audio does not leave the device. The transcripts do not leave the
device. The only network use is the model download, and that should be the only
outbound connection, to a pinned model host with hash verification (the same
`sha256` check the desktop catalog already uses). I recommend the mobile app ship
with zero outbound network traffic except the model download, and that this be a
testable, assertable property. If we cannot guarantee that, we have broken the
product's identity.

One privacy point that is easy to get wrong on mobile: analytics and crash
reporting. The desktop app should already have a policy on these. On mobile, the
temptation is to add crash reporting or analytics that phone home. I recommend
against it for a v1, to keep the "nothing leaves the device except the model
download" promise clean. If crash reporting is added, it must be off by default and
must not include audio or transcript content.

## 8. Platform and code strategy

The question is what to build with, and what to reuse.

Reuse the Rust core. `teletype-core` is platform independent by design (the brain
and the crate layout say so). The AutoText store, the learning store, the
persistence layer, the pipeline, and the dictionary are all in Rust with no OS
specific code. These should be compiled into the mobile app. The mobile app is a
Rust core with a native shell, not a webview app.

On the inference, the desktop path does not port. `server.rs` spawns a child
llama-server process and talks over localhost. Mobile cannot do that. The mobile
app needs an in process inference path. For ASR, the candidates are a Rust binding
to sherpa-onnx (portable, CPU) or Core ML on iOS (fastest on Apple silicon). For
the LLM, the candidates are a Rust binding to llama.cpp running in process, or a
mobile inference framework. This is new code, and it is the largest new
engineering effort in the plan. I am not estimating a duration here because the
model question in Section 4 has to be answered first.

On the UI, I recommend not reusing the desktop React app for the IME. The IME
keyboard view is a native view on both platforms (iOS `UIView` based keyboard
extension, Android `InputMethodService`). The React app could potentially be reused
for the standalone app's main screen, because a standalone app can host a webview,
but an IME cannot host a full webview for its keyboard view. So the realistic split
is: native keyboard for the IME, and a native or lightweight UI for the standalone
companion. The desktop React app is a reference for the visual language, not a
shared dependency.

I want to be direct about the effort. This is not "build for iOS and Android". It is
"write a mobile inference layer, write a keyboard extension in two native UIs, and
wire the shared Rust core into two app targets". That is a substantial body of new
code, and the desktop team would be building it in parallel with the desktop
roadmap. I am flagging the capacity question honestly.

## 9. Store, legal, and operations

This section is mostly open questions, because several of them depend on facts I
have not verified and on decisions the owner must make. I am listing them so they
are not discovered late.

Apple. An IME on iOS is a keyboard extension, which has its own app sandbox
restrictions. A keyboard extension runs in its own sandbox and cannot freely share
data with the parent app. `UNVERIFIED`: the exact data sharing limits between an
iOS keyboard extension and the parent app are not settled here. This matters because
the AutoText and the learning store live in the parent app, and the keyboard needs
the AutoText. If the keyboard extension cannot read the parent's data directly, we
need a shared app group container, which is the standard iOS solution but needs to
be confirmed. The App Store also has rules about keyboards and about data, and about
what a keyboard extension may do. I am not going to claim the rules without reading
them.

Google. An IME on Android is an `InputMethodService`, which is a documented Android
component. Android is generally less restrictive about a keyboard reading the app's
own data, because Android apps share a uid. `UNVERIFIED`: I have not checked the
current Play Store review guidance for a dictation keyboard. It needs checking.

Both stores. The model download is the main legal and store question. Downloading
several hundred megabytes of model weights after install, the size limits, and the
rule that the app cannot be blocked on a download, all need to be checked against
the current store guidelines. I am listing this as a task, not a fact.

Privacy labels. Both stores now require privacy labels that declare what data is
collected. For a local first app the answer is "nothing", which is a good answer
but still has to be filled in correctly, including the model download as a data
flow. This is paperwork, not a blocker, but it has to be right.

Signing and release. The desktop app already has a signing and notarization
pipeline (the brain lists a deployment engineer for it). Mobile adds iOS App Store
signing and Android Play signing, which are different systems. This is operations
work, not a design decision, but it is real work and it belongs in the milestone
plan.

## 10. Milestones

I am presenting milestones as a sequence with an explicit go no go at each gate,
because the model feasibility in Section 4 means the whole thing can stop at the
first gate and that is a good outcome, not a failure.

Milestone 0, decide. Answer the two open questions in Section 11. If the answer is
"yes, build the mobile keyboard dictation product, and accept a slower smaller LLM
or drop the LLM", proceed. If not, stop and this document is a reference. Go no go:
owner decision.

Milestone 1, prove the model stack. A spike on real hardware. Build a throw away
standalone app that runs the sherpa-onnx ASR model and the 0.6B LLM on a test
iPhone and a test Android phone, and measure the actual transcription latency and
polish latency. The goal is a number, not an app. Go no go: does ASR transcribe a
15 second dictation in under a few seconds, and does the 0.6B LLM produce a usable
polish in a tolerable time. If ASR is fine but the LLM is too slow, the decision is
whether to ship without the LLM polish.

Milestone 2, the standalone app. Build form factor B, the standalone dictation app,
with the shared Rust core, the local persistence, the AutoText and dictionary, and
the model download. This is the proof that the core re works on a phone and the
privacy story holds. Go no go: does the standalone app dictation, polish or not, and
let the user copy the result, with everything on device.

Milestone 3, the keyboard. Build form factor A, the IME, around the proven model
stack. On iOS the keyboard extension, on Android the `InputMethodService`. Wire in
the AutoText typing expansion and the spoken phrase expansion. Go no go: does the
user switch to the Teletype keyboard, tap the mic, speak, and have the text appear
in the field, at an acceptable speed.

Milestone 4, the data bridge. Ship the dictionary and AutoText import, using the
existing versioned export format, over the channel chosen in Section 6. Go no go:
does a desktop AutoText library import into the phone correctly.

Milestone 5, store readiness. Privacy labels, model download compliance, signing,
and the store submission for one platform first (I recommend Android first, because
the keyboard rules are less restrictive, then iOS). Go no go: store approval.

Each milestone is a real unit of work with a real gate, and the gates are ordered so
the riskiest question (the model) is answered before the most expensive work
(the keyboard) starts.

## 11. Open questions

These are the things I could not answer from the repository, and the things the
owner must answer. I am listing them in the order they block.

1. Do we need mobile at all? The owner may decide the desktop app is enough. This
   is the first question and it gates everything.
2. If we build mobile, do we accept a slower, smaller LLM polish, or do we ship
   without the LLM polish? This decides whether Transforms is a mobile feature.
3. Do we build the IME, or is a standalone app enough for v1? This decides the
   product. I recommend the IME, but the owner may want the cheaper standalone app
   first.
4. What is the transfer channel for the dictionary and AutoText? A file, a per entry
   QR, or a QR to a local pointer? I recommend the existing file format with QR as a
   convenience.
5. What is the model download policy on each store? This needs the actual store
   guidelines, which I have not read.
6. Can the iOS keyboard extension read the parent app's data, and do we need a shared
   app group? This needs an iOS spike.
7. Which phones are the target? The model feasibility depends on the hardware. I am
   assuming current generation phones, but the owner should set a floor (for example
   the last three generations) so the model sizes are chosen against a real device
   class.
8. What is the capacity plan? This is new product work in parallel with the desktop
   roadmap. Who builds the mobile inference layer and the keyboard, and when.

I am deliberately not answering 1, 2, or 3. They are the owner's. Everything else in
this document is my recommendation, and it changes based on the answers.

## 12. What I would do next

If the owner says go, the next concrete action is Milestone 1, the model spike,
because it is the cheapest way to answer the riskiest question. The spike needs two
test devices, the sherpa-onnx INT8 model, the 0.6B LLM in GGUF, and a day or two to
build a throw away harness and run it. The output is a table of measured latencies
on real hardware, and that table decides Milestone 2 and everything after it. Until
that table exists, every latency number in this document is an estimate, and the
owner should treat it that way.

I have not changed any code. I have not touched git. The only new file is this
document.
