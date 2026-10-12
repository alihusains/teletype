# tinydecide (Rust)

A Rust runtime for TinyDecide. It reads the same `meta.json` and `model.bin` as the JavaScript engine,
and one encoder pass answers every question about a message.

It is a port of `tinydecide.js`. `tests/conformance.rs` compares it with that engine on both released
builds. Token ids, picks and span text match exactly, and probabilities agree to within 1e-6 (the test
fails above 1e-3). It runs on one CPU core with no GPU, and a request takes about 30 ms on an
Apple-silicon Mac. It also builds for WebAssembly (see below).

## Use it

The crate is not on crates.io. Add it as a path dependency, or as a `git` dependency on a repo that holds this folder.

```toml
[dependencies]
tinydecide = { path = "path/to/TinyDecide/rust" }
```

```rust
use tinydecide::{Question, TinyDecide};

let model = TinyDecide::load("path/to/TinyDecide")?;   // the folder with meta.json and model.bin
let r = model.answer(
    "Book a table for 4 at an Italian place near the station on Friday at 7:30",
    &[
        Question::choice("Which app should handle this?", &["reminders", "music", "calendar", "restaurants", "weather"]),
        Question::noul("The message is urgent."),
        Question::score("How positive is the tone?", &["negative", "neutral", "positive"]),
        Question::span("Extract the time."),
    ],
)?;
println!("{:?} {:?} {:?} {:?}", r.answers[0].pick, r.answers[1].p, r.answers[2].score, r.answers[3].text);
// Some(3) Some(0.219) Some(0.517) Some("7:30")
```

Run the same thing with `cargo run --release --example quickstart -- path/to/TinyDecide`. The default
path is `..`, which is the Hugging Face repo layout.

`TinyDecide::from_bytes(meta_json, bin)` builds a model from bytes you already have in memory.

## Answers

| type | fields that are set |
|---|---|
| `choice` | `probs` (one per option), `pick`, `confidence` (1 minus normalised entropy), `qvec`, `z0` |
| `score` | the same, plus `score` from 0 (first level) to 1 (last level) |
| `noul` | `p`, the probability the statement is true, plus `qvec` and `z0` |
| `span` | `text`, `p_present`, `p_span`, `tok` (first and last state token), `char` |

- `char` holds **UTF-8 byte offsets** into the state string, so `&state[a..b]` is the span before trimming. The JavaScript engine reports UTF-16 units instead.
- `Response` also carries `ids`, `tokens` (state, questions, total), `truncated` and `ms` (always 0 on `wasm32-unknown-unknown`, which has no clock).
- The model reads the first 127 tokens of a message and sets `truncated` when it cuts the rest.
- A question longer than 192 tokens returns `Error::Request("Question 1 is too long (...)")`.
- A choice or score question needs 2 to 32 options, and a noul or span question takes none. Anything else returns `Error::Request`, with the same message as the JavaScript engine.

All types serialise with serde, so `serde_json::to_string(&r)` gives JSON close to the JavaScript result.

## Corrections

Store `(qvec, z0)` under the option a person picked, then pass prototypes on later calls. Nothing retrains the model.

```rust
use tinydecide::corrections::{make_protos, Example};

let q = Question::choice("What kind of note is this?", &["shopping", "task", "event"]);
let mut lists: Vec<Vec<Example>> = vec![vec![]; 3];
for (note, k) in [("buy oat milk", 0), ("call the plumber", 1), ("dentist thursday 4pm", 2)] {
    let a = &model.answer(note, std::slice::from_ref(&q))?.answers[0];
    lists[k].push(Example::from_answer(a));                    // a person picked option k
}
let protos = make_protos(q.kind, &lists, model.beta(), None);   // None: centre on the examples
let r = model.answer_with("pick up eggs", &[q], &[protos])?;
```

For a noul question, list 0 is "false" and list 1 is "true". Without a `center`, the centre is the mean of
the examples, so a single example has no effect. If you track the mean `qvec` of every message asked with
a question, pass it as `center`. The playground does this. `examples/corrections.rs` runs the snippet above.

## Speed and WebAssembly

The `[profile.release]` in this crate's `Cargo.toml` only applies when you build the crate itself.
Set it in your own project too:

```toml
[profile.release]
codegen-units = 1
lto = true
```

The crate builds for `wasm32-unknown-unknown` without wasm-bindgen or any JavaScript glue. Load the
model with `TinyDecide::from_bytes`, since there is no file system. Turn on WebAssembly SIMD, which
makes requests about 2.5 times faster and gives the same results:

```sh
RUSTFLAGS="-C target-feature=+simd128" cargo build --release --target wasm32-unknown-unknown
```

Without SIMD, LLVM does not vectorise the inner dot product on wasm32. Every current browser, Node
and Deno support SIMD.

## Test

```sh
cargo test --release -- --nocapture
```

The test reads the fixture in `../conformance`. Inside the downloaded repo it tests `../model.bin` and
`../full/`. Set `TINYDECIDE_S768` and `TINYDECIDE_FULL` to test other folders.

Dependencies: serde, serde_json, unicode-normalization and unicode-properties. The last two follow
Unicode 17, like Node 26, which made the reference outputs. Apache 2.0.
