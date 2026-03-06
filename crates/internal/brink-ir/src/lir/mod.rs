//! Low-level IR: resolved, container-centric program representation.
//!
//! The LIR is the output of HIR + analysis (`SymbolIndex`, `ResolutionMap`).
//! All references are resolved to `DefinitionId`s or temp slot indices,
//! container boundaries are decided, and the program is organized as a
//! flat list of containers.
//!
//! Two backends consume the LIR:
//! - **Bytecode backend:** linearizes to opcodes + line tables → `.inkb`
//! - **JSON backend:** serializes to `.ink.json` (inklecate-compatible)
//!
//! ## Design properties
//!
//! - **Flat container list.** Every knot, stitch, gather, and choice target
//!   is a separate `Container` with its own `DefinitionId`. Container
//!   boundaries are decided during HIR → LIR lowering, not by backends.
//!
//! - **Structured statements.** Conditionals, sequences, and choice sets
//!   keep their branch structure within each container. Each backend
//!   serializes this structure into its output format (jump offsets for
//!   bytecode, nested arrays for JSON). This avoids committing to a
//!   bytecode-specific linearization that the JSON backend can't use.
//!
//! - **Fully resolved.** No unresolved `Path` nodes. Every reference is
//!   a `DefinitionId` (globals, containers, list items, externals) or a
//!   temp slot index (`u16`). The LIR never needs the `SymbolIndex` or
//!   `ResolutionMap` — all lookups are done during lowering.

mod types;

pub use types::*;
