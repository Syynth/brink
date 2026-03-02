//! Converts inklecate `.ink.json` files to brink `.inkb` format.
//!
//! Reads the reference ink compiler's JSON output (via `brink-json`),
//! maps reference instructions to brink opcodes, and produces `.inkb`
//! files (via `brink-format`). Used to bootstrap runtime testing against
//! the 937 golden test files without needing the brink compiler.
