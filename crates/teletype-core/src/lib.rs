//! Teletype core: platform-independent business logic.
//!
//! This crate must stay free of OS-specific code and of the UI framework.
//! Platform capabilities (active app, permissions, paste shortcut) are reached
//! through [`platform::Platform`]; text injection goes through
//! [`injector::TextInjector`]; AI work goes through the
//! `teletype_inference::InferenceProvider` trait.

pub mod audio;
pub mod autotext;
pub mod context;
pub mod injector;
pub mod llm;
pub mod personalization;
pub mod pipeline;
pub mod platform;
pub mod shortcuts;
pub mod state;
pub mod storage;
pub mod transforms;
