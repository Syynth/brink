//! Runtime/VM for executing compiled ink stories.
//!
//! The runtime takes a [`StoryData`](brink_format::StoryData) from the compiler,
//! links it into an immutable [`Program`], and executes it via [`Story`].
//!
//! ```ignore
//! let (program, line_tables) = brink_runtime::link(&story_data)?;
//! let mut story = brink_runtime::Story::new(&program, line_tables);
//! loop {
//!     match story.continue_single()? {
//!         Line::Text { text, .. } => print!("{text}"),
//!         Line::Choices { text, choices, .. } => {
//!             print!("{text}");
//!             // pick a choice...
//!             story.choose(0)?;
//!         }
//!         Line::End { text, .. } => {
//!             print!("{text}");
//!             break;
//!         }
//!     }
//! }
//! ```

mod error;
mod linker;
mod list_ops;
mod locale;
mod output;
mod program;
pub mod rng;
mod state;
mod story;
mod value_ops;
mod vm;

pub use error::RuntimeError;
pub use linker::link;
pub use locale::{LocaleMode, apply_locale};
pub use program::Program;
pub use rng::{DotNetRng, FastRng, StoryRng};
pub use state::WriteObserver;
pub use story::{Choice, ExternalFnHandler, ExternalResult, Line, Stats, Story, StorySnapshot};
