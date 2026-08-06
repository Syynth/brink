//! Shared JSON field-name constants for the host manifest file
//! (`docs/host-capability-manifest.md`) — the single on-disk manifest that
//! two independent Rust types deserialize, each reading only the subset of
//! keys it needs (issue #911, BH follow-up deliverable 1):
//!
//! - `brink_ir::host_manifest::HostManifest`/`ManifestExternal` (compiler/IDE
//!   side, tooling-only — signatures, semantic types, widgets; never
//!   consumed by codegen or the runtime).
//! - `bevy_brink::capability::CapabilityManifest`/`CapabilityManifestExternal`
//!   (host/ECS side — the `effects` capability grammar, `docs/effects-spec.md`
//!   §13.2; never consumed by the compiler or IDE).
//!
//! **Why one canonical type was rejected.** These two types live in crates on
//! opposite sides of a dependency boundary that must stay one-directional:
//! `brink-ir` is compiler/IDE-only (no ECS, no bevy dependency, ships to
//! `brink-web`'s wasm surface); `bevy-brink` is host/ECS-only (pulls in
//! `bevy_ecs`, never built into the compiler or IDE). A single shared type
//! would force either an unwanted dependency edge between them or a new
//! third crate both would depend on, for two fields (`externals[].name`) in
//! common and otherwise-disjoint concerns (tooling metadata vs. ECS capability
//! grammar) — not justified for a low-priority de-drift ask. The chosen fix is
//! minimal: these constants pin the two spellings both types' `externals`
//! wrapper and each entry's `name` field must keep using, so a rename on
//! either side is a deliberate, visible edit here, not silent drift; a
//! cross-validation test (`crates/bevy-brink/tests/manifest_field_convergence.rs`)
//! parses one literal manifest JSON built from these constants through both
//! types and asserts they agree.
//!
//! Every other key (`effects`/`reads`/`writes`/`detect` on the capability
//! side; `params`/`returns`/`kind`/`doc`/`widgets`/`path` and the top-level
//! `types`/`markup` sections on the tooling side) is owned by exactly one
//! consumer — serde's default "unknown fields are ignored" behavior is what
//! lets the same file carry both without either type needing to know about
//! the other's keys, so those names carry no shared-drift risk and aren't
//! duplicated here.

/// The top-level manifest wrapper key: `{"externals": [...]}`.
pub const EXTERNALS: &str = "externals";

/// Each entry's external-function name key, common to both consumers'
/// per-external structs.
pub const NAME: &str = "name";
