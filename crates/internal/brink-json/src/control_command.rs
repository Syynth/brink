use std::fmt;
use std::str::FromStr;

use serde::{Serialize, Serializer};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ControlCommandParseError {
    #[error("Invalid command: {0}")]
    InvalidCommand(String),
}

/// Control commands are special instructions to the text engine to perform
/// various actions. They are all represented by a particular text string:
#[derive(Debug, Clone, PartialEq)]
pub enum ControlCommand {
    /// "ev"
    /// Begin logical evaluation mode. In evaluation mode, objects that are
    /// encountered are added to an evaluation stack, rather than simply echoed
    /// into the main text output stream. As they're pushed onto the stack,
    /// they may be processed by other commands, functions, etc.
    BeginLogicalEval,

    /// "/ev"
    /// End logical evaluation mode. Future objects will be appended to the
    /// output stream rather than to the evaluation stack.
    EndLogicalEval,

    /// "out"
    /// The topmost object on the evaluation stack is popped and appended to
    /// the output stream (main story output).
    Output,

    /// "pop"
    /// Pops a value from the evaluation stack, without appending to the output
    /// stream.
    Pop,

    /// "->->"
    /// pop the callstack - used for returning from a tunnel. They are
    /// specified independently for error checking, since the callstack is
    /// aware of whether each element was pushed as a tunnel or function in the
    /// first place.
    TunnelReturn,

    /// "~ret"
    /// pop the callstack - used for returning from a function. They are
    /// specified independently for error checking, since the callstack is
    /// aware of whether each element was pushed as a tunnel or function in the
    /// first place.
    FunctionReturn,

    /// "du"
    /// Duplicate the topmost object on the evaluation stack. Useful since some
    /// commands consume objects on the evaluation stack.
    Duplicate,

    /// "str"
    /// Begin string evaluation mode. Adds a marker to the output stream, and
    /// goes into content mode (from evaluation mode). Must have already been
    /// in evaluation mode when this is encountered. See below for explanation.
    BeginStringEval,

    /// "/str"
    /// End string evaluation mode. All content after the previous Begin marker
    /// is concatenated together, removed from the output stream, and appended
    /// as a string value to the evaluation stack. Re-enters evaluation mode
    /// immediately afterwards.
    EndStringEval,

    /// "nop"
    /// No-operation. Does nothing, but is useful as an addressable piece of
    /// content to divert to.
    NoOperation,

    /// "choiceCnt"
    /// Pushes an integer with the current number of choices to the evaluation
    /// stack.
    ChoiceCount,

    /// "turn"
    /// Pushes an integer with the current turn number to the evaluation stack.
    Turn,

    /// "turns"
    /// Pops from the evaluation stack, expecting to see a divert target for a
    /// knot, stitch, gather or choice. Pushes an integer with the number of
    /// turns since that target was last visited by the story engine.
    Turns,

    /// "visit"
    /// Pushes an integer with the number of visits to the current container by
    /// the story engine.
    Visit,

    /// "seq"
    /// Pops an integer, expected to be the number of elements in a sequence
    /// that's being entered. In return, it pushes an integer with the next
    /// sequence shuffle index to the evaluation stack. This shuffle index is
    /// derived from the number of elements in the sequence, the number of
    /// elements in it, and the story's random seed from when it was first
    /// begun.
    Sequence,

    /// "thread"
    /// Clones/starts a new thread, as used with the <- knot syntax in ink.
    /// This essentially clones the entire callstack, branching it.
    Thread,

    /// "done"
    /// Tries to close/pop the active thread, otherwise marks the story flow
    /// safe to exit without a loose end warning.
    Done,

    /// "end"
    /// Ends the story flow immediately, closes all active threads, unwinds the
    /// callstack, and removes any choices that were previously created.
    End,

    /// "#"
    /// Marks the beginning of a tag. The tag text is built up on the output
    /// stream and then popped as a tag value.
    Tag,

    /// "<>"
    /// Glue. Joins text on either side, removing any newlines between them.
    Glue,

    /// "/#"
    /// End tag. Pops content from the output stream back to the last tag
    /// marker and creates a tag.
    EndTag,
}

impl fmt::Display for ControlCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::BeginLogicalEval => "ev",
            Self::EndLogicalEval => "/ev",
            Self::Output => "out",
            Self::Pop => "pop",
            Self::TunnelReturn => "->->",
            Self::FunctionReturn => "~ret",
            Self::Duplicate => "du",
            Self::BeginStringEval => "str",
            Self::EndStringEval => "/str",
            Self::NoOperation => "nop",
            Self::ChoiceCount => "choiceCnt",
            Self::Turn => "turn",
            Self::Turns => "turns",
            Self::Visit => "visit",
            Self::Sequence => "seq",
            Self::Thread => "thread",
            Self::Done => "done",
            Self::End => "end",
            Self::Tag => "#",
            Self::Glue => "<>",
            Self::EndTag => "/#",
        })
    }
}

impl Serialize for ControlCommand {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl FromStr for ControlCommand {
    type Err = ControlCommandParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ev" => Ok(ControlCommand::BeginLogicalEval),
            "/ev" => Ok(ControlCommand::EndLogicalEval),
            "out" => Ok(ControlCommand::Output),
            "pop" => Ok(ControlCommand::Pop),
            "->->" => Ok(ControlCommand::TunnelReturn),
            "~ret" => Ok(ControlCommand::FunctionReturn),
            "du" => Ok(ControlCommand::Duplicate),
            "str" => Ok(ControlCommand::BeginStringEval),
            "/str" => Ok(ControlCommand::EndStringEval),
            "nop" => Ok(ControlCommand::NoOperation),
            "choiceCnt" => Ok(ControlCommand::ChoiceCount),
            "turn" => Ok(ControlCommand::Turn),
            "turns" => Ok(ControlCommand::Turns),
            "visit" => Ok(ControlCommand::Visit),
            "seq" => Ok(ControlCommand::Sequence),
            "thread" => Ok(ControlCommand::Thread),
            "done" => Ok(ControlCommand::Done),
            "end" => Ok(ControlCommand::End),
            "#" => Ok(ControlCommand::Tag),
            "<>" => Ok(ControlCommand::Glue),
            "/#" => Ok(ControlCommand::EndTag),
            _ => Err(ControlCommandParseError::InvalidCommand(s.to_string())),
        }
    }
}
