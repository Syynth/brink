use bitflags::bitflags;

bitflags! {
    /// Flags controlling what the runtime counts for a container.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct CountingFlags: u8 {
        /// Track how many times this container has been visited.
        const VISITS          = 0x01;
        /// Track the turn number at which this container was last visited.
        const TURNS           = 0x02;
        /// Only count the visit/turn when the container is entered at its
        /// first line (not when re-entered mid-way).
        const COUNT_START_ONLY = 0x04;
    }
}
