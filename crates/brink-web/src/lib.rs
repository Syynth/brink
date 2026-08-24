//! WASM bindings for the brink IDE and runtime — the web playground surface.
//!
//! Split by responsibility (issue #651, mechanical — no behavior change):
//! - [`compile`] — stateless compile entrypoints (`compile`, `compile_fragment`,
//!   `program_checksum`) and the shared `CompileResult`/`DiagnosticJs` wire types.
//! - [`value_marshal`] — `Value`/`Line`/`DebugSnapshot` ↔ JS/JSON marshaling
//!   shared by every stateful surface below.
//! - [`external_binding`] — the ink↔JS external-function call boundary
//!   (`JsHandler`, `RecordingReplayHandler`) and the reentrancy guard.
//! - [`story_runner`] — `StoryRunner`, the Program Explorer wasm surface.
//! - [`speculation`] — `WebSpeculation`, the sandboxed speculative-eval surface.
//! - [`session`] — `WebSession`, the Story Session wasm surface (#370/#387).
//! - [`editor`] — `EditorSession`, the IDE/fmt bridge (document state, IDE
//!   queries, fragment view rebasing/splicing).
//! - [`editor_dto`] — the JSON DTOs `editor`'s IDE queries return.
//! - [`editor_refactor`] — structural-edit JSON builders (rename, move,
//!   extract, delete) shared by `editor`.
//!
//! `pub use` re-exports below keep the wasm-bindgen surface — every
//! `#[wasm_bindgen]` item's name and signature — identical to pre-split
//! `lib.rs`; module extraction is purely an internal file layout change.

mod compile;
mod editor;
mod editor_dto;
mod editor_refactor;
mod external_binding;
mod perf;
mod program_model;
mod session;
mod speculation;
mod story_runner;
mod value_marshal;

pub use compile::{compile, compile_fragment, program_checksum};
pub use editor::EditorSession;
pub use editor_dto::{token_modifier_names, token_type_names};
pub use session::{WebSession, diff_snapshots};
pub use speculation::WebSpeculation;
pub use story_runner::StoryRunner;
