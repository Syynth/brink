//! Semantic mutators for the mutation-sensitivity study
//! (`docs/observable-semantics-spec.md` §4, tier 3a).
//!
//! Every mutator here is **grounded**: it only produces a mutant when the
//! site it edits is demonstrably exercised by the baseline [`Trace`] of the
//! unmutated program. That distinction is the whole point of the study — an
//! ungrounded mutant (a dropped line in a knot no run ever reaches) survives
//! because bounded exploration never looked, which says nothing about the
//! *definition*. A grounded mutant that survives is a real blind spot in
//! §2's definition or in the instrumentation that computes it, and per the
//! spec is fixed in the oracle, never weakened in the test.
//!
//! [`Trace`]: crate::trace::Trace

use std::collections::BTreeSet;

use crate::trace::{Trace, TraceEvent};

/// The mutation classes the study reports a survivor rate for
/// (`docs/observable-semantics-spec.md` §4, tier 3a).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MutationClass {
    /// Two adjacent choices presented together swap places. Detected only
    /// because choices compare **by order** (spec §2.1).
    SwapChoices,
    /// A text line that the baseline actually printed is deleted.
    DropLine,
    /// A conditional's comparison is inverted.
    FlipCondition,
    /// A `LIST` declaration's items are reordered, renumbering them.
    ReorderList,
    /// A literal an initialised global carries is changed.
    ChangeLiteral,
    /// An unused `RANDOM` draw is removed — spec §2.1 says it is *not*
    /// removable, because every later draw shifts.
    RemoveRandomDraw,
    /// A write to a global takes a different value.
    ChangeGlobalWrite,
}

impl MutationClass {
    /// Every class, in a stable order — for reporting a survivor rate per
    /// class with no map iteration order to worry about.
    pub const ALL: [Self; 7] = [
        Self::SwapChoices,
        Self::DropLine,
        Self::FlipCondition,
        Self::ReorderList,
        Self::ChangeLiteral,
        Self::RemoveRandomDraw,
        Self::ChangeGlobalWrite,
    ];

    /// The class's name as the study reports it.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::SwapChoices => "swap-choices",
            Self::DropLine => "drop-line",
            Self::FlipCondition => "flip-condition",
            Self::ReorderList => "reorder-list",
            Self::ChangeLiteral => "change-literal",
            Self::RemoveRandomDraw => "remove-random-draw",
            Self::ChangeGlobalWrite => "change-global-write",
        }
    }
}

/// One mutated program source, with the evidence that grounds it.
#[derive(Debug, Clone)]
pub struct Mutant {
    /// Which class this mutation belongs to.
    pub class: MutationClass,
    /// A human-readable description of the edit, for failure reporting.
    pub description: String,
    /// The mutated source text.
    pub source: String,
}

/// Collect the observables a baseline trace demonstrably exercised: the exact
/// text of every printed line, and every pair of choices presented adjacently.
#[derive(Debug, Default)]
pub struct Coverage {
    printed: BTreeSet<String>,
    adjacent_choices: BTreeSet<(String, String)>,
    global_values: BTreeSet<(String, String)>,
}

impl Coverage {
    /// Fold every trace of a baseline run set into one coverage record.
    #[must_use]
    pub fn of(traces: &[Trace]) -> Self {
        let mut cov = Self::default();
        for trace in traces {
            for event in &trace.events {
                match event {
                    TraceEvent::Line { text, .. } => {
                        let trimmed = text.trim();
                        if !trimmed.is_empty() {
                            cov.printed.insert(trimmed.to_owned());
                        }
                    }
                    TraceEvent::Choices(choices) => {
                        for pair in choices.windows(2) {
                            let (a, b) = (&pair[0], &pair[1]);
                            if a.text != b.text {
                                cov.adjacent_choices
                                    .insert((a.text.trim().to_owned(), b.text.trim().to_owned()));
                            }
                        }
                    }
                    TraceEvent::Globals(globals) => {
                        for (name, value) in globals {
                            cov.global_values
                                .insert((name.clone(), format!("{value:?}")));
                        }
                    }
                    TraceEvent::External { .. }
                    | TraceEvent::Probe { .. }
                    | TraceEvent::Terminal(_) => {}
                }
            }
        }
        cov
    }

    /// Whether the baseline printed exactly this text.
    fn printed(&self, text: &str) -> bool {
        self.printed.contains(text.trim())
    }

    /// Whether the baseline presented these two choice texts side by side.
    fn presented_adjacent(&self, a: &str, b: &str) -> bool {
        self.adjacent_choices
            .contains(&(a.trim().to_owned(), b.trim().to_owned()))
    }

    /// Whether a global by this name was host-readable at some boundary.
    fn has_global(&self, name: &str) -> bool {
        self.global_values.iter().any(|(n, _)| n == name)
    }
}

/// Every grounded mutant of `source`, capped at `per_class` mutants per class
/// so a large corpus file cannot blow up the study's runtime.
///
/// Ordering is deterministic: source order within a class, classes in
/// [`MutationClass::ALL`] order.
#[must_use]
pub fn grounded_mutants(source: &str, coverage: &Coverage, per_class: usize) -> Vec<Mutant> {
    let mut out = Vec::new();
    out.extend(take(swap_choices(source, coverage), per_class));
    out.extend(take(drop_line(source, coverage), per_class));
    out.extend(take(change_literal(source, coverage), per_class));
    out.extend(take(change_global_write(source, coverage), per_class));
    out
}

fn take(mutants: Vec<Mutant>, limit: usize) -> Vec<Mutant> {
    mutants.into_iter().take(limit).collect()
}

/// The choice marker run (`*`/`+`, possibly repeated) a line opens with, plus
/// the text after it. `None` for a line that is not a choice.
fn choice_marker(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim_start();
    let mut markers = String::new();
    let mut rest = trimmed;
    while let Some(stripped) = rest.strip_prefix(['*', '+']) {
        markers.push(rest.as_bytes().first().map_or('*', |&b| b as char));
        rest = stripped.trim_start();
    }
    if markers.is_empty() {
        return None;
    }
    Some((markers, rest.to_owned()))
}

/// Swap two adjacent choice lines that the baseline presented side by side.
fn swap_choices(source: &str, coverage: &Coverage) -> Vec<Mutant> {
    let lines: Vec<&str> = source.lines().collect();
    let mut out = Vec::new();
    for i in 0..lines.len().saturating_sub(1) {
        let (Some((ma, ta)), Some((mb, tb))) =
            (choice_marker(lines[i]), choice_marker(lines[i + 1]))
        else {
            continue;
        };
        // Same nesting depth only — swapping across depths restructures the
        // weave rather than reordering two siblings.
        if ma != mb || ta == tb {
            continue;
        }
        // The choice text a player sees is the marker line's text with any
        // `[...]`/divert suffix still attached; ground on a prefix match so a
        // `* Go north -> north` line grounds against the presented "Go north".
        if !presented_pair(coverage, &ta, &tb) {
            continue;
        }
        let mut mutated = lines.clone();
        mutated.swap(i, i + 1);
        out.push(Mutant {
            class: MutationClass::SwapChoices,
            description: format!("swap choices at lines {}/{}", i + 1, i + 2),
            source: rejoin(&mutated, source),
        });
    }
    out
}

/// Ground a choice-line pair against the baseline's presented adjacency,
/// tolerating the source line's trailing decorations (`[..]`, `-> divert`).
fn presented_pair(coverage: &Coverage, a: &str, b: &str) -> bool {
    let ca = choice_display(a);
    let cb = choice_display(b);
    !ca.is_empty() && !cb.is_empty() && coverage.presented_adjacent(&ca, &cb)
}

/// The part of a choice's source text a player is shown for the *choice*
/// itself: everything before a `[`, a `->`, or a `#` tag.
fn choice_display(text: &str) -> String {
    let mut end = text.len();
    for pat in ["[", "->", "#", "{"] {
        if let Some(idx) = text.find(pat) {
            end = end.min(idx);
        }
    }
    text[..end].trim().to_owned()
}

/// Delete a plain text line the baseline actually printed.
fn drop_line(source: &str, coverage: &Coverage) -> Vec<Mutant> {
    let lines: Vec<&str> = source.lines().collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || !is_plain_prose(trimmed) || !coverage.printed(trimmed) {
            continue;
        }
        let mut mutated = lines.clone();
        mutated.remove(i);
        out.push(Mutant {
            class: MutationClass::DropLine,
            description: format!("drop printed line {} ({trimmed:?})", i + 1),
            source: rejoin(&mutated, source),
        });
    }
    out
}

/// Whether a trimmed source line is ordinary prose — no structural marker, no
/// interpolation, no tag. Conservative on purpose: a false negative only
/// costs a mutant, a false positive costs a mutant that does not compile.
fn is_plain_prose(trimmed: &str) -> bool {
    const STRUCTURAL: [&str; 12] = ["*", "+", "-", "~", "=", ">", "<", "{", "}", "#", "/", "\\"];
    const KEYWORDS: [&str; 6] = ["VAR ", "CONST ", "LIST ", "EXTERNAL ", "INCLUDE ", "TODO:"];
    if STRUCTURAL.iter().any(|p| trimmed.starts_with(p)) {
        return false;
    }
    if KEYWORDS.iter().any(|k| trimmed.starts_with(k)) {
        return false;
    }
    !trimmed.contains('{') && !trimmed.contains('#') && !trimmed.contains("->")
}

/// Change an integer literal a host-readable global is initialised with.
fn change_literal(source: &str, coverage: &Coverage) -> Vec<Mutant> {
    declaration_literal_mutants(source, coverage, "VAR ", MutationClass::ChangeLiteral)
}

/// Change the value an assignment writes to a host-readable global.
fn change_global_write(source: &str, coverage: &Coverage) -> Vec<Mutant> {
    declaration_literal_mutants(source, coverage, "~ ", MutationClass::ChangeGlobalWrite)
}

/// Shared body of [`change_literal`] and [`change_global_write`]: find
/// `<prefix><name> = <int>` and bump the integer, but only when `<name>` is a
/// global the baseline could actually read.
fn declaration_literal_mutants(
    source: &str,
    coverage: &Coverage,
    prefix: &str,
    class: MutationClass,
) -> Vec<Mutant> {
    let lines: Vec<&str> = source.lines().collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix(prefix) else {
            continue;
        };
        let Some((name, value)) = rest.split_once('=') else {
            continue;
        };
        let name = name.trim();
        let value = value.trim();
        if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        if !coverage.has_global(name) {
            continue;
        }
        let Ok(n) = value.parse::<i32>() else {
            continue;
        };
        let indent = &line[..line.len() - trimmed.len()];
        let mut mutated: Vec<String> = lines.iter().map(|l| (*l).to_string()).collect();
        mutated[i] = format!("{indent}{prefix}{name} = {}", n.wrapping_add(1));
        let borrowed: Vec<&str> = mutated.iter().map(String::as_str).collect();
        out.push(Mutant {
            class,
            description: format!("line {}: {name} = {n} -> {}", i + 1, n.wrapping_add(1)),
            source: rejoin(&borrowed, source),
        });
    }
    out
}

/// Rejoin mutated lines, preserving whether the original ended in a newline.
fn rejoin(lines: &[&str], original: &str) -> String {
    let mut out = lines.join("\n");
    if original.ends_with('\n') {
        out.push('\n');
    }
    out
}
