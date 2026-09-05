//! Rule inference from marked lines (#3409) — the core of the
//! teach-by-example Conventions editor (RULED 2026-09-02) — and the
//! parsers it verifies against, in Rust.
//!
//! This is the Rust home of what `@brink-lang/dialect` ships in TypeScript
//! (`infer.ts`, `config.ts`, `DialectParser`, `runsOf`): the native studio
//! reads it directly, and the two are held together by one golden corpus
//! (`tests::CORPUS` here mirrors `infer-corpus.ts` line for line). The
//! TypeScript stays: it is the pure artifact a web editor or a game
//! engine shares without wasm. The rule is that a case added to one corpus
//! is added to the other.
//!
//! Explainable and verified, never clever:
//!
//! 1. PROPOSE a candidate shape per marked kind from a small fixed
//!    hypothesis space — an affix (common prefix/suffix, with `<>` glue),
//!    a `Name: text` line, or an all-caps line.
//! 2. REJECT any candidate a line of another mark also satisfies.
//!    Negatives are load-bearing: `Warning: the bridge is out` marked
//!    narration must kill a bare `…:` cue rule.
//! 3. VERIFY by re-parsing every line through [`parse_source`] with the
//!    candidate dialect and keeping only what reproduces the marks. The
//!    support counts the UI shows are re-parse results, not estimates.
//! 4. Whatever the shapes cannot settle becomes a DECISION for the author
//!    — never a guess. Narration versus dialogue among bare lines is
//!    positional, not shape-based, so it is always the author's call.
//!
//! The output is the full dialect artifact. Whether it fits the
//! `[dialogue]` table form is a separate, verified question
//! ([`to_dialogue_config`]); the file form is the ruled escape hatch.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use brink_ir::dialect::{
    AffixShape, ChainRule, DialectElement, DialogueDialect, ElementNature, EmittedShape,
    PRESET_NAMES, PatternShape, ResolvedDialect, SourceShape, Templates, affix_element,
    emitted_for_affix, preset_by_name,
};
use brink_project_config::{DialogueConfig, DialogueElementConfig};
use regex::Regex;

use crate::dialect_config::resolve_dialogue_config;

// ─── Source and emitted parsing (the `DialectParser` mirror) ───────────

/// One classified source line: its dialect kind (`None` when no element or
/// chain rule matched — plain narrative, blank, structural ink) and the
/// attrs the winning element captured or a chain rule carried forward.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLine {
    pub kind: Option<String>,
    /// Sorted by attr name; empty when `kind` is `None`.
    pub attrs: Vec<(String, String)>,
}

/// Whether a trimmed source line begins with ink structural syntax — a
/// divert, thread, tag, logic, choice, gather, header, comment or
/// declaration — and so must never chain into dialect content. The
/// TypeScript parser's `STRUCTURAL_LINE_PATTERN`, spelled without the
/// lookahead Rust's `regex` lacks.
#[must_use]
pub fn is_structural_line(trimmed: &str) -> bool {
    const PREFIXES: [&str; 14] = [
        "->",
        "<-",
        "#",
        "~",
        "*",
        "+",
        "=",
        "//",
        "/*",
        "{",
        "INCLUDE ",
        "EXTERNAL ",
        "VAR",
        "CONST",
    ];
    // `->` is a divert and `-` a gather: both structural, the same way the
    // TypeScript alternation `->|-(?!>)` reads them.
    trimmed.starts_with("LIST ")
        || trimmed.starts_with('-')
        || PREFIXES.iter().any(|p| trimmed.starts_with(p))
}

/// Classify source lines in order — one record per input line — the way
/// the editor does: an element's source pattern is tried against each
/// trimmed line in declaration order (first match wins); a narrative line
/// immediately following a classified line chains per the dialect's chain
/// rules, carrying the declared `carry` attrs; a blank line always breaks
/// the chain, and so does a structural line (it never becomes content).
#[must_use]
pub fn parse_source(dialect: &ResolvedDialect, lines: &[&str]) -> Vec<SourceLine> {
    let mut out: Vec<SourceLine> = Vec::with_capacity(lines.len());
    let mut carry: BTreeMap<String, String> = BTreeMap::new();
    for (i, text) in lines.iter().enumerate() {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            carry.clear();
            out.push(SourceLine {
                kind: None,
                attrs: Vec::new(),
            });
            continue;
        }
        let leading = text.len() - text.trim_start().len();
        let leading = u32::try_from(leading).unwrap_or(u32::MAX);
        if let Some(m) = dialect.classify(text.trim_start(), leading) {
            carry = m.attrs.iter().cloned().collect();
            out.push(SourceLine {
                kind: Some(m.kind),
                attrs: m.attrs,
            });
            continue;
        }
        let prev_kind = i
            .checked_sub(1)
            .and_then(|p| out[p].kind.clone())
            .filter(|_| !is_structural_line(trimmed));
        if let Some(prev_kind) = prev_kind
            && let Some(rule) = dialect.chain_rule_after(&prev_kind)
        {
            let carried: Vec<(String, String)> = rule
                .carry
                .iter()
                .filter_map(|name| carry.get(name).map(|v| (name.clone(), v.clone())))
                .collect();
            for (k, v) in &carried {
                carry.insert(k.clone(), v.clone());
            }
            out.push(SourceLine {
                kind: Some(rule.becomes.clone()),
                attrs: carried,
            });
            continue;
        }
        carry.clear();
        out.push(SourceLine {
            kind: None,
            attrs: Vec::new(),
        });
    }
    out
}

/// One segment of a composite emitted line. `kind: None` is a plain-text
/// remainder — no declared emitted shape matched at that position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmittedSegment {
    pub kind: Option<String>,
    /// The raw matched text, affixes and glue included.
    pub text: String,
    /// The extracted content-group value, when the kind has one.
    pub content: Option<String>,
}

/// The emitted-side parser: every declared kind's `emitted` shape,
/// compiled once.
pub struct EmittedParser {
    shapes: Vec<(String, Regex, EmittedShape)>,
}

impl EmittedParser {
    /// # Errors
    /// The first emitted pattern that does not compile, named.
    pub fn compile(dialect: &DialogueDialect) -> Result<Self, String> {
        let mut shapes = Vec::new();
        for el in &dialect.elements {
            let Some(emitted) = &el.emitted else {
                continue;
            };
            let re = Regex::new(&emitted.pattern)
                .map_err(|e| format!("emitted shape of `{}`: {e}", el.kind))?;
            shapes.push((el.kind.clone(), re, emitted.clone()));
        }
        Ok(Self { shapes })
    }

    /// Parse ONE runtime-emitted line into its segments (the pinned
    /// composite-segment protocol): walk left to right; at each position
    /// try every kind with an emitted shape in declaration order — at the
    /// start of the line only `reserved_prefix` shapes may open — and the
    /// first that matches there consumes its text; where none matches,
    /// the plain text up to the next position where one does is a segment.
    #[must_use]
    pub fn parse_emitted(&self, text: &str) -> Vec<EmittedSegment> {
        let mut segments = Vec::new();
        let mut pos = 0;
        let mut first = true;
        while pos < text.len() {
            let rest = &text[pos..];
            if let Some((segment, len)) = self.match_at(rest, first) {
                segments.push(segment);
                pos += len;
                first = false;
                continue;
            }
            let mut end = text.len();
            for p in (pos + 1)..text.len() {
                if !text.is_char_boundary(p) {
                    continue;
                }
                if self.match_at(&text[p..], false).is_some() {
                    end = p;
                    break;
                }
            }
            segments.push(EmittedSegment {
                kind: None,
                text: text[pos..end].to_owned(),
                content: None,
            });
            pos = end;
            first = false;
        }
        segments
    }

    fn match_at(&self, rest: &str, at_start: bool) -> Option<(EmittedSegment, usize)> {
        for (kind, re, shape) in &self.shapes {
            if at_start && !shape.reserved_prefix {
                continue;
            }
            let Some(caps) = re.captures(rest) else {
                continue;
            };
            let whole = caps.get(0)?;
            if whole.start() != 0 {
                continue;
            }
            let content = shape
                .content_group
                .as_deref()
                .and_then(|g| caps.name(g))
                .map(|m| m.as_str().to_owned());
            return Some((
                EmittedSegment {
                    kind: Some(kind.clone()),
                    text: whole.as_str().to_owned(),
                    content,
                },
                whole.end(),
            ));
        }
        None
    }
}

/// One emitted line as [`runs_of`] takes it; `boundary` marks that a turn
/// boundary (choices presented) preceded it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmittedLine {
    pub segments: Vec<EmittedSegment>,
    pub boundary: bool,
}

/// One dialogue run, or one standalone line, in the emitted stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmittedRun {
    /// The run's opening kind (the cue), the standalone line's own kind, or
    /// `None` for plain narrative outside any run.
    pub kind: Option<String>,
    /// What the chain rule carries from the opening segment onto the run.
    pub attrs: BTreeMap<String, String>,
    /// Indices into the input.
    pub lines: Vec<usize>,
}

/// The emitted-side run rule (#3388, RULED 2026-08-30): fold parsed
/// emitted lines into runs. A line opening with a triggering kind (one of
/// a chain rule's `after` kinds that has its own emitted shape) closes the
/// active run and opens a new one; a line opening with a `run_ends_at`
/// kind closes the run and stands alone; a turn boundary closes it when
/// `"choices"` is in `run_ends_at`; any other line joins an open run or
/// stands alone.
#[must_use]
pub fn runs_of(lines: &[EmittedLine], dialect: &DialogueDialect) -> Vec<EmittedRun> {
    let mut triggers: BTreeSet<&str> = BTreeSet::new();
    let mut enders: BTreeSet<&str> = BTreeSet::new();
    let mut choices_end = false;
    for rule in &dialect.chain {
        for k in &rule.after {
            if dialect
                .elements
                .iter()
                .any(|e| e.kind == *k && e.emitted.is_some())
            {
                triggers.insert(k);
            }
        }
        for k in &rule.run_ends_at {
            if k == "choices" {
                choices_end = true;
            } else {
                enders.insert(k);
            }
        }
    }
    let carry_for = |kind: &str| -> Vec<String> {
        dialect
            .chain
            .iter()
            .filter(|r| r.after.iter().any(|k| k == kind))
            .flat_map(|r| r.carry.iter().cloned())
            .collect()
    };
    let content_group_of = |kind: &str| -> Option<String> {
        dialect
            .elements
            .iter()
            .find(|e| e.kind == kind)
            .and_then(|e| e.emitted.as_ref())
            .and_then(|e| e.content_group.clone())
    };

    let mut runs: Vec<EmittedRun> = Vec::new();
    let mut open: Option<EmittedRun> = None;
    for (i, line) in lines.iter().enumerate() {
        if line.boundary
            && choices_end
            && let Some(run) = open.take()
        {
            runs.push(run);
        }
        let first = line.segments.first();
        let kind = first.and_then(|s| s.kind.clone());
        if let Some(k) = &kind
            && triggers.contains(k.as_str())
        {
            if let Some(run) = open.take() {
                runs.push(run);
            }
            let mut attrs = BTreeMap::new();
            let group = content_group_of(k);
            for name in carry_for(k) {
                if group.as_deref() == Some(name.as_str())
                    && let Some(content) = first.and_then(|s| s.content.clone())
                    && !content.is_empty()
                {
                    attrs.insert(name, content);
                }
            }
            open = Some(EmittedRun {
                kind: kind.clone(),
                attrs,
                lines: vec![i],
            });
            continue;
        }
        if let Some(k) = &kind
            && enders.contains(k.as_str())
        {
            if let Some(run) = open.take() {
                runs.push(run);
            }
            runs.push(EmittedRun {
                kind: kind.clone(),
                attrs: BTreeMap::new(),
                lines: vec![i],
            });
            continue;
        }
        if let Some(run) = open.as_mut() {
            run.lines.push(i);
            continue;
        }
        runs.push(EmittedRun {
            kind,
            attrs: BTreeMap::new(),
            lines: vec![i],
        });
    }
    if let Some(run) = open.take() {
        runs.push(run);
    }
    runs
}

// ─── The `[dialogue]` table projection (the `config.ts` mirror) ────────

/// The `[dialogue]` table that resolves to exactly `dialect`, or `None`
/// when the table form cannot express it (a chain rule the preset does not
/// carry, a pattern element that needs an emitted shape, …). Tried against
/// every preset; the first that fits wins. Verified, not inferred: the
/// candidate table is resolved through the real resolver and compared by
/// content.
#[must_use]
pub fn to_dialogue_config(dialect: &DialogueDialect) -> Option<DialogueConfig> {
    let run_ends_at = dialect
        .chain
        .first()
        .map(|r| r.run_ends_at.clone())
        .unwrap_or_default();
    for name in PRESET_NAMES {
        let Some(preset) = preset_by_name(name) else {
            continue;
        };
        let mut overlays = Vec::new();
        let mut fits = true;
        for el in &dialect.elements {
            if preset.elements.iter().any(|p| p == el) {
                continue;
            }
            if let Some(row) = affix_row_for(el) {
                overlays.push(row);
            } else {
                fits = false;
                break;
            }
        }
        if !fits {
            continue;
        }
        let config = DialogueConfig {
            preset: Some((*name).to_owned()),
            file: None,
            elements: overlays,
            run_ends_at: run_ends_at.clone(),
        };
        if resolve_dialogue_config(&config, &|_: &str| None).as_ref() == Ok(dialect) {
            return Some(config);
        }
    }
    None
}

/// The `[[dialogue.elements]]` row for an element the affix sugar can
/// express, or `None`.
fn affix_row_for(el: &DialectElement) -> Option<DialogueElementConfig> {
    if !el.malformed.is_empty() {
        return None;
    }
    let nature = match el.nature {
        ElementNature::Narrative => None,
        ElementNature::Machinery => Some("machinery".to_owned()),
        ElementNature::Structural => Some("structural".to_owned()),
    };
    let Some(source) = &el.source else {
        if el.emitted.is_some() {
            return None;
        }
        return Some(DialogueElementConfig {
            kind: el.kind.clone(),
            nature,
            ..DialogueElementConfig::default()
        });
    };
    let SourceShape::Affix(affix) = source else {
        return None;
    };
    if el.emitted.as_ref() != Some(&emitted_for_affix(affix)) {
        return None;
    }
    Some(DialogueElementConfig {
        kind: el.kind.clone(),
        nature,
        prefix: affix.prefix.clone().filter(|p| !p.is_empty()),
        suffix: affix.suffix.clone().filter(|s| !s.is_empty()),
        glued: affix.glued.then_some(true),
        content_role: Some(affix.content_role.clone()).filter(|r| r != "content"),
        pattern: None,
        template: None,
    })
}

// ─── Inference ─────────────────────────────────────────────────────────

/// What the author says a line is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Mark {
    Cue,
    Dialogue,
    Action,
    Narration,
    Parenthetical,
}

impl Mark {
    pub const ALL: [Mark; 5] = [
        Mark::Cue,
        Mark::Dialogue,
        Mark::Action,
        Mark::Narration,
        Mark::Parenthetical,
    ];

    /// The mark's name as the ids and messages spell it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Mark::Cue => "cue",
            Mark::Dialogue => "dialogue",
            Mark::Action => "action",
            Mark::Narration => "narration",
            Mark::Parenthetical => "parenthetical",
        }
    }

    /// The element kind the mark maps to; `None` for plain narrative.
    #[must_use]
    pub fn kind(self) -> Option<&'static str> {
        match self {
            Mark::Cue => Some("character"),
            Mark::Dialogue => Some("dialogue"),
            Mark::Action => Some("action"),
            Mark::Narration => None,
            Mark::Parenthetical => Some("parenthetical"),
        }
    }

    fn of_kind(kind: &str) -> Option<Mark> {
        Mark::ALL.into_iter().find(|m| m.kind() == Some(kind))
    }
}

/// Where a passage line came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Origin {
    #[default]
    Line,
    Choice,
    Gather,
}

/// One line the author may have marked.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MarkedLine {
    pub text: String,
    /// Tags ride separately from `text` (never part of a shape).
    pub tags: Vec<String>,
    pub origin: Origin,
    /// Absent = unmarked: still checked, never taught from.
    pub mark: Option<Mark>,
}

/// A rule the studio learned, in plain words, with the lines that support
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Learned {
    pub id: String,
    pub sentence: String,
    /// Indices of the lines this rule reproduces on re-parse.
    pub support: Vec<usize>,
    /// How many lines carried the mark this rule is about.
    pub total: usize,
}

/// Something the shapes could not settle; the author decides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub id: String,
    pub message: String,
    pub lines: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Inference {
    /// The proposed dialect; `None` when nothing was taught.
    pub dialect: Option<DialogueDialect>,
    pub learned: Vec<Learned>,
    pub decisions: Vec<Decision>,
}

const GLUE: &str = "<>";

fn quote(s: &str) -> String {
    format!("\u{201c}{s}\u{201d}")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum How {
    Affix,
    NameColon,
    Caps,
}

struct Shape {
    how: How,
    element: DialectElement,
    sentence: String,
    /// Extra learned sentences the shape carries (glue).
    extras: Vec<String>,
}

/// The longest run of non-letter, non-digit characters at the start of
/// every text.
fn common_prefix(texts: &[&str]) -> String {
    let Some(first) = texts.first() else {
        return String::new();
    };
    let mut p: Vec<char> = first.chars().collect();
    for t in &texts[1..] {
        let n = p
            .iter()
            .zip(t.chars())
            .take_while(|(a, b)| **a == *b)
            .count();
        p.truncate(n);
    }
    // Cut back to marker characters only: `@MARA`/`@MARK` share "@MAR",
    // but the marker is "@".
    let end = p.iter().take_while(|c| !c.is_alphanumeric()).count();
    p[..end].iter().collect()
}

fn common_suffix(texts: &[&str]) -> String {
    let Some(first) = texts.first() else {
        return String::new();
    };
    let mut s: Vec<char> = first.chars().collect();
    for t in &texts[1..] {
        let tc: Vec<char> = t.chars().collect();
        let n = s
            .iter()
            .rev()
            .zip(tc.iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        s = s[s.len() - n..].to_vec();
    }
    let start = s.len() - s.iter().rev().take_while(|c| !c.is_alphanumeric()).count();
    let tail: String = s[start..].iter().collect();
    // Sentence punctuation is how sentences end, not a marker: `> She sets
    // the lantern down.` must not learn "." as its suffix.
    tail.trim_end_matches(['.', '?', '!', '\u{2026}', ',', ';'])
        .to_owned()
}

fn describe_affix(prefix: &str, suffix: &str, glued: bool, what: &str) -> String {
    let mut parts = Vec::new();
    if !prefix.is_empty() {
        parts.push(format!("starts with {}", quote(prefix.trim_end())));
    }
    if !suffix.is_empty() {
        let shown = format!("{suffix}{}", if glued { GLUE } else { "" });
        parts.push(format!("ends with {}", quote(shown.trim_start())));
    }
    format!("A line that {} is {what}.", parts.join(" and "))
}

/// Hypothesis 1: a prefix/suffix marker, with `<>` glue split off the
/// suffix.
fn affix_shape(kind: &str, role: &str, texts: &[&str], what: &str) -> Option<Shape> {
    let glued = texts.iter().all(|t| t.ends_with(GLUE));
    let bodies: Vec<&str> = if glued {
        texts.iter().map(|t| &t[..t.len() - GLUE.len()]).collect()
    } else {
        texts.to_vec()
    };
    let prefix = common_prefix(&bodies);
    let suffix = common_suffix(&bodies);
    if prefix.is_empty() && suffix.is_empty() {
        return None;
    }
    // Content must not be empty on every line — a marker alone is not a
    // shape.
    if bodies
        .iter()
        .all(|b| b.chars().count() <= prefix.chars().count() + suffix.chars().count())
    {
        return None;
    }
    // A cue's content is a name: short, no sentence inside. This is what
    // keeps `"What's that?" my master asked.` from teaching `"` as a cue
    // marker with the whole sentence as the "speaker".
    if role == "speaker" {
        let plausible = bodies.iter().all(|b| {
            let chars: Vec<char> = b.chars().collect();
            let start = prefix.chars().count().min(chars.len());
            let end = chars
                .len()
                .saturating_sub(suffix.chars().count())
                .max(start);
            let content: String = chars[start..end].iter().collect();
            let content = content.trim();
            let n = content.chars().count();
            n > 0 && n <= 40 && !content.contains(['.', '?', '!', '"'])
        });
        if !plausible {
            return None;
        }
    }
    let element = affix_element(
        kind,
        ElementNature::Narrative,
        AffixShape {
            prefix: (!prefix.is_empty()).then(|| prefix.clone()),
            suffix: (!suffix.is_empty()).then(|| suffix.clone()),
            glued,
            content_role: role.to_owned(),
        },
    );
    let mut extras = Vec::new();
    if glued {
        extras.push(format!(
            "The {} at the end attaches it to the line after it.",
            quote(GLUE)
        ));
    }
    Some(Shape {
        how: How::Affix,
        element,
        sentence: describe_affix(&prefix, &suffix, glued, what),
        extras,
    })
}

// Hypothesis 2 (the ink docs' sub-format): `Name: text` on one line.
// Portable-regex subset — the same pattern travels to the TS parser.
const NAME_CLASS_FIRST: &str = "[A-Z\u{c0}-\u{de}]";
const NAME_CLASS_REST: &str = "[A-Za-z\u{c0}-\u{ff}0-9'\u{2019} -]";
const CAPS_CLASS_REST: &str = "[A-Z\u{c0}-\u{de}0-9 .'\u{2019}-]";

fn name_colon_pattern() -> String {
    format!(r"^(?<speaker>{NAME_CLASS_FIRST}{NAME_CLASS_REST}{{0,40}}?):\s+(?<content>\S.*)$")
}

fn name_colon_emitted() -> String {
    format!(r"^(?<speaker>{NAME_CLASS_FIRST}{NAME_CLASS_REST}{{0,40}}?):\s+")
}

fn caps_pattern() -> String {
    format!(r"^(?<speaker>{NAME_CLASS_FIRST}{CAPS_CLASS_REST}*)$")
}

#[expect(clippy::expect_used, reason = "a fixed pattern the tests compile")]
static NAME_COLON: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&name_colon_pattern()).expect("a fixed pattern"));
#[expect(clippy::expect_used, reason = "a fixed pattern the tests compile")]
static CAPS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&caps_pattern()).expect("a fixed pattern"));
#[expect(clippy::expect_used, reason = "a fixed pattern the tests compile")]
static TWO_CAPS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("[A-Z\u{c0}-\u{de}]{2}").expect("a fixed pattern"));

fn name_colon_shape(kind: &str, texts: &[&str]) -> Option<Shape> {
    if !texts.iter().all(|t| NAME_COLON.is_match(t)) {
        return None;
    }
    let element = DialectElement {
        kind: kind.to_owned(),
        nature: ElementNature::Narrative,
        source: Some(SourceShape::Pattern(PatternShape {
            pattern: name_colon_pattern(),
            content_group: Some("content".to_owned()),
            template_group: None,
            hidden: Vec::new(),
            template: "${speaker}: ${content}".to_owned(),
        })),
        emitted: Some(EmittedShape {
            pattern: name_colon_emitted(),
            content_group: Some("speaker".to_owned()),
            reserved_prefix: true,
        }),
        malformed: Vec::new(),
    };
    let first = texts[0];
    let sample = first.find(':').map_or(first, |at| &first[..=at]);
    Some(Shape {
        how: How::NameColon,
        element,
        sentence: format!(
            "A line that starts with a name and a colon, like {}, is a cue; the name is the speaker and the rest of the line is what they say.",
            quote(sample)
        ),
        extras: Vec::new(),
    })
}

/// Hypothesis 3 (screenplay): a line in capitals on its own.
fn caps_shape(kind: &str, texts: &[&str]) -> Option<Shape> {
    if !texts
        .iter()
        .all(|t| CAPS.is_match(t) && TWO_CAPS.is_match(t))
    {
        return None;
    }
    let element = DialectElement {
        kind: kind.to_owned(),
        nature: ElementNature::Narrative,
        source: Some(SourceShape::Pattern(PatternShape {
            pattern: caps_pattern(),
            content_group: Some("speaker".to_owned()),
            template_group: None,
            hidden: Vec::new(),
            template: "${speaker}".to_owned(),
        })),
        emitted: Some(EmittedShape {
            pattern: caps_pattern(),
            content_group: Some("speaker".to_owned()),
            reserved_prefix: true,
        }),
        malformed: Vec::new(),
    };
    Some(Shape {
        how: How::Caps,
        element,
        sentence: format!(
            "A line in capitals on its own, like {}, is a cue naming the speaker.",
            quote(texts[0])
        ),
        extras: Vec::new(),
    })
}

fn candidates_for(mark: Mark, texts: &[&str]) -> Vec<Shape> {
    let Some(kind) = mark.kind() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if mark == Mark::Cue {
        out.extend(affix_shape(
            kind,
            "speaker",
            texts,
            "a cue naming the speaker",
        ));
        out.extend(name_colon_shape(kind, texts));
        out.extend(caps_shape(kind, texts));
    } else {
        let what = if mark == Mark::Action {
            "an action line"
        } else {
            "a parenthetical"
        };
        out.extend(affix_shape(kind, "content", texts, what));
    }
    out
}

/// The `[dialogue]` table for learned affix elements over the shipped
/// preset.
fn preset_table(elements: &[DialectElement], run_ends_at: Vec<String>) -> DialogueConfig {
    let mut rows = Vec::new();
    for el in elements {
        let Some(SourceShape::Affix(affix)) = &el.source else {
            continue;
        };
        rows.push(DialogueElementConfig {
            kind: el.kind.clone(),
            nature: None,
            prefix: affix.prefix.clone().filter(|p| !p.is_empty()),
            suffix: affix.suffix.clone().filter(|s| !s.is_empty()),
            glued: affix.glued.then_some(true),
            content_role: Some(affix.content_role.clone()).filter(|r| r != "content"),
            pattern: None,
            template: None,
        });
    }
    DialogueConfig {
        preset: Some("at-cue".to_owned()),
        file: None,
        elements: rows,
        run_ends_at,
    }
}

/// Does `element`'s source shape classify `text`? (Isolated, no chain.)
fn matches_source(element: &DialectElement, text: &str) -> bool {
    let probe = DialogueDialect {
        version: 1,
        name: "probe".to_owned(),
        elements: vec![element.clone()],
        chain: Vec::new(),
        transitions: Vec::new(),
        templates: Templates::default(),
    };
    let Ok(resolved) = ResolvedDialect::compile(&probe) else {
        return false;
    };
    parse_source(&resolved, &[text])
        .first()
        .is_some_and(|l| l.kind.as_deref() == Some(element.kind.as_str()))
}

fn plural<'a>(n: usize, one: &'a str, many: &'a str) -> &'a str {
    if n == 1 { one } else { many }
}

/// Infer a dialect from marked lines. See the module doc for the four
/// steps; the result's `learned` sentences and `decisions` ids are the
/// UI's, and the corpus pins them.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "one pass, mirrored line for line on the TypeScript it is held against"
)]
pub fn infer_dialect(lines: &[MarkedLine]) -> Inference {
    let mut learned: Vec<Learned> = Vec::new();
    let mut decisions: Vec<Decision> = Vec::new();
    let idx = |m: Mark| -> Vec<usize> {
        lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.mark == Some(m))
            .map(|(i, _)| i)
            .collect()
    };

    let mut elements: Vec<DialectElement> = Vec::new();
    let mut shapes: Vec<(String, Shape)> = Vec::new();

    for mark in [Mark::Cue, Mark::Action, Mark::Parenthetical] {
        let positives = idx(mark);
        if positives.is_empty() {
            continue;
        }
        let texts: Vec<&str> = positives.iter().map(|&i| lines[i].text.as_str()).collect();
        let negatives: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.mark.is_some_and(|m| m != mark))
            .map(|(i, _)| i)
            .collect();
        let mut chosen: Option<Shape> = None;
        let mut rejected: Option<(Shape, Vec<usize>)> = None;
        for cand in candidates_for(mark, &texts) {
            let clashes: Vec<usize> = negatives
                .iter()
                .copied()
                .filter(|&i| matches_source(&cand.element, &lines[i].text))
                .collect();
            if clashes.is_empty() {
                chosen = Some(cand);
                break;
            }
            if rejected.is_none() {
                rejected = Some((cand, clashes));
            }
        }
        if let Some(shape) = chosen {
            elements.push(shape.element.clone());
            shapes.push((shape.element.kind.clone(), shape));
            continue;
        }
        if let Some((shape, clashes)) = rejected {
            let n = clashes.len();
            decisions.push(Decision {
                id: format!("{}-ambiguous", mark.as_str()),
                message: format!(
                    "{} — but {} would match too and {} marked differently. Mark {} the same way, or use a marker the other lines never start with.",
                    shape.sentence.trim_end_matches('.'),
                    plural(n, "this line", "these lines"),
                    plural(n, "is", "are"),
                    plural(n, "it", "them"),
                ),
                lines: clashes,
            });
            continue;
        }
        let all_tagged = positives.iter().all(|&i| !lines[i].tags.is_empty());
        let message = if all_tagged {
            format!(
                "These lines are marked {} but share no marker in their text — the {} seems to live in a tag ({}). This editor cannot express a tag-carried {} yet.",
                mark.as_str(),
                if mark == Mark::Cue {
                    "speaker"
                } else {
                    "marking"
                },
                quote(&format!(
                    "# {}",
                    lines[positives[0]].tags.first().map_or("", String::as_str)
                )),
                mark.as_str(),
            )
        } else {
            format!(
                "These lines are marked {} but share no marker the studio can learn. Give them one (a character at the start, or at the end), or mark them as something else.",
                mark.as_str()
            )
        };
        decisions.push(Decision {
            id: format!("{}-no-shape", mark.as_str()),
            message,
            lines: positives,
        });
    }

    let cue_kind = Mark::Cue.kind().unwrap_or("character");
    let has_cue = elements.iter().any(|e| e.kind == cue_kind);
    let dialogue_idx = idx(Mark::Dialogue);
    if !has_cue && !dialogue_idx.is_empty() {
        decisions.push(Decision {
            id: "dialogue-without-cue".to_owned(),
            message: "Lines are marked dialogue, but no cue names a speaker. Mark the line that names who is speaking as a cue.".to_owned(),
            lines: dialogue_idx.clone(),
        });
    }
    if idx(Mark::Cue).is_empty()
        && dialogue_idx.is_empty()
        && idx(Mark::Action).is_empty()
        && idx(Mark::Parenthetical).is_empty()
    {
        return Inference {
            dialect: None,
            learned: Vec::new(),
            decisions: vec![Decision {
                id: "nothing-marked".to_owned(),
                message: "Mark at least one line — a cue that names who is speaking is the usual place to start.".to_owned(),
                lines: Vec::new(),
            }],
        };
    }

    // The chain: which shaped kinds a following bare line belongs to, and
    // which end a run. Read off the marks: dialogue right after an action
    // means the action did not end the turn. A cue that carries its speech
    // on the same line (`Name: text`) owns no following lines unless the
    // author marked one as dialogue; a header-only cue (affix, caps)
    // always does — a glued cue with nothing after it means nothing.
    let cue_how = shapes
        .iter()
        .find(|(k, _)| k == cue_kind)
        .map(|(_, s)| s.how);
    let has_chain = has_cue && (!dialogue_idx.is_empty() || cue_how != Some(How::NameColon));
    let mut chain_after: Vec<String> = Vec::new();
    let mut run_ends_at: Vec<String> = vec!["choices".to_owned()];
    if has_chain {
        chain_after.push(cue_kind.to_owned());
        chain_after.push("dialogue".to_owned());
        if elements.iter().any(|e| e.kind == "parenthetical") {
            chain_after.push("parenthetical".to_owned());
        }
        let mut action_continues = false;
        let mut action_ends = false;
        for i in 1..lines.len() {
            if lines[i - 1].mark == Some(Mark::Action) {
                if lines[i].mark == Some(Mark::Dialogue) {
                    action_continues = true;
                }
                if lines[i].mark == Some(Mark::Narration) {
                    action_ends = true;
                }
            }
        }
        if elements.iter().any(|e| e.kind == "action") {
            if action_continues && !action_ends {
                chain_after.push("action".to_owned());
            } else {
                run_ends_at.push("action".to_owned());
            }
        }
    }

    if has_chain && !elements.iter().any(|e| e.kind == "dialogue") {
        elements.push(DialectElement {
            kind: "dialogue".to_owned(),
            nature: ElementNature::Narrative,
            source: None,
            emitted: None,
            malformed: Vec::new(),
        });
    }

    // Prefer the shipped preset when it can carry the result: every
    // learned shape is affix sugar and the chain is the preset's own. Then
    // the dialect IS the resolution of the table the editor will write, by
    // construction — no projection to verify, nothing to lose in a file.
    let action_chains = chain_after.iter().any(|k| k == "action");
    let preset_fits =
        has_chain && shapes.iter().all(|(_, s)| s.how == How::Affix) && !action_chains;
    let explicit = || DialogueDialect {
        version: 1,
        name: "project".to_owned(),
        elements: elements.clone(),
        chain: if has_chain {
            vec![ChainRule {
                after: chain_after.clone(),
                is: vec!["narrative".to_owned()],
                becomes: "dialogue".to_owned(),
                carry: vec!["speaker".to_owned()],
                run_ends_at: run_ends_at.clone(),
            }]
        } else {
            Vec::new()
        },
        transitions: Vec::new(),
        templates: Templates::default(),
    };
    let dialect = if preset_fits {
        resolve_dialogue_config(&preset_table(&elements, run_ends_at.clone()), &|_: &str| {
            None
        })
        .unwrap_or_else(|_| explicit())
    } else {
        explicit()
    };

    // VERIFY: re-parse every line with the candidate and read the support
    // counts off the result. Whatever does not reproduce a mark is a
    // decision.
    let texts: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
    let parsed = ResolvedDialect::compile(&dialect)
        .map(|resolved| parse_source(&resolved, &texts))
        .unwrap_or_default();
    let kind_at = |i: usize| -> Option<&str> { parsed.get(i).and_then(|l| l.kind.as_deref()) };

    for (kind, shape) in &shapes {
        let Some(mark) = Mark::of_kind(kind) else {
            continue;
        };
        let positives = idx(mark);
        let support: Vec<usize> = positives
            .iter()
            .copied()
            .filter(|&i| kind_at(i) == Some(kind.as_str()))
            .collect();
        learned.push(Learned {
            id: format!("{}-shape", mark.as_str()),
            sentence: shape.sentence.clone(),
            support: support.clone(),
            total: positives.len(),
        });
        for extra in &shape.extras {
            learned.push(Learned {
                id: format!("{}-extra-{}", mark.as_str(), learned.len()),
                sentence: extra.clone(),
                support: support.clone(),
                total: positives.len(),
            });
        }
        let missed: Vec<usize> = positives
            .iter()
            .copied()
            .filter(|&i| kind_at(i) != Some(kind.as_str()))
            .collect();
        if !missed.is_empty() {
            let n = missed.len();
            decisions.push(Decision {
                id: format!("{}-unexplained", mark.as_str()),
                message: format!(
                    "{} marked {} but {} not fit the rule the other {} lines taught.",
                    plural(n, "This line is", "These lines are"),
                    mark.as_str(),
                    plural(n, "does", "do"),
                    mark.as_str(),
                ),
                lines: missed,
            });
        }
    }

    if has_chain {
        // Support is every line the chain claimed that the author did not
        // contradict: marked dialogue, or left unmarked.
        let support: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(i, l)| {
                (l.mark == Some(Mark::Dialogue) || l.mark.is_none())
                    && kind_at(*i) == Some("dialogue")
            })
            .map(|(i, _)| i)
            .collect();
        let mut enders: Vec<&str> = Vec::new();
        if run_ends_at.iter().any(|k| k == "action") {
            enders.push("an action line");
        }
        enders.push("the next cue");
        enders.push("the choices");
        let last = enders.pop().unwrap_or("the choices");
        let missed_dialogue: Vec<usize> = dialogue_idx
            .iter()
            .copied()
            .filter(|&i| kind_at(i) != Some("dialogue"))
            .collect();
        let total = support.len() + missed_dialogue.len();
        learned.push(Learned {
            id: "run".to_owned(),
            sentence: format!(
                "Lines after a cue belong to that speaker until {} or {last}.",
                enders.join(", ")
            ),
            support: support.clone(),
            total,
        });
        if action_chains {
            learned.push(Learned {
                id: "run-through-action".to_owned(),
                sentence: "An action line does not end the speaker's turn — the lines after it are still theirs.".to_owned(),
                support: support.clone(),
                total,
            });
        }
        if !missed_dialogue.is_empty() {
            let n = missed_dialogue.len();
            decisions.push(Decision {
                id: "dialogue-unexplained".to_owned(),
                message: format!(
                    "{} marked dialogue but nothing before {} names a speaker.",
                    plural(n, "This line is", "These lines are"),
                    plural(n, "it", "them"),
                ),
                lines: missed_dialogue,
            });
        }
        let narration_as_dialogue: Vec<usize> = idx(Mark::Narration)
            .into_iter()
            .filter(|&i| kind_at(i) == Some("dialogue"))
            .collect();
        if !narration_as_dialogue.is_empty() {
            let n = narration_as_dialogue.len();
            decisions.push(Decision {
                id: "narration-after-cue".to_owned(),
                message: format!(
                    "{} marked narration but {} a speaker's lines with nothing in between — the studio cannot tell {} from more speech. Put an action line before {}, or mark {} as dialogue.",
                    plural(n, "This line is", "These lines are"),
                    plural(n, "follows", "follow"),
                    plural(n, "it", "them"),
                    plural(n, "it", "them"),
                    plural(n, "it", "them"),
                ),
                lines: narration_as_dialogue,
            });
        }
    }
    // Narration lines that a shape swallowed.
    let stolen: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(i, l)| {
            l.mark == Some(Mark::Narration) && kind_at(*i).is_some_and(|k| k != "dialogue")
        })
        .map(|(i, _)| i)
        .collect();
    if !stolen.is_empty() {
        let n = stolen.len();
        decisions.push(Decision {
            id: "narration-shaped".to_owned(),
            message: format!(
                "{} marked narration but {} a rule above.",
                plural(n, "This line is", "These lines are"),
                plural(n, "matches", "match"),
            ),
            lines: stolen,
        });
    }

    Inference {
        dialect: Some(dialect),
        learned,
        decisions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_ir::dialect::validate;

    /// Golden corpus — `packages/dialect/src/__tests__/infer-corpus.ts`,
    /// line for line. A case added to one is added to the other.
    struct Case {
        id: &'static str,
        lines: Vec<MarkedLine>,
        /// Substrings each learned sentence must contain, in order.
        learned: &'static [&'static str],
        /// Decision ids expected, in order.
        decisions: &'static [&'static str],
        /// Element kinds expected on the dialect, sorted; `None` = no
        /// dialect.
        kinds: Option<&'static [&'static str]>,
        /// Whether `to_dialogue_config` must find a table form.
        table_form: bool,
    }

    fn l(text: &str, mark: Option<Mark>) -> MarkedLine {
        MarkedLine {
            text: text.to_owned(),
            mark,
            ..MarkedLine::default()
        }
    }

    fn tagged(text: &str, mark: Option<Mark>, tags: &[&str]) -> MarkedLine {
        MarkedLine {
            tags: tags.iter().map(|t| (*t).to_owned()).collect(),
            ..l(text, mark)
        }
    }

    fn choice(text: &str, mark: Option<Mark>) -> MarkedLine {
        MarkedLine {
            origin: Origin::Choice,
            ..l(text, mark)
        }
    }

    use Mark::{Action, Cue, Dialogue, Narration, Parenthetical};

    #[expect(
        clippy::too_many_lines,
        reason = "the golden corpus, one case per entry"
    )]
    fn corpus() -> Vec<Case> {
        vec![
            Case {
                id: "at-cue glued, action ends the turn (the canvas sample)",
                lines: vec![
                    l("@MARA: <>", Some(Cue)),
                    l("We don't have until morning.", Some(Dialogue)),
                    l("Not even close.", Some(Dialogue)),
                    l("> She sets the lantern down.", Some(Action)),
                    l("The lantern gutters.", Some(Narration)),
                    l("@JUNO: <>", Some(Cue)),
                    l("Then we go now.", Some(Dialogue)),
                ],
                learned: &[
                    "starts with \u{201c}@\u{201d} and ends with \u{201c}: <>\u{201d} is a cue",
                    "\u{201c}<>\u{201d} at the end attaches",
                    "starts with \u{201c}>\u{201d} is an action line",
                    "until an action line, the next cue or the choices",
                ],
                decisions: &[],
                kinds: Some(&["action", "character", "dialogue", "parenthetical"]),
                table_form: true,
            },
            Case {
                id: "action does not end the turn when dialogue is marked after it",
                lines: vec![
                    l("@MARA: <>", Some(Cue)),
                    l("Wait.", Some(Dialogue)),
                    l("> She listens.", Some(Action)),
                    l("Nothing. Go.", Some(Dialogue)),
                ],
                learned: &[
                    "is a cue",
                    "attaches",
                    "is an action line",
                    "until the next cue or the choices",
                    "does not end the speaker's turn",
                ],
                decisions: &[],
                kinds: Some(&["action", "character", "dialogue"]),
                table_form: false,
            },
            Case {
                id: "ink docs: `Name: line`, double space, inside choice text",
                lines: vec![
                    choice("Lisa: Where did he go?", Some(Cue)),
                    l("Joe:  I think he jumped over the garden fence.", Some(Cue)),
                    choice("Lisa: Let's take a look.", Some(Cue)),
                    l("The fence was higher than it looked.", Some(Narration)),
                ],
                learned: &["starts with a name and a colon, like \u{201c}Lisa:\u{201d}"],
                decisions: &[],
                kinds: Some(&["character"]),
                table_form: false,
            },
            Case {
                id: "ink docs: a colon mid-sentence in narration is a decision, never a rule",
                lines: vec![
                    l("Lisa: Where did he go?", Some(Cue)),
                    l("Joe: Over the fence.", Some(Cue)),
                    l("Warning: the bridge is out.", Some(Narration)),
                ],
                learned: &[],
                decisions: &["cue-ambiguous"],
                kinds: Some(&[]),
                table_form: false,
            },
            Case {
                id: "ink docs: tags never become part of a shape",
                lines: vec![
                    tagged(
                        "Passepartout: Really, Monsieur.",
                        Some(Cue),
                        &["surly", "really_monsieur.ogg"],
                    ),
                    l("Fogg: Quite.", Some(Cue)),
                    tagged("The clock struck.", Some(Narration), &["chime"]),
                ],
                learned: &["starts with a name and a colon"],
                decisions: &[],
                kinds: Some(&["character"]),
                table_form: false,
            },
            Case {
                id: "ink docs: a speaker carried in a tag is a decision this editor cannot express",
                lines: vec![
                    tagged("Really, Monsieur.", Some(Cue), &["speaker: Passepartout"]),
                    tagged("Quite.", Some(Cue), &["speaker: Fogg"]),
                    l("The clock struck.", Some(Narration)),
                ],
                learned: &[],
                decisions: &["cue-no-shape"],
                kinds: Some(&[]),
                table_form: false,
            },
            Case {
                id: "ink docs: quoted prose with attribution teaches no cue rule",
                lines: vec![
                    l("\"What's that?\" my master asked.", Some(Narration)),
                    l("\"I am somewhat tired,\" I repeated.", Some(Narration)),
                    l(
                        "\"Really,\" he responded. \"How deleterious.\"",
                        Some(Narration),
                    ),
                ],
                learned: &[],
                decisions: &["nothing-marked"],
                kinds: None,
                table_form: false,
            },
            Case {
                id: "quoted prose marked as cues has no learnable shape (the naive-prefix trap)",
                lines: vec![
                    l("\"What's that?\" my master asked.", Some(Cue)),
                    l("\"Quite well,\" he replied.", Some(Cue)),
                    l("He looked away.", Some(Narration)),
                ],
                learned: &[],
                decisions: &["cue-no-shape"],
                kinds: Some(&[]),
                table_form: false,
            },
            Case {
                id: "screenplay: NAME on its own line with a parenthetical under it",
                lines: vec![
                    l("MARA", Some(Cue)),
                    l("(quietly)", Some(Parenthetical)),
                    l("We don't have until morning.", Some(Dialogue)),
                    l("JUNO", Some(Cue)),
                    l("Then we go now.", Some(Dialogue)),
                ],
                learned: &[
                    "in capitals on its own, like \u{201c}MARA\u{201d}",
                    "starts with \u{201c}(\u{201d} and ends with \u{201c})\u{201d} is a parenthetical",
                    "until the next cue or the choices",
                ],
                decisions: &[],
                kinds: Some(&["character", "dialogue", "parenthetical"]),
                table_form: false,
            },
            Case {
                id: "narration right after a speaker's lines is the author's call",
                lines: vec![
                    l("@MARA: <>", Some(Cue)),
                    l("Wait.", Some(Dialogue)),
                    l("The wind picked up.", Some(Narration)),
                ],
                learned: &["is a cue", "attaches", "until the next cue or the choices"],
                decisions: &["narration-after-cue"],
                kinds: Some(&["character", "dialogue", "parenthetical"]),
                table_form: true,
            },
            Case {
                id: "dialogue marked with no cue anywhere",
                lines: vec![l("Wait.", Some(Dialogue)), l("Go.", Some(Dialogue))],
                learned: &[],
                decisions: &["dialogue-without-cue"],
                kinds: Some(&[]),
                table_form: false,
            },
            Case {
                id: "unmarked lines are checked, not taught from",
                lines: vec![
                    l("@MARA: <>", Some(Cue)),
                    l("We don't have until morning.", None),
                    l("Not even close.", None),
                    l("> She sets the lantern down.", Some(Action)),
                ],
                learned: &[
                    "is a cue",
                    "attaches",
                    "is an action line",
                    "until an action line, the next cue or the choices",
                ],
                decisions: &[],
                kinds: Some(&["action", "character", "dialogue", "parenthetical"]),
                table_form: true,
            },
        ]
    }

    #[test]
    fn the_golden_corpus_holds() {
        for c in corpus() {
            let r = infer_dialect(&c.lines);
            let sentences: Vec<&str> = r.learned.iter().map(|l| l.sentence.as_str()).collect();
            for (i, needle) in c.learned.iter().enumerate() {
                assert!(
                    sentences.get(i).is_some_and(|s| s.contains(needle)),
                    "{}: learned[{i}] of {sentences:?} should contain {needle:?}",
                    c.id
                );
            }
            assert_eq!(sentences.len(), c.learned.len(), "{}: {sentences:?}", c.id);
            let ids: Vec<&str> = r.decisions.iter().map(|d| d.id.as_str()).collect();
            assert_eq!(ids, c.decisions, "{}", c.id);
            let Some(kinds) = c.kinds else {
                assert!(r.dialect.is_none(), "{}", c.id);
                continue;
            };
            let d = r.dialect.as_ref().expect(c.id);
            let mut got: Vec<&str> = d.elements.iter().map(|e| e.kind.as_str()).collect();
            got.sort_unstable();
            assert_eq!(got, kinds, "{}", c.id);
            assert!(
                validate(d).is_ok(),
                "{}: the proposed dialect must validate: {:?}",
                c.id,
                validate(d).err()
            );
            assert_eq!(to_dialogue_config(d).is_some(), c.table_form, "{}", c.id);
        }
    }

    #[test]
    fn support_counts_are_reparse_results() {
        let c = corpus().remove(0);
        let r = infer_dialect(&c.lines);
        let d = r.dialect.as_ref().expect(c.id);
        let resolved = ResolvedDialect::compile(d).expect("compiles");
        let texts: Vec<&str> = c.lines.iter().map(|l| l.text.as_str()).collect();
        let parsed = parse_source(&resolved, &texts);
        for learned in &r.learned {
            assert!(!learned.support.is_empty());
            assert_eq!(learned.support.len(), learned.total);
            for &i in &learned.support {
                let expect = match c.lines[i].mark {
                    Some(Mark::Cue) => Some("character"),
                    Some(Mark::Narration) | None => None,
                    Some(m) => m.kind(),
                };
                assert_eq!(parsed[i].kind.as_deref(), expect, "line {i}");
            }
        }
    }

    #[test]
    fn the_ambiguous_colon_decision_names_the_offending_line() {
        let c = corpus()
            .into_iter()
            .find(|c| c.id.contains("colon mid-sentence"))
            .expect("in the corpus");
        let r = infer_dialect(&c.lines);
        assert_eq!(r.decisions[0].lines, [2]);
        assert!(r.decisions[0].message.contains("marked differently"));
    }

    #[test]
    fn the_source_parser_chains_carries_and_breaks() {
        let dialect = brink_ir::dialect::at_cue_preset();
        let resolved = ResolvedDialect::compile(&dialect).expect("compiles");
        let parsed = parse_source(
            &resolved,
            &[
                "@Mara:<>",
                "We go now.",
                "-> DONE",
                "Still hers?",
                "",
                "Not after a blank.",
            ],
        );
        assert_eq!(parsed[0].kind.as_deref(), Some("character"));
        assert_eq!(parsed[0].attrs, [("speaker".to_owned(), "Mara".to_owned())]);
        assert_eq!(parsed[1].kind.as_deref(), Some("dialogue"));
        assert_eq!(parsed[1].attrs, [("speaker".to_owned(), "Mara".to_owned())]);
        assert_eq!(parsed[2].kind, None, "a divert never chains");
        assert_eq!(parsed[3].kind, None, "and it broke the chain");
        assert_eq!(parsed[5].kind, None);
        assert!(is_structural_line("- gather"));
        assert!(is_structural_line("->x"));
        assert!(!is_structural_line("Plain prose."));
    }

    #[test]
    fn the_emitted_parser_and_run_rule_fold_a_turn() {
        let dialect = resolve_dialogue_config(
            &DialogueConfig {
                preset: Some("at-cue".to_owned()),
                run_ends_at: vec!["action".to_owned(), "choices".to_owned()],
                elements: vec![DialogueElementConfig {
                    kind: "action".to_owned(),
                    prefix: Some("> ".to_owned()),
                    ..DialogueElementConfig::default()
                }],
                file: None,
            },
            &|_: &str| None,
        )
        .expect("resolves");
        let parser = EmittedParser::compile(&dialect).expect("compiles");
        let segs = parser.parse_emitted("@Mara: (quietly) We go now.");
        assert_eq!(segs.len(), 3, "{segs:?}");
        assert_eq!(segs[0].kind.as_deref(), Some("character"));
        assert_eq!(segs[0].content.as_deref(), Some("Mara"));
        assert_eq!(segs[1].kind.as_deref(), Some("parenthetical"));
        assert_eq!(segs[2].kind, None);
        assert_eq!(
            parser.parse_emitted("(aside) alone")[0].kind,
            None,
            "a non-reserved shape never opens a line"
        );
        let lines: Vec<EmittedLine> = [
            "@Mara: We go now.",
            "Not even close.",
            "> She sets the lantern down.",
            "The lantern gutters.",
            "@Juno: Then we go.",
        ]
        .iter()
        .map(|t| EmittedLine {
            segments: parser.parse_emitted(t),
            boundary: false,
        })
        .collect();
        let runs = runs_of(&lines, &dialect);
        assert_eq!(runs.len(), 4, "{runs:?}");
        assert_eq!(runs[0].lines, [0, 1]);
        assert_eq!(
            runs[0].attrs.get("speaker").map(String::as_str),
            Some("Mara")
        );
        assert_eq!(runs[1].kind.as_deref(), Some("action"));
        assert_eq!(runs[2].kind, None);
        assert_eq!(runs[3].lines, [4]);
    }

    #[test]
    fn the_table_projection_round_trips_and_refuses_what_it_cannot_say() {
        let config = DialogueConfig {
            preset: Some("at-cue".to_owned()),
            run_ends_at: vec!["action".to_owned(), "choices".to_owned()],
            elements: vec![DialogueElementConfig {
                kind: "action".to_owned(),
                prefix: Some("> ".to_owned()),
                ..DialogueElementConfig::default()
            }],
            file: None,
        };
        let d = resolve_dialogue_config(&config, &|_: &str| None).expect("resolves");
        assert_eq!(
            d.elements
                .iter()
                .map(|e| e.kind.as_str())
                .collect::<Vec<_>>(),
            ["character", "parenthetical", "dialogue", "action"]
        );
        assert_eq!(to_dialogue_config(&d), Some(config));
        let plain = resolve_dialogue_config(
            &DialogueConfig {
                preset: Some("at-cue".to_owned()),
                ..DialogueConfig::default()
            },
            &|_: &str| None,
        )
        .expect("resolves");
        assert_eq!(
            to_dialogue_config(&plain),
            Some(DialogueConfig {
                preset: Some("at-cue".to_owned()),
                ..DialogueConfig::default()
            })
        );
        let mut widened = plain.clone();
        widened.chain[0].after.push("action".to_owned());
        widened.elements.push(affix_element(
            "action",
            ElementNature::Narrative,
            AffixShape {
                prefix: Some(">".to_owned()),
                suffix: None,
                glued: false,
                content_role: "content".to_owned(),
            },
        ));
        assert_eq!(to_dialogue_config(&widened), None);
    }
}
