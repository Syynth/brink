//! Binary interface between the brink compiler and runtime.
//!
//! This crate defines the types shared across the compiler/runtime boundary:
//! `DefinitionId`, opcodes, value types, line templates, and serialization
//! for `.inkb`, `.inkl`, and `.inkt` formats.
//!
//! `brink-runtime` depends ONLY on this crate — nothing else from brink.
