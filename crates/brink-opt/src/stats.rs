//! Artifact measurement (`docs/optimizer-spec.md` §3).
//!
//! Every metric that matters is measurable in one place, because it all lives
//! in the artifact. That is the quiet advantage of the post-compile placement:
//! under the earlier LIR placement, `line_entries` — the only figure here
//! denominated in human cost rather than machine cost — was not measurable at
//! all, because line tables do not exist until codegen has run.

use brink_format::StoryData;

/// A size measurement of one artifact.
///
/// Reported before and after a run (`crate::OptReport`), so a pass can be shown
/// to have moved its target metric. A pass that runs and changes nothing
/// measurable is a pass that silently does nothing, which is the failure the
/// generator property in `brink-gen/tests/opt_equivalence.rs` exists to catch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ArtifactStats {
    /// Compiled containers.
    pub containers: usize,
    /// Total bytecode across every container.
    pub bytecode_bytes: usize,
    /// Line-table entries — **translatable units**, the one metric denominated
    /// in human cost. This is what a translator is billed for.
    pub line_entries: usize,
    /// Interned names.
    pub name_table: usize,
    /// Literal-pool entries.
    pub literal_pool: usize,
    /// List-literal entries.
    pub list_literals: usize,
    /// Encoded size of the whole artifact.
    pub artifact_bytes: usize,
}

impl ArtifactStats {
    /// Measure a story.
    ///
    /// `artifact_bytes` requires an encode, so this is not free — it is called
    /// twice per `optimize` call and should not be called in a loop.
    #[must_use]
    pub fn measure(story: &StoryData) -> Self {
        let mut encoded = Vec::new();
        brink_format::write_inkb(story, &mut encoded);
        Self {
            containers: story.containers.len(),
            bytecode_bytes: story.containers.iter().map(|c| c.bytecode.len()).sum(),
            line_entries: story.line_tables.iter().map(|t| t.lines.len()).sum(),
            name_table: story.name_table.len(),
            literal_pool: story.literal_pool.len(),
            list_literals: story.list_literals.len(),
            artifact_bytes: encoded.len(),
        }
    }
}
