//! Runtime/VM for executing compiled ink stories.
//!
//! The runtime takes a [`StoryData`](brink_format::StoryData) from the compiler,
//! links it into an immutable [`Program`], and executes it via [`Story`].
//!
//! ```ignore
//! let program = brink_runtime::link(&story_data)?;
//! let mut story = brink_runtime::Story::new(&program);
//! loop {
//!     match story.step(&program)? {
//!         StepResult::Done { text, .. } => print!("{text}"),
//!         StepResult::Choices { text, choices, .. } => {
//!             print!("{text}");
//!             // pick a choice...
//!             story.choose(0)?;
//!         }
//!         StepResult::Ended { text, .. } => {
//!             print!("{text}");
//!             break;
//!         }
//!     }
//! }
//! ```

mod error;
mod linker;
mod list_ops;
mod output;
mod program;
mod story;
mod value_ops;
mod vm;

pub use error::RuntimeError;
pub use linker::link;
pub use program::Program;
pub use story::{Choice, StepResult, Story, StoryStatus};
