//! Read-only debug introspection for the studio State View.
//!
//! [`Story::debug_snapshot`](crate::Story::debug_snapshot) produces a
//! [`DebugSnapshot`] — a name-resolved, structured view of the runtime's
//! current state (location, globals, call stack, visit counts, pending
//! choices, rng). Unlike the VM internals, everything here is resolved to
//! author-facing knot/stitch paths and variable names.
//!
//! This is built on demand and is not on any hot path.

use std::collections::HashMap;

use brink_format::{DefinitionId, Value};

use crate::program::Program;

/// A structured, read-only snapshot of the runtime's current state.
pub struct DebugSnapshot {
    /// Execution status: `active` / `waiting_for_choice` / `done` / `ended`.
    pub status: &'static str,
    /// Nearest named knot/stitch the cursor is currently in, if resolvable.
    pub current_location: Option<String>,
    /// Current turn index.
    pub turn_index: u32,
    /// Global variables and their current values (display strings).
    pub globals: Vec<DebugGlobal>,
    /// Active call frames, innermost (current) first.
    pub call_stack: Vec<DebugFrame>,
    /// Per-knot/stitch visit counts, sorted by path.
    pub visit_counts: Vec<DebugVisit>,
    /// Choices currently offered to the player.
    pub pending_choices: Vec<DebugChoice>,
    /// Story RNG state.
    pub rng: DebugRng,
}

/// A global variable and its current value.
pub struct DebugGlobal {
    pub name: String,
    pub value: String,
}

/// One call frame, resolved to a knot/stitch path.
pub struct DebugFrame {
    /// Frame kind: `root` / `function` / `tunnel` / `thread` / `external` / `eval`.
    pub kind: &'static str,
    /// Nearest named container for this frame, if resolvable.
    pub location: Option<String>,
    /// Number of temporary (local) variables in this frame.
    pub temps: usize,
}

/// A visit count for a named knot/stitch.
pub struct DebugVisit {
    pub path: String,
    pub count: u32,
}

/// A pending choice and the knot it targets.
pub struct DebugChoice {
    pub text: String,
    pub target: Option<String>,
}

/// Story RNG state.
pub struct DebugRng {
    pub seed: i32,
    pub previous: i32,
}

/// Resolves container indices / definition ids to author-facing paths and
/// formats values for display. Holds a one-time reverse map of the program's
/// `address_by_path` table.
pub(crate) struct NameResolver<'p> {
    program: &'p Program,
    /// `container_idx → shortest knot/stitch path` (offset-0 scope entries).
    rev: HashMap<u32, String>,
}

impl<'p> NameResolver<'p> {
    pub(crate) fn new(program: &'p Program) -> Self {
        let mut rev: HashMap<u32, String> = HashMap::new();
        for (path, (idx, off)) in &program.address_by_path {
            if *off != 0 {
                continue;
            }
            // Deterministic on collision: shortest path, then lexicographically
            // smallest — independent of HashMap iteration order.
            let better = match rev.get(idx) {
                None => true,
                Some(existing) => {
                    path.len() < existing.len()
                        || (path.len() == existing.len() && path.as_str() < existing.as_str())
                }
            };
            if better {
                rev.insert(*idx, path.clone());
            }
        }
        Self { program, rev }
    }

    /// The knot/stitch path for a container, if it names a scope.
    pub(crate) fn container_path(&self, idx: u32) -> Option<&str> {
        self.rev.get(&idx).map(String::as_str)
    }

    /// The knot/stitch path a definition id lives in, if resolvable.
    pub(crate) fn def_path(&self, id: DefinitionId) -> Option<&str> {
        let (idx, _) = self.program.resolve_target(id)?;
        self.container_path(idx)
    }

    /// Format a runtime value for display, resolving names where possible.
    pub(crate) fn format_value(&self, value: &Value) -> String {
        match value {
            Value::Int(i) => i.to_string(),
            Value::Float(f) => f.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::String(s) => format!("\"{s}\""),
            Value::Null => "null".to_owned(),
            Value::List(list) => {
                let members: Vec<&str> = list
                    .items
                    .iter()
                    .filter_map(|id| self.program.list_item_name(*id))
                    .collect();
                format!("({})", members.join(", "))
            }
            Value::DivertTarget(id) => match self.def_path(*id) {
                Some(p) => format!("-> {p}"),
                None => "-> ?".to_owned(),
            },
            Value::VariablePointer(id) => match self.program.global_var_name(*id) {
                Some(n) => format!("ref {n}"),
                None => "ref ?".to_owned(),
            },
            Value::TempPointer { slot, frame_depth } => {
                format!("temp[{slot}]@{frame_depth}")
            }
            Value::FragmentRef(idx) => format!("<fragment {idx}>"),
        }
    }
}
