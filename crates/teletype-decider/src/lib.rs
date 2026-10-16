//! NeuDecide runtime: a small ONNX model that turns speech directly into
//! tool calls.
//!
//! The crate is split into four concerns:
//!
//! - [`tool_json`] – Python-`json.dumps`-compatible serialization of tool
//!   lists (the model was trained on that exact format).
//! - [`tools`] – the v1 tool definitions as data.
//! - [`parse`] – tolerant parsing of the model's decoded output into
//!   [`ToolCall`]s.
//! - [`runtime`] – the ONNX inference loop, abstracted behind
//!   [`runtime::SessionRunner`] so the greedy decode is unit-testable
//!   without a native ONNX runtime.
//!
//! The ONNX runtime itself is behind the `onnx` feature (default OFF).
//! Without it, [`NeuDecide::load`] and [`NeuDecide::generate`] return
//! [`DeciderError::RuntimeNotEnabled`].

pub mod error;
pub mod parse;
pub mod runtime;
pub mod tool_json;
pub mod tools;

pub use error::{DeciderError, Result};
pub use parse::parse_tool_calls;
pub use runtime::{NeuDecide, NeuDecideConfig, SessionRunner};
pub use tool::ToolCall;
pub use tool_json::{dump_python_json, format_python_float, tools_to_json, v1_tools, ToolDef};

mod tool;
