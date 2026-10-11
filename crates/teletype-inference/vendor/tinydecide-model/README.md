---
license: apache-2.0
language:
- en
base_model: google/electra-small-discriminator
pipeline_tag: zero-shot-classification
tags:
- tiny
- embedded
- esp32
- microcontroller
- decision-model
- calibration
- zero-shot
- information-extraction
- 4-bit
- javascript
- python
- rust
- arduino
---

<p align="center">
  <img src="https://huggingface.co/TheREZOR/TinyDecide/resolve/main/assets/hero.svg" width="100%" alt="TinyDecide answers four questions about one message: which app, is it urgent, tone, and the time">
</p>

<p align="center"><b>The smallest Jev-style decision model.</b><br>It runs anywhere and punches far above its size.</p>

<p align="center">
  <a href="https://huggingface.co/spaces/TheREZOR/TinyDecide-Playground"><b>Try it in your browser</b></a> ·
  <a href="#watch"><b>Watch</b></a> ·
  <a href="#quick-start"><b>Quick start</b></a> ·
  <a href="#python-rust-and-esp32"><b>Python, Rust, ESP32</b></a> ·
  <a href="#benchmarks"><b>Benchmarks</b></a>
</p>

TinyDecide reads one message and answers any number of questions about it. You write the questions in plain English when you call it. It returns a probability for every answer and does not generate text, so one encoder pass answers every question at once.

The model fits the 8 MB flash of an ESP32-S3 without PSRAM, such as the M5Stack Cardputer. This repo has engines for JavaScript in the browser and Node, Python, Rust and the ESP32-S3.

## Watch

<video controls playsinline preload="metadata" width="100%" poster="https://huggingface.co/TheREZOR/TinyDecide/resolve/main/assets/promo-poster.jpg" src="https://huggingface.co/TheREZOR/TinyDecide/resolve/main/assets/promo.mp4"></video>

A 48-second promo with sound. Every answer in it comes from the 4-bit build in this repo. Its Decision Index numbers are self-scored, as [Benchmarks](#benchmarks) explains. If the player does not load, [download the video](https://huggingface.co/TheREZOR/TinyDecide/resolve/main/assets/promo.mp4).

<p align="center">
  <img src="https://huggingface.co/TheREZOR/TinyDecide/resolve/main/assets/types.svg" width="100%" alt="Four answer types: pick one, yes or no, score, extract">
</p>

## Use cases

- Send a voice or chat command to the right app or skill.
- Sort an inbox by spam, urgency and kind of message.
- Pull a time, an amount, a name or a place out of a message.
- Rate tone, priority or sentiment on a scale you define.
- Run offline, with no server and no API key.

## Benchmarks

<p align="center">
  <img src="https://huggingface.co/TheREZOR/TinyDecide/resolve/main/assets/benchmark.svg" width="100%" alt="Decision Index: TinyDecide 5.77 (10.4M, this download) and 6.31 (13.8M, full precision) against eight larger models scoring 1.38 to 5.07">
</p>

<p align="center">
  <img src="https://huggingface.co/TheREZOR/TinyDecide/resolve/main/assets/efficiency.svg" width="100%" alt="Decision Index per million parameters: TinyDecide 0.55, best other board entry 0.052">
</p>

The [Decision Index](https://huggingface.co/spaces/multimodalart/jev-decision-index) scores typed decision models on 38 public benchmarks. Against the 2026-09-28 board, the 10.4M build would rank first among entries under 100M parameters. It is self-scored and not a listed entry. The board's leaders are 26B to 31B models that score around 57, so this is a result for the small class.

## Try it

Open the [playground](https://huggingface.co/spaces/TheREZOR/TinyDecide-Playground). It runs this exact model in your browser, so your messages stay on your machine. Type a message, edit the questions, and watch every answer update.

## Quick start

You need Node 22 or newer. The engine is one JavaScript file with no dependencies.

```sh
hf download TheREZOR/TinyDecide --local-dir tinydecide --exclude "assets/*"
cd tinydecide
node example.mjs
```

```js
import { readFileSync } from "node:fs";
import { TinyDecide } from "./tinydecide.js";

const meta = JSON.parse(readFileSync("meta.json", "utf8"));
const bin = readFileSync("model.bin");
const model = new TinyDecide(meta, bin.buffer.slice(bin.byteOffset, bin.byteOffset + bin.byteLength));

const { answers } = model.answer({
  state: "Book a table for 4 at an Italian place near the station on Friday at 7:30",
  questions: [
    { type: "choice", text: "Which app should handle this?", options: ["reminders", "music", "calendar", "restaurants", "weather"] },
    { type: "noul", text: "The message is urgent." },
    { type: "score", text: "How positive is the tone?", options: ["negative", "neutral", "positive"] },
    { type: "span", text: "Extract the time." },
  ],
});
```

| question | answer |
|---|---|
| Which app should handle this? | `restaurants`, p = 0.97 |
| The message is urgent. | p = 0.22 |
| How positive is the tone? | score = 0.52 |
| Extract the time. | `7:30` |

| type | you give | you get back |
|---|---|---|
| `choice` | a question and 2 to 32 options | `probs` per option, `pick`, `confidence` |
| `noul` | a yes or no statement | `p`, the probability it is true |
| `score` | a question and levels, lowest first | `probs`, `score` from 0 to 1 |
| `span` | "Extract the ..." | `text`, `p_present` |

In a browser, `load()` from `load.js` fetches the two files from this repo, or from any URL you pass, and returns the model.

```js
import { load } from "./load.js";
const model = await load();   // https://huggingface.co/TheREZOR/TinyDecide/resolve/main/
```

## Python, Rust and ESP32

Four engines run the same model files. `conformance/` holds the JavaScript engine's outputs for 288 tokenizer strings in many scripts and for 84 requests. The requests cover all four question types, several questions per request, corrections and bad input. Each engine's test compares its own outputs with these.

| engine | folder | needs | how close it is to the JavaScript engine |
|---|---|---|---|
| JavaScript | the repo root | Node 22 or newer, or a browser | It is the reference. It matches PyTorch to 3e-6. |
| Python | `python/` | numpy | Same token ids, picks and spans. Probabilities within 1e-6. |
| Rust | `rust/` | serde, serde_json, unicode-normalization, unicode-properties | Same token ids, picks and spans. Probabilities within 1e-6. |
| ESP32-S3 | `esp32/` | ESP-IDF 5.5 or Arduino core 3 | Same token ids, picks and spans. Probabilities within about 0.02, because the device rounds activations to 8 bits. |

**Python.** Install it with `pip install ./python` from the downloaded repo.

```python
from tinydecide import TinyDecide

model = TinyDecide.load(".")            # or TinyDecide.from_pretrained() with huggingface_hub
r = model.answer("Book a table for 4 at an Italian place near the station on Friday at 7:30", [
    {"type": "choice", "text": "Which app should handle this?", "options": ["reminders", "music", "calendar", "restaurants", "weather"]},
    {"type": "span", "text": "Extract the time."},
])
print(r["answers"][0]["pick"], r["answers"][1]["text"])   # 3 7:30
```

**Rust.** Add it as a path dependency, `tinydecide = { path = "rust" }`.

```rust
use tinydecide::{Question, TinyDecide};

let model = TinyDecide::load(".")?;      // the folder with meta.json and model.bin
let r = model.answer(
    "Book a table for 4 at an Italian place near the station on Friday at 7:30",
    &[
        Question::choice("Which app should handle this?", &["reminders", "music", "calendar", "restaurants", "weather"]),
        Question::span("Extract the time."),
    ],
)?;
println!("{:?} {:?}", r.answers[0].pick, r.answers[1].text);   // Some(3) Some("7:30")
```

**ESP32.** `esp32/tinydecide` is an ESP-IDF component and an Arduino library.

```cpp
#include "tinydecide.h"

td::initEmbedded();                       // ESP-IDF embeds model.bin in the app; Arduino uses td::initPartition()
const char* apps[] = {"reminders", "music", "calendar", "restaurants", "weather"};
const td::Question qs[] = {
    {td::CHOICE, "Which app should handle this?", apps, 5},
    {td::SPAN, "Extract the time."},
};
static td::Answer a[2];
const char* msg = "Book a table for 4 at an Italian place near the station on Friday at 7:30";
td::answer(msg, qs, 2, a);                // a[0].pick == 3, msg[a[1].start, a[1].end) == "7:30"
```

On the ESP32-S3, the PIE vector unit runs the 4-bit matmuls on both cores. On an M5Stack Cardputer the quick-start request takes 2.5 s. It has four questions and 67 tokens, and the answers match the table above. That timing comes from the ESP-IDF project in `esp32/examples/basic`. The Arduino sketch in `esp32/tinydecide/examples/Basic` builds, but it has not run on a device yet. Each folder's README has the details.

## Files

| file | what it is |
|---|---|
| `model.bin`, `meta.json` | The 10.4M build, with 4-bit matrices and embeddings and a 16,000-token vocabulary. 6.2 MB. |
| `full/model.bin`, `full/meta.json` | The 13.8M build in the same 4-bit format, with the full 30,534-token vocabulary. 10 MB. Its 6.31 comes from the full-precision weights. This 4-bit file has no board score. |
| `tinydecide.js` | The engine. Plain JavaScript with no dependencies. |
| `load.js`, `corrections.js`, `*.d.ts` | Loading from a folder or URL, building corrections, and TypeScript types. |
| `example.mjs` | The quick start above. |
| `verify.mjs`, `ref.json` | Checks the engine against PyTorch outputs. The largest difference is 3e-6. |
| `python/`, `rust/`, `esp32/` | The other engines. Each has a README, an example and a conformance test. |
| `conformance/` | The reference inputs and outputs that every engine's test uses, and the script that makes them. |
| `assets/promo.mp4` | The promo video. |

<details>
<summary><b>How it was built</b></summary>

- The backbone is [ELECTRA-small](https://huggingface.co/google/electra-small-discriminator), with 12 layers and width 256. This build cuts the feed-forward blocks from 1,024 to 768 units and keeps the 16,000 most used tokens of the vocabulary. It keeps every single character, so any word still splits into known pieces.
- Each question attends to the message but not to the other questions. Asking a question alone or with ten others gives the same answer.
- The training data has public labelled tasks without any benchmark train splits, about 62,000 real messages that Qwen3.6-35B-A3B labelled, and programmatic checks for numbers, times and contact details.
- The 10.4M build learned to imitate an ensemble of two full-size models.
- The losses are proper scoring rules. A held-out split sets one temperature per answer type.

</details>

<details>
<summary><b>Held-out results (10.4M build)</b></summary>

| tier | accuracy |
|---|---|
| known question types, new messages | 82.8 |
| new wording of known types | 82.2 |
| new question kinds about real messages (agreement with the teacher) | 86.3 |
| public slot types never trained on | 86.3 |
| numbers, times, counts and contact details | 92.4 |
| messages that try to steer the answer | 82.0 |
| public tasks never trained on (10 families) | 65.2 |

Calibration error (ECE) is 0.02 to 0.15 on the tiers with true labels.

</details>

<details>
<summary><b>Limits</b></summary>

- English only.
- Every engine reads the first 127 tokens of a message and sets `truncated` when it cuts.
- It answers best on short, everyday messages. Questions that need two-step reasoning, such as natural language inference, are close to chance.
- On a Cardputer the quick-start request takes 2.5 s on both cores. [Pocket Notes](https://github.com/therezor/pocket-notes) asks one choice question about a short note and takes about 1.8 s. A request needs about 2.2 KB of heap per token. The bare ESP32 example has 371 KB free, enough for about 150 tokens. An app with a display and Wi-Fi has less.
- The Decision Index numbers are self-scored with the kit. The board's maintainers have not verified them.
- The 13.8M build's 6.31 is not a clean out-of-sample number. It had two board runs, and one data change came after the first run. The 10.4M build's 5.77 came from a single run.

</details>

Made by [REZOR](https://huggingface.co/TheREZOR). Apache 2.0.
