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
            _ => Err(NativeFunctionParseError::InvalidFunction(s.to_string())),
        }
    }
}
