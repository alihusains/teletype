//! P8.1-P8.4 desktop delivery adapters — lead pre-scaffold (bw-c23).
//!
//! The worker fills this module with the desktop `DeliveryIo` implementations:
//!
//! - `Typer` via `enigo` (respecting the ≤ 2,000 char / 2 ms-per-char cap),
//! - `ClipboardIo` via `arboard` (faithful snapshot/restore; never restore
//!   something that could not be read),
//! - `paste_key` reusing the existing ⌘V sender from the current paste path.
//!
//! Kept as an empty compiling module so the shared tree stays green while
//! parallel workers hold `dictation.rs`.
