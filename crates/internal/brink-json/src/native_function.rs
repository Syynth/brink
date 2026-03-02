use std::fmt;
use std::str::FromStr;

use serde::{Serialize, Serializer};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum NativeFunctionParseError {
    #[error("Invalid native function: {0}")]
    InvalidFunction(String),
}

/// These are mathematical and logical functions that pop 1 or 2 arguments from
/// the evaluation stack, evaluate the result, and push the result back onto
/// the evaluation stack.
///
/// Booleans are supported only in the C-style - i.e. as integers where non-zero
/// is treated as "true" and zero as "false". The true result of a boolean
/// operation is pushed to the evaluation stack as 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NativeFunction {
    /// "+"
    Add,

    /// "-"
    Subtract,

    /// "/"
    Divide,

    /// "*"
    Multiply,

    /// "%"
    Modulo,

    /// "_"
    Negate,

    /// "=="
    Equal,

    /// "!="
    NotEqual,

    /// ">"
    GreaterThan,

    /// "<"
    LessThan,

    /// ">="
    GreaterThanEqual,

    /// "<="
    LessThanEqual,

    /// "&&"
    And,

    /// "||"
    Or,

    /// "!"
    Not,

    /// "MIN"
    Min,

    /// "MAX"
    Max,

    /// "?"
    /// List has/contains
    Has,

    /// "!?"
    /// List has-not/doesn't-contain
    HasNot,

    /// "L^"
    /// List intersect
    Intersect,

    /// "rnd"
    /// Random integer between two values
    Random,

    /// "srnd"
    /// Seed the random number generator
    SeedRandom,

    /// "readc"
    /// Read count of a target
    ReadCount,

    /// "FLOOR"
    /// Floor of a float
    Floor,

    /// "CEILING"
    /// Ceiling of a float
    Ceiling,

    /// "INT"
    /// Cast to integer
    IntCast,

    /// "FLOAT"
    /// Cast to float
    FloatCast,

    /// "POW"
    /// Power/exponentiation
    Pow,

    /// `LIST_COUNT`
    /// Count of items in a list
    ListCount,

    /// `LIST_ALL`
    /// All items in a list's origin
    ListAll,

    /// `LIST_MIN`
    /// Minimum item in a list
    ListMin,

    /// `LIST_MAX`
    /// Maximum item in a list
    ListMax,

    /// `LIST_VALUE`
    /// Get the integer value of a list item
    ListValue,

    /// `LIST_RANDOM`
    /// Random item from a list
    ListRandom,

    /// `LIST_RANGE`
    /// Range of list items
    ListRange,

    /// `LIST_INVERT`
    /// Invert a list
    ListInvert,

    /// "range"
    /// Clamp a value to a range
    Range,

    /// "listInt"
    /// Convert list item + value to a list
    ListInt,

    /// "lrnd"
    /// Random from a list range
    ListRandom2,
}

impl fmt::Display for NativeFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Add => "+",
            Self::Subtract => "-",
            Self::Divide => "/",
            Self::Multiply => "*",
            Self::Modulo => "%",
            Self::Negate => "_",
            Self::Equal => "==",
            Self::NotEqual => "!=",
            Self::GreaterThan => ">",
            Self::LessThan => "<",
            Self::GreaterThanEqual => ">=",
            Self::LessThanEqual => "<=",
            Self::And => "&&",
            Self::Or => "||",
            Self::Not => "!",
            Self::Min => "MIN",
            Self::Max => "MAX",
            Self::Has => "?",
            Self::HasNot => "!?",
            Self::Intersect => "L^",
            Self::Random => "rnd",
            Self::SeedRandom => "srnd",
            Self::ReadCount => "readc",
            Self::Floor => "FLOOR",
            Self::Ceiling => "CEILING",
            Self::IntCast => "INT",
            Self::FloatCast => "FLOAT",
            Self::Pow => "POW",
            Self::ListCount => "LIST_COUNT",
            Self::ListAll => "LIST_ALL",
            Self::ListMin => "LIST_MIN",
            Self::ListMax => "LIST_MAX",
            Self::ListValue => "LIST_VALUE",
            Self::ListRandom => "LIST_RANDOM",
            Self::ListRange => "LIST_RANGE",
            Self::ListInvert => "LIST_INVERT",
            Self::Range => "range",
            Self::ListInt => "listInt",
            Self::ListRandom2 => "lrnd",
        })
    }
}

impl Serialize for NativeFunction {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl FromStr for NativeFunction {
    type Err = NativeFunctionParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "+" => Ok(NativeFunction::Add),
            "-" => Ok(NativeFunction::Subtract),
            "/" => Ok(NativeFunction::Divide),
            "*" => Ok(NativeFunction::Multiply),
            "%" => Ok(NativeFunction::Modulo),
            "_" => Ok(NativeFunction::Negate),
            "==" => Ok(NativeFunction::Equal),
            "!=" => Ok(NativeFunction::NotEqual),
            ">" => Ok(NativeFunction::GreaterThan),
            "<" => Ok(NativeFunction::LessThan),
            ">=" => Ok(NativeFunction::GreaterThanEqual),
            "<=" => Ok(NativeFunction::LessThanEqual),
            "&&" => Ok(NativeFunction::And),
            "||" => Ok(NativeFunction::Or),
            "!" => Ok(NativeFunction::Not),
            "MIN" => Ok(NativeFunction::Min),
            "MAX" => Ok(NativeFunction::Max),
            "?" => Ok(NativeFunction::Has),
            "!?" => Ok(NativeFunction::HasNot),
            "L^" => Ok(NativeFunction::Intersect),
            "rnd" => Ok(NativeFunction::Random),
            "srnd" => Ok(NativeFunction::SeedRandom),
            "readc" => Ok(NativeFunction::ReadCount),
            "FLOOR" => Ok(NativeFunction::Floor),
            "CEILING" => Ok(NativeFunction::Ceiling),
            "INT" => Ok(NativeFunction::IntCast),
            "FLOAT" => Ok(NativeFunction::FloatCast),
            "POW" => Ok(NativeFunction::Pow),
            "LIST_COUNT" => Ok(NativeFunction::ListCount),
            "LIST_ALL" => Ok(NativeFunction::ListAll),
            "LIST_MIN" => Ok(NativeFunction::ListMin),
            "LIST_MAX" => Ok(NativeFunction::ListMax),
            "LIST_VALUE" => Ok(NativeFunction::ListValue),
            "LIST_RANDOM" => Ok(NativeFunction::ListRandom),
            "LIST_RANGE" => Ok(NativeFunction::ListRange),
            "LIST_INVERT" => Ok(NativeFunction::ListInvert),
            "range" => Ok(NativeFunction::Range),
            "listInt" => Ok(NativeFunction::ListInt),
            "lrnd" => Ok(NativeFunction::ListRandom2),
            _ => Err(NativeFunctionParseError::InvalidFunction(s.to_string())),
        }
    }
}
