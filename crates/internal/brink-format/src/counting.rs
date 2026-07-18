use bitflags::bitflags;

bitflags! {
    /// Container-level structural flags: what the runtime counts for a
    /// container, plus the FS-3 "invisible" marker.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct CountingFlags: u8 {
        /// Track how many times this container has been visited.
        const VISITS          = 0x01;
        /// Track the turn number at which this container was last visited.
        const TURNS           = 0x02;
        /// Only count the visit/turn when the container is entered at its
        /// first line (not when re-entered mid-way).
        const COUNT_START_ONLY = 0x04;
        /// This is a synthesized **invisible continuation container**
        /// (`docs/flow-suspension-spec.md` §11.2): the tail of an awaiting
        /// def split off at an `await` site (§11.1). It is compiler plumbing,
        /// not story structure, so it carries **no visit counts** (never
        /// [`VISITS`](Self::VISITS)/[`TURNS`](Self::TURNS) — they would
        /// pollute `shuffle`/`once` semantics in behavior loops), is **not a
        /// valid divert target**, and is **hidden from IDE navigation and
        /// completion** (debug views such as the `.inkt` dump excepted). The
        /// runtime enters it only via the FlowFrame resume path, never a
        /// user-authored divert. Reserved-through-fence: nothing in the
        /// compiler emits it while the E052 `await` lowering fence stands
        /// (FS-3c); its first emission rides the continuation-splitting
        /// codegen when the fence drops (FS-3r).
        const INVISIBLE       = 0x08;
    }
}
