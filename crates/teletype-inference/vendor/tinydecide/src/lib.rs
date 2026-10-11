//! TinyDecide: a small typed decision model. You give it one message (the *state*) and any number
//! of questions written in plain language; one encoder pass returns calibrated probabilities for
//! all of them, with no text generation.
//!
//! ```no_run
//! use tinydecide::{Question, TinyDecide};
//! let model = TinyDecide::load("path/to/TinyDecide")?;   // meta.json + model.bin
//! let r = model.answer(
//!     "Book a table for 4 at an Italian place near the station on Friday at 7:30",
//!     &[Question::choice("Which app should handle this?", &["reminders", "calendar", "restaurants"]),
//!       Question::span("Extract the time.")],
//! )?;
//! println!("{:?} {:?}", r.answers[0].pick, r.answers[1].text);
//! # Ok::<(), tinydecide::Error>(())
//! ```
//!
//! This crate is a port of the reference JavaScript engine (`tinydecide.js`) and is checked against
//! it by `tests/conformance.rs`.

pub mod corrections;
mod engine;
mod error;
pub mod tokenizer;
mod weights;

pub use engine::{Answer, Protos, QType, Question, Response, TinyDecide, Tokens};
pub use error::{Error, Result};
pub use weights::{Cfg, Format, Meta};
