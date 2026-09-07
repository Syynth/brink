pub mod binder_order;
pub mod compiled;
pub mod fixes;
pub mod graph;
pub mod play;
pub mod program;
pub mod prose;
pub mod query;
pub mod tokens;
pub mod worker;

/// The mounted stdlib, as `(root-relative key, source text)`.
///
/// The Binder's Library section (ruled 2026-08-06 in the web): the modules
/// a project analyses against but does not own. Re-exported from
/// `brink-environment` — the single place a stdlib module is registered —
/// so the studio cannot drift from what the session actually mounts.
#[must_use]
pub fn library_sources() -> &'static [(&'static str, &'static str)] {
    brink_environment::stdlib_sources()
}
