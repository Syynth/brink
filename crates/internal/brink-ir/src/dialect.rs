//! Dialogue dialect — a versioned, pure-JSON, authoring-time/tooling schema
//! describing a project's dialogue-line conventions (cues, parentheticals,
//! dialogue chains) so the editor can classify and decorate lines without
//! hardcoding any one convention. See `docs/dialect-spec.md` (#368).
//!
//! **Scope ruling (the big one):** the dialect is an authoring-time/tooling
//! artifact — it is **never** runtime-delivered. It has no `.inkb` embedding
//! and no project-file home in v1 (mount-time config only), mirroring the
//! host-capability-manifest charter (see `host_manifest.rs`). The `emitted`
//! facet exists so the *editor* can model what the runtime will see; it does
//! not instruct any runtime.
//!
//! The artifact splits into two sections with different owners
//! (separate-concerns-by-ownership):
//! - **Semantics core** (`elements`, `chain`) — the durable truth, and the
//!   future host-capability-manifest section. This is what a future runtime
//!   consumer (a game engine plugin) would read.
//! - **Editor overlay** (`transitions`, per-element `template`) — pure editor
//!   UX. Never travels beyond tooling.
//!
//! `Default` reproduces today's hardcoded `@Name:<>` at-cue screenplay
//! behavior byte-for-byte (see `element-type.ts`'s screenplay post-pass and
//! `screenplay.ts`'s `CHAR_SUFFIX_LEN`/`GLUE_LEN`/`characterName()`).

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

// ─── The artifact ────────────────────────────────────────────────────

/// A versioned, pure-JSON dialogue dialect. No functions, no `RegExp`
/// objects — patterns are strings in the portable-regex subset (JS `RegExp`
/// ∩ Rust `regex`: named groups yes, lookaround/backreferences no).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DialogueDialect {
    /// Schema version. Only `1` is defined.
    pub version: u32,
    /// Human-readable dialect name (e.g. `"at-cue"`).
    #[serde(default)]
    pub name: String,
    /// Element declarations, in **classification precedence order**
    /// (determinism rule: this is a `Vec`, never a map, so JSON round-trips
    /// through serde preserve author-controlled precedence).
    #[serde(default)]
    pub elements: Vec<DialectElement>,
    /// Chain rules: "narrative immediately after X becomes Y". Blank lines
    /// always break a chain — this is not configurable in v1 (see spec
    /// decision 9).
    #[serde(default)]
    pub chain: Vec<ChainRule>,
    /// Editor-overlay transition rows — **never travels beyond tooling**.
    /// Rows are contributed only for kinds the dialect declares; dialect rows
    /// resolve before the built-in structural weave rows (overlay, not
    /// replace).
    #[serde(default)]
    pub transitions: Vec<TransitionRow>,
    /// Editor-overlay templates (picker key, blank-tab behavior, labels) —
    /// never travels beyond tooling.
    #[serde(default)]
    pub templates: Templates,
}

impl Default for DialogueDialect {
    /// The `@Name:<>` at-cue preset — reproduces today's hardcoded
    /// screenplay behavior exactly (`element-type.ts` post-pass,
    /// `screenplay.ts` sigil geometry).
    fn default() -> Self {
        at_cue_preset()
    }
}

/// One declared element kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DialectElement {
    /// Open string taxonomy kind (e.g. `"character"`). CSS class derives as
    /// `brink-<kind>`.
    pub kind: String,
    /// The 3-way nature: `narrative` (prose), `machinery` (structural-ish but
    /// not reserved), or `structural` (joins neither fold-run type).
    pub nature: ElementNature,
    /// The source-side shape. Absent for chain-only kinds (e.g. `dialogue`,
    /// which is never matched directly — it's produced only by a chain
    /// rule). When absent, the pattern-less-kind contract applies: content is
    /// the whole trimmed line, and `convert`/format resolves to a strip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceShape>,
    /// The post-glue emitted shape the runtime sees out of `continue_line()`
    /// output. Positionally constrained (see [`EmittedShape`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emitted: Option<EmittedShape>,
    /// Near-miss diagnostics: patterns that almost match this kind but don't
    /// quite, paired with a diagnostic message + severity.
    #[serde(default)]
    pub malformed: Vec<MalformedRule>,
}

/// The 3-way element nature (ruling: 3-way, not 2-way).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ElementNature {
    /// Prose the player reads (character text, dialogue, plain narrative).
    Narrative,
    /// Structural-ish machinery that is still a dialect concern (not a fold
    /// boundary of its own).
    Machinery,
    /// Joins neither #365 fold-run type — reserved for elements like scene
    /// headings that stand alone.
    Structural,
}

// ─── Source shape: pattern form + affix sugar ─────────────────────────

/// How an element is recognized and produced in source text. Either a
/// portable-regex `pattern` form (the general representation, and the only
/// thing interpreters execute) or an `affix` sugar form that **compiles
/// mechanically to the pattern form** in [`compile_affix`] — one derivation
/// site, never prose-respecified per consumer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SourceShape {
    /// The general portable-regex representation.
    Pattern(PatternShape),
    /// Sugar: `{ prefix, suffix, glued, contentRole }`. Never interpreted
    /// directly — always compiled via [`compile_affix`] before use.
    Affix(AffixShape),
}

impl SourceShape {
    /// Resolve this shape to its canonical [`PatternShape`], compiling affix
    /// sugar if necessary. This is the *only* derivation site — classifiers,
    /// validators, and geometry computation all go through this.
    #[must_use]
    pub fn resolve(&self) -> PatternShape {
        match self {
            SourceShape::Pattern(p) => p.clone(),
            SourceShape::Affix(a) => compile_affix(a),
        }
    }
}

/// The portable-regex pattern form — the general representation every
/// interpreter (Rust today, TS/future engines later) executes directly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatternShape {
    /// Portable-regex pattern (JS `RegExp` ∩ Rust `regex` subset: named
    /// groups yes, lookaround/backreferences no), anchored `^...$` against
    /// the trimmed line.
    pub pattern: String,
    /// Which named group is the editable content. Drives `content_span`
    /// geometry (markup/inline-decoration scoping) and the `data-*` attrs
    /// derived from classification — this is the "what region of the line is
    /// content" answer, which for a kind like `parenthetical` legitimately
    /// includes wrapping punctuation that stays visible on the line (#406).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_group: Option<String>,
    /// Which named group's captured value fills `template`'s placeholder for
    /// convert/strip round-trips (`ResolvedDialect::convertible_shapes`).
    /// Defaults to `content_group` when absent — additive, byte-identical
    /// for every dialect that doesn't set it (#406). Exists because a kind
    /// can need a *different* answer to "what region is content"
    /// (`content_group`, wrap-inclusive for `parenthetical` so the parens
    /// stay part of the editable/markup-scoped region) than to "what value
    /// round-trips through `template`" (`template_group`, wrap-EXCLUSIVE —
    /// the literal `(`/`)` live in `template` itself, matching how every
    /// other convert/strip consumer — `DEFAULT_CONVERTIBLE_SHAPES`, the
    /// built-in `convertToParenthetical`/`stripToNarrative` actions —
    /// already treats "Parenthetical content" as the bare text between the
    /// parens). Never emitted as a `data-*` attr and never hidden — see
    /// `build_match`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_group: Option<String>,
    /// Named groups whose matched span is hidden geometry — ALL editor
    /// decorations/atomic-ranges/edit-guards derive from these match
    /// indices, computed once at classification time.
    #[serde(default)]
    pub hidden: Vec<String>,
    /// Template string for insertion/conversion/format (e.g. `"@${speaker}:<>"`).
    /// Validated to round-trip against `pattern` (see
    /// [`validate_template_roundtrip`]).
    pub template: String,
}

/// Affix sugar: a content slot wrapped in literal prefix/suffix text. The
/// common case — every known convention today is affix-shaped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AffixShape {
    /// Literal prefix before the content (e.g. `"@"`). Hidden by construction.
    #[serde(default)]
    pub prefix: Option<String>,
    /// Literal suffix after the content (e.g. `":"`, or `":<>"` when glued).
    #[serde(default)]
    pub suffix: Option<String>,
    /// Whether the suffix's glue (`<>`) is appended and always hidden.
    #[serde(default)]
    pub glued: bool,
    /// The semantic role of the content slot (drives `contentGroup` naming
    /// and `data-*` attribute naming downstream).
    #[serde(default = "default_content_role")]
    pub content_role: String,
}

fn default_content_role() -> String {
    "content".to_owned()
}

/// The glue suffix appended when `glued: true` (matches `screenplay.ts`'s
/// `GLUE_LEN` = 2).
const GLUE: &str = "<>";

/// Compile affix sugar to the canonical pattern form — the ONE derivation
/// site (spec requirement: never prose-respecified per consumer).
///
/// Derivation rules:
/// - `prefix` (if present) becomes a hidden, anchored literal group `lead`.
/// - `suffix` (if present), with `<>` appended when `glued`, becomes a
///   hidden, anchored literal group `tail`.
/// - The content slot in between becomes the named group `content_role`,
///   matching everything up to the suffix (non-greedy is unnecessary since
///   the suffix is anchored at the end).
/// - `template` is rebuilt as `${prefix}${content}${suffix}` with the
///   content role interpolated by name.
#[must_use]
pub fn compile_affix(affix: &AffixShape) -> PatternShape {
    use std::fmt::Write as _;

    let prefix = affix.prefix.as_deref().unwrap_or("");
    let mut suffix = affix.suffix.as_deref().unwrap_or("").to_owned();
    if affix.glued {
        suffix.push_str(GLUE);
    }
    let role = affix.content_role.as_str();

    let mut pattern = String::from("^");
    let mut hidden = Vec::new();
    let mut template = String::new();

    if !prefix.is_empty() {
        pattern.push_str("(?<lead>");
        pattern.push_str(&regex_escape_literal(prefix));
        pattern.push(')');
        hidden.push("lead".to_owned());
        template.push_str(prefix);
    }

    let _ = write!(pattern, "(?<{role}>[^");
    // Content stops before the first character of the suffix (or end of
    // line when there's no suffix), matching the affix model: the content
    // slot is "everything up to the reserved suffix".
    if let Some(first) = suffix.chars().next() {
        pattern.push_str(&regex_escape_class_char(first));
    }
    pattern.push_str("]*)");
    let _ = write!(template, "${{{role}}}");

    if !suffix.is_empty() {
        pattern.push_str("(?<tail>");
        pattern.push_str(&regex_escape_literal(&suffix));
        pattern.push(')');
        hidden.push("tail".to_owned());
        template.push_str(&suffix);
    }
    pattern.push('$');

    PatternShape {
        pattern,
        content_group: Some(role.to_owned()),
        template_group: None,
        hidden,
        template,
    }
}

/// Escape a literal string for inclusion in a regex pattern (outside a
/// character class).
fn regex_escape_literal(s: &str) -> String {
    regex::escape(s)
}

/// Escape a single character for inclusion inside a `[...]` character class.
fn regex_escape_class_char(c: char) -> String {
    if matches!(c, ']' | '\\' | '^' | '-') {
        format!("\\{c}")
    } else {
        c.to_string()
    }
}

// ─── Emitted shape (runtime-facing, positionally constrained) ────────

/// The post-glue shape the runtime sees out of `continue_line()` output.
/// **Positionally constrained** (mandatory hardening from the design round):
/// non-reserved-prefix shapes (e.g. a parenthetical) peel only after a
/// reserved-prefix segment (e.g. a cue) — never from arbitrary prose. This
/// is what makes `@channel: hello` prose and `(aside)` prose fail to parse
/// as cue/parenthetical.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmittedShape {
    /// Portable-regex pattern matched against the emitted (post-glue)
    /// segment.
    pub pattern: String,
    /// Which named group is the editable/extractable content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_group: Option<String>,
    /// Whether this kind's prefix is "reserved" — i.e. a sequence that
    /// cannot occur at the start of ordinary prose (e.g. `@` for cues).
    /// Non-reserved kinds (e.g. a parenthetical's `(`) may only be parsed
    /// as a continuation segment immediately following a reserved-prefix
    /// segment, never as the first segment of a line.
    #[serde(default)]
    pub reserved_prefix: bool,
}

// ─── Chain rules ───────────────────────────────────────────────────────

/// "Narrative immediately after one of `after` becomes `becomes`." Blank
/// lines always break the chain (not configurable in v1 — spec decision 9).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChainRule {
    /// Kinds (or `"narrative"`) that, when immediately preceding a narrative
    /// line, trigger the chain.
    pub after: Vec<String>,
    /// The predecessor kinds this rule fires from must produce a line of
    /// this kind for the rule to apply (v1: always `["narrative"]"`, kept as
    /// a field for forward compatibility).
    #[serde(default = "default_chain_is")]
    pub is: Vec<String>,
    /// The kind the matched line becomes.
    pub becomes: String,
    /// Named groups from the triggering predecessor to carry forward onto
    /// the whole chained run as `data-*` attributes (e.g. `["speaker"]` →
    /// `data-speaker` on every line in the run).
    #[serde(default)]
    pub carry: Vec<String>,
}

fn default_chain_is() -> Vec<String> {
    vec!["narrative".to_owned()]
}

// ─── Editor overlay: transitions + templates ──────────────────────────

/// One Tab/Enter/Shift-Tab transition row, contributed by the dialect for a
/// kind it declares. Structural transition rows stay interpreter-owned;
/// dialect rows are an **overlay**, resolved before the built-in weave
/// table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransitionRow {
    /// The kind this row applies to (must be declared, or a reserved
    /// structural kind — see [`validate`]).
    pub on: String,
    /// The triggering key (`"Tab"`, `"Enter"`, `"Shift-Tab"`, …).
    pub key: String,
    /// Whether this row requires the line to have content.
    #[serde(default)]
    pub has_content: Option<bool>,
    /// The action to take: `{ "convert": "<kind>" }`, `{ "newline": true }`,
    /// `"strip"`, `"clear"`, or `"trap"`.
    pub action: TransitionAction,
    /// Editor-facing hint text (status bar, etc). Editor overlay only.
    #[serde(default)]
    pub hint: Option<String>,
}

/// A transition action. Tagged so serde errors are legible (untagged unions
/// over mixed string/object shapes were rejected during design review for
/// producing unusable errors).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum TransitionAction {
    /// Convert the current line to `kind`.
    Convert { kind: String },
    /// Insert a new sibling line.
    Newline,
    /// Strip all dialect sigils — plain narrative text.
    Strip,
    /// Clear the line's content.
    Clear,
    /// Swallow the key (no-op edit) — used to protect sigil regions.
    Trap,
}

/// Editor-overlay template metadata (picker labels, blank-tab behavior).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Templates {
    /// Per-kind picker entries: kind → (label, optional picker key, blank-tab
    /// flag). Kept as a `Vec` for determinism (ordered UI).
    #[serde(default)]
    pub entries: Vec<TemplateEntry>,
}

/// One picker/template entry for a declared kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateEntry {
    /// The kind this template is for.
    pub kind: String,
    /// Display label (e.g. `"Character cue"`).
    pub label: String,
    /// Optional picker key (keyboard shortcut / quick-pick id).
    #[serde(default)]
    pub picker_key: Option<String>,
    /// Whether pressing Tab on a blank line inserts this template.
    #[serde(default)]
    pub blank_tab: bool,
}

// ─── Succession-row wire conversion (issue #2115) ─────────────────────

/// See [`crate::ConventionsProjection::to_wire`]'s doc — these rows travel
/// to the wire **as data**, never interpreted by the compiler (§5 of
/// `docs/prose-dialect-spec.md`: "ignored by the compiler"). Conversion is
/// a plain field-for-field mirror, the same posture
/// `ConventionProjectionEntry::to_wire` already takes.
impl TransitionRow {
    #[must_use]
    pub fn to_wire(&self) -> brink_format::TransitionRowDef {
        brink_format::TransitionRowDef {
            on: self.on.clone(),
            key: self.key.clone(),
            has_content: self.has_content,
            action: self.action.to_wire(),
            hint: self.hint.clone(),
        }
    }
}

impl TransitionAction {
    #[must_use]
    pub fn to_wire(&self) -> brink_format::TransitionActionDef {
        match self {
            Self::Convert { kind } => {
                brink_format::TransitionActionDef::Convert { kind: kind.clone() }
            }
            Self::Newline => brink_format::TransitionActionDef::Newline,
            Self::Strip => brink_format::TransitionActionDef::Strip,
            Self::Clear => brink_format::TransitionActionDef::Clear,
            Self::Trap => brink_format::TransitionActionDef::Trap,
        }
    }
}

impl Templates {
    #[must_use]
    pub fn to_wire(&self) -> brink_format::TemplatesDef {
        brink_format::TemplatesDef {
            entries: self.entries.iter().map(TemplateEntry::to_wire).collect(),
        }
    }
}

impl TemplateEntry {
    #[must_use]
    pub fn to_wire(&self) -> brink_format::TemplateEntryDef {
        brink_format::TemplateEntryDef {
            kind: self.kind.clone(),
            label: self.label.clone(),
            picker_key: self.picker_key.clone(),
            blank_tab: self.blank_tab,
        }
    }
}

// ─── Malformed diagnostics ─────────────────────────────────────────────

/// A near-miss diagnostic: a pattern that almost matches a kind but doesn't
/// quite, paired with a message and severity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MalformedRule {
    /// Portable-regex pattern identifying the near-miss.
    pub pattern: String,
    /// Diagnostic message shown to the author.
    pub message: String,
    /// Severity (`"error"`, `"warning"`, `"info"`).
    #[serde(default = "default_severity")]
    pub severity: String,
}

fn default_severity() -> String {
    "warning".to_owned()
}

// ─── Reserved structural kinds ─────────────────────────────────────────

/// Kinds reserved by the interpreter's built-in structural taxonomy
/// (`LineElement` in `brink-ide`). Chain/transition rows may reference these
/// without the dialect declaring them itself (validation rule: "declared OR
/// reserved-structural").
#[must_use]
pub fn reserved_structural_kinds() -> &'static [&'static str] {
    &[
        "knot_header",
        "stitch_header",
        "narrative",
        "choice",
        "choice_body",
        "gather",
        "divert",
        "logic",
        "var_decl",
        "comment",
        "include",
        "external",
        "tag",
        "blank",
    ]
}

// ─── Validation ────────────────────────────────────────────────────────

/// A dialect validation failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DialectError {
    /// Unsupported schema version.
    #[error("unsupported dialect version {0} (only version 1 is defined)")]
    UnsupportedVersion(u32),
    /// A pattern uses a construct outside the portable-regex subset
    /// (lookaround or backreferences).
    #[error("kind '{kind}': pattern uses a non-portable construct: {reason}")]
    NonPortablePattern { kind: String, reason: String },
    /// A pattern failed to compile at all.
    #[error("kind '{kind}': pattern failed to compile: {reason}")]
    InvalidPattern { kind: String, reason: String },
    /// The `template` does not round-trip against `pattern` (re-matching the
    /// rendered template does not reproduce the same named-group spans).
    #[error("kind '{kind}': template '{template}' does not round-trip against its pattern")]
    TemplateRoundtripFailed { kind: String, template: String },
    /// A chain rule references a kind that is neither declared nor a
    /// reserved structural kind.
    #[error("chain rule references undeclared, non-structural kind '{0}'")]
    ChainUndeclaredKind(String),
    /// A transition row references a kind that is neither declared nor a
    /// reserved structural kind.
    #[error("transition row references undeclared, non-structural kind '{0}'")]
    TransitionUndeclaredKind(String),
    /// A `templates` entry references a kind that is neither declared nor a
    /// reserved structural kind.
    #[error("template entry references undeclared, non-structural kind '{0}'")]
    TemplateUndeclaredKind(String),
    /// Two elements declare the same kind.
    #[error("duplicate element kind '{0}'")]
    DuplicateKind(String),
    /// A `chain.becomes` kind was never declared as an element (chain-only
    /// kinds must still appear in `elements` with no `source`).
    #[error("chain rule produces undeclared kind '{0}' (add it to `elements` with no `source`)")]
    ChainBecomesUndeclared(String),
}

/// Validate a dialect. Checks:
/// - schema version,
/// - each `source.pattern` (after affix resolution) is in the portable
///   regex subset and compiles,
/// - `template` round-trips against `pattern`,
/// - chain/transition kinds are declared OR reserved-structural,
/// - `chain.becomes` kinds are declared as elements (possibly pattern-less),
/// - no duplicate kinds.
pub fn validate(dialect: &DialogueDialect) -> Result<(), Vec<DialectError>> {
    let mut errors = Vec::new();

    if dialect.version != 1 {
        errors.push(DialectError::UnsupportedVersion(dialect.version));
    }

    let mut seen_kinds = BTreeSet::new();
    let mut declared_kinds = BTreeSet::new();
    for el in &dialect.elements {
        if !seen_kinds.insert(el.kind.clone()) {
            errors.push(DialectError::DuplicateKind(el.kind.clone()));
        }
        declared_kinds.insert(el.kind.clone());

        if let Some(source) = &el.source {
            let resolved = source.resolve();
            if let Err(reason) = check_portable_pattern(&resolved.pattern) {
                errors.push(DialectError::NonPortablePattern {
                    kind: el.kind.clone(),
                    reason,
                });
                continue;
            }
            match regex::Regex::new(&resolved.pattern) {
                Ok(re) => {
                    if !validate_template_roundtrip(&re, &resolved) {
                        errors.push(DialectError::TemplateRoundtripFailed {
                            kind: el.kind.clone(),
                            template: resolved.template.clone(),
                        });
                    }
                }
                Err(e) => {
                    errors.push(DialectError::InvalidPattern {
                        kind: el.kind.clone(),
                        reason: e.to_string(),
                    });
                }
            }
        }
    }

    let reserved: BTreeSet<&str> = reserved_structural_kinds().iter().copied().collect();
    let is_known = |k: &str| declared_kinds.contains(k) || reserved.contains(k);

    for rule in &dialect.chain {
        for k in rule.after.iter().chain(rule.is.iter()) {
            if !is_known(k) {
                errors.push(DialectError::ChainUndeclaredKind(k.clone()));
            }
        }
        if !declared_kinds.contains(&rule.becomes) {
            errors.push(DialectError::ChainBecomesUndeclared(rule.becomes.clone()));
        }
    }

    errors.extend(validate_succession(
        &dialect.transitions,
        &dialect.templates,
        is_known,
    ));

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Validate succession rows (transitions + templates) against an arbitrary
/// set of known kinds (issue #2115). Unlike [`validate`]'s own chain check
/// (which is always checked against ONE `DialogueDialect`'s own `elements`),
/// this is reusable against any known-kind predicate — in particular a
/// [`crate::ConventionsProjection`]'s own convention kinds
/// (`entries[].name`), so `transitions`/`templates` can re-key off the
/// projection instead of carrying an independent element list. That
/// re-keying is exactly what `docs/decision-log.md`'s 2026-08-03 "Conventions
/// × the editor" entry rules for `DialogueDialect`'s surviving
/// `transitions`/`templates` fields: *"they re-key off convention kinds from
/// the projection instead of carrying their own element list"* — the same
/// divergence `set_dialect` already created between editor and compiler is
/// exactly what re-keying against one shared source avoids re-creating.
///
/// [`validate`] calls this with its own dialect's `is_known` closure, so a
/// `DialogueDialect`'s own transitions/templates are validated by exactly
/// this function too — one check, two callers, never two implementations
/// that could silently disagree.
#[must_use]
pub fn validate_succession(
    transitions: &[TransitionRow],
    templates: &Templates,
    is_known: impl Fn(&str) -> bool,
) -> Vec<DialectError> {
    let mut errors = Vec::new();
    for row in transitions {
        if !is_known(&row.on) {
            errors.push(DialectError::TransitionUndeclaredKind(row.on.clone()));
        }
        if let TransitionAction::Convert { kind } = &row.action
            && !is_known(kind)
        {
            errors.push(DialectError::TransitionUndeclaredKind(kind.clone()));
        }
    }
    for entry in &templates.entries {
        if !is_known(&entry.kind) {
            errors.push(DialectError::TemplateUndeclaredKind(entry.kind.clone()));
        }
    }
    errors
}

/// Reject portable-regex-subset violations: lookaround (`(?=`, `(?!`, `(?<=`,
/// `(?<!` when not a named group) and backreferences (`\1`, `\k<name>`).
/// Rust's `regex` crate cannot express these at all, so `Regex::new` would
/// already reject them — this pre-check exists to give a clear, spec-named
/// error rather than a raw regex-crate parse error, and to run identically
/// in the TS interpreter (which uses full JS `RegExp` and must reject the
/// same constructs explicitly).
fn check_portable_pattern(pattern: &str) -> Result<(), String> {
    let bytes = pattern.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            // Backreference: \1, \2, ... or \k<name>
            if let Some(&next) = bytes.get(i + 1) {
                if next.is_ascii_digit() && next != b'0' {
                    return Err("backreferences are not allowed".to_owned());
                }
                if next == b'k' && bytes.get(i + 2) == Some(&b'<') {
                    return Err("backreferences are not allowed".to_owned());
                }
            }
            i += 2;
            continue;
        }
        if bytes[i] == b'(' && bytes.get(i + 1) == Some(&b'?') {
            let rest = &pattern[i + 2..];
            if rest.starts_with('=') || rest.starts_with('!') {
                return Err("lookahead is not allowed".to_owned());
            }
            if rest.starts_with("<=") || rest.starts_with("<!") {
                return Err("lookbehind is not allowed".to_owned());
            }
        }
        i += 1;
    }
    Ok(())
}

/// Render the template with placeholder values, re-match it against the
/// pattern, and confirm the named groups round-trip.
///
/// The probe value must itself satisfy whatever the content group's own
/// sub-pattern requires (e.g. a parenthetical's `content` group is
/// `\([^)]*\)` — the captured text already includes the literal parens, so a
/// bare alphanumeric probe would never re-match). Rather than statically
/// deriving a value from the sub-pattern, this tries a small candidate set
/// covering the shapes real dialects use (bare text, parenthesized text,
/// bracketed text) and accepts the first that round-trips.
fn validate_template_roundtrip(re: &regex::Regex, shape: &PatternShape) -> bool {
    const CANDIDATES: &[&str] = &["PROBE", "(PROBE)", "[PROBE]"];
    CANDIDATES
        .iter()
        .any(|probe| roundtrips_with_probe(re, shape, probe))
}

fn roundtrips_with_probe(re: &regex::Regex, shape: &PatternShape, probe: &str) -> bool {
    let mut rendered = shape.template.clone();
    for name in re.capture_names().flatten() {
        rendered = rendered.replace(&format!("${{{name}}}"), probe);
    }
    // `template_group` (#406), when set, is the group whose captured value
    // the template's placeholder actually round-trips (e.g. a
    // wrap-inclusive `content_group` like `parenthetical`'s means the OUTER
    // group would never equal the bare probe — `template_group` names the
    // inner bare group the template literally wraps).
    let checked_group = shape
        .template_group
        .as_deref()
        .or(shape.content_group.as_deref());
    match re.captures(&rendered) {
        Some(caps) => {
            if let Some(checked_group) = checked_group {
                caps.name(checked_group)
                    .is_some_and(|m| m.as_str() == probe)
            } else {
                true
            }
        }
        None => false,
    }
}

// ─── Classification ─────────────────────────────────────────────────────

/// A resolved (compiled) dialect — patterns pre-compiled once, ready for
/// repeated classification. Building this is the only place regex
/// compilation happens; classifying a line never re-compiles.
pub struct ResolvedDialect {
    elements: Vec<ResolvedElement>,
    chain: Vec<ChainRule>,
}

struct ResolvedElement {
    decl: DialectElement,
    pattern: Option<(regex::Regex, PatternShape)>,
}

/// One classified dialect match on a line.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DialectMatch {
    /// The matched kind.
    pub kind: String,
    /// Captured named-group attributes (excluding hidden groups), sorted by
    /// name for determinism.
    pub attrs: Vec<(String, String)>,
    /// Hidden geometry: byte ranges (relative to the trimmed line's start
    /// within the full line, i.e. already offset by leading whitespace)
    /// that should never show a cursor / should render as a hidden
    /// decoration.
    pub hidden_spans: Vec<(u32, u32)>,
    /// The content region: the byte range (same offset basis as
    /// `hidden_spans`) of the editable content.
    pub content_span: Option<(u32, u32)>,
}

impl ResolvedDialect {
    /// Compile a dialect's patterns once. Returns an error if any element's
    /// pattern fails to compile (should not happen for a dialect that passed
    /// [`validate`]).
    pub fn compile(dialect: &DialogueDialect) -> Result<Self, DialectError> {
        let mut elements = Vec::with_capacity(dialect.elements.len());
        for decl in &dialect.elements {
            let pattern = match &decl.source {
                Some(source) => {
                    let resolved = source.resolve();
                    let re = regex::Regex::new(&resolved.pattern).map_err(|e| {
                        DialectError::InvalidPattern {
                            kind: decl.kind.clone(),
                            reason: e.to_string(),
                        }
                    })?;
                    Some((re, resolved))
                }
                None => None,
            };
            elements.push(ResolvedElement {
                decl: decl.clone(),
                pattern,
            });
        }
        Ok(Self {
            elements,
            chain: dialect.chain.clone(),
        })
    }

    /// Classify a single trimmed line against the declared elements, in
    /// declaration order (first match wins — the determinism/precedence
    /// rule). `leading_ws` is the byte length of the line's leading
    /// whitespace, used to offset spans back into full-line coordinates.
    /// Elements with no `source` (chain-only kinds) are never matched here —
    /// they can only be produced by a chain rule.
    #[must_use]
    pub fn classify(&self, trimmed: &str, leading_ws: u32) -> Option<DialectMatch> {
        for el in &self.elements {
            let Some((re, shape)) = &el.pattern else {
                continue;
            };
            if let Some(caps) = re.captures(trimmed) {
                return Some(build_match(&el.decl.kind, re, &caps, shape, leading_ws));
            }
        }
        None
    }

    /// Chain rules declared by this dialect.
    #[must_use]
    pub fn chain_rules(&self) -> &[ChainRule] {
        &self.chain
    }

    /// The declared [`ElementNature`] for a dialect kind (e.g. `"character"`,
    /// or a chain-produced kind like `"dialogue"`). `None` if `kind` isn't
    /// declared by this dialect (e.g. a built-in structural kind) — callers
    /// fall back to their own structural default in that case.
    ///
    /// Consumed by `brink-ide`'s fold-kind run computation (#365) so the
    /// machinery/narrative fold-run pass never re-derives nature from a
    /// hardcoded kind list — it asks the resolved dialect.
    #[must_use]
    pub fn nature_of(&self, kind: &str) -> Option<ElementNature> {
        self.elements
            .iter()
            .find(|el| el.decl.kind == kind)
            .map(|el| el.decl.nature)
    }

    /// Find the chain rule (if any) that fires when a narrative line follows
    /// a line of dialect-kind `prev_kind`.
    #[must_use]
    pub fn chain_rule_after(&self, prev_kind: &str) -> Option<&ChainRule> {
        self.chain
            .iter()
            .find(|r| r.after.iter().any(|k| k == prev_kind))
    }
}

/// Convert a regex match's byte span (within the trimmed line) to an
/// absolute-in-line `(start, end)` pair, offset by `leading_ws`. Source
/// lines are bounded well under `u32::MAX` bytes, so this truncation is not
/// reachable in practice.
#[expect(clippy::cast_possible_truncation)]
fn span_of(m: &regex::Match<'_>, leading_ws: u32) -> (u32, u32) {
    (leading_ws + m.start() as u32, leading_ws + m.end() as u32)
}

fn build_match(
    kind: &str,
    re: &regex::Regex,
    caps: &regex::Captures<'_>,
    shape: &PatternShape,
    leading_ws: u32,
) -> DialectMatch {
    let mut attrs = Vec::new();
    let mut hidden_spans = Vec::new();
    let mut content_span = None;
    let hidden: BTreeSet<&str> = shape.hidden.iter().map(String::as_str).collect();

    for hidden_name in &shape.hidden {
        if let Some(m) = caps.name(hidden_name) {
            hidden_spans.push(span_of(&m, leading_ws));
        }
    }

    if let Some(content_group) = &shape.content_group
        && let Some(m) = caps.name(content_group)
    {
        content_span = Some(span_of(&m, leading_ws));
    }

    // `template_group` (#406), when it names a DIFFERENT group than
    // `content_group`, is a template-fill-only helper group — it must not
    // leak into `attrs`/`data-*` (byte-identical-attrs contract) or
    // `hidden_spans` (it's not a hiding instruction; a kind that wants it
    // hidden puts it in `hidden` too). It stays visible on the line, simply
    // excluded from both derived outputs.
    let template_only_group: Option<&str> = shape
        .template_group
        .as_deref()
        .filter(|g| Some(*g) != shape.content_group.as_deref());

    // Named groups beyond `contentGroup`/`hidden`/`template_group` emit as
    // `data-*` line attributes (spec: "Named groups beyond
    // contentGroup/hidden emit as data-* line attributes"). `contentGroup`
    // itself is also captured as an attr so consumers get the extracted text
    // alongside its span.
    for name in re.capture_names().flatten() {
        if hidden.contains(name) || Some(name) == template_only_group {
            continue;
        }
        if let Some(m) = caps.name(name) {
            attrs.push((name.to_owned(), m.as_str().to_owned()));
        }
    }

    attrs.sort();
    DialectMatch {
        kind: kind.to_owned(),
        attrs,
        hidden_spans,
        content_span,
    }
}

// ─── The at-cue preset (Default) ───────────────────────────────────────

/// Build the `@Name:<>` at-cue preset — byte-identical to today's hardcoded
/// TS behavior:
/// - `character`: `@Name:<>` — hidden `@` prefix (1 byte) and hidden `:<>`
///   suffix (3 bytes, matching `CHAR_SUFFIX_LEN`), content is the name.
/// - `parenthetical`: `(text)<>` — hidden `<>` glue suffix only (2 bytes,
///   matching `GLUE_LEN`); the parens themselves are part of the content,
///   not hidden (matches `screenplay.ts`: only `<>` is replaced).
/// - `dialogue`: chain-only kind (no `source`) — narrative immediately
///   after `character`/`parenthetical`/`dialogue` becomes `dialogue`; blank
///   always breaks (chain rule enforces `after: narrative` only, per the
///   TS post-pass which checks `type === NarrativeText`, excluding
///   `ChoiceBody`-typed lines — i.e. chaining runs on narrative only).
#[must_use]
pub fn at_cue_preset() -> DialogueDialect {
    let character = DialectElement {
        kind: "character".to_owned(),
        nature: ElementNature::Narrative,
        source: Some(SourceShape::Pattern(PatternShape {
            pattern: r"^(?<lead>@)(?<speaker>[^:]*)(?<tail>:<>)$".to_owned(),
            content_group: Some("speaker".to_owned()),
            template_group: None,
            hidden: vec!["lead".to_owned(), "tail".to_owned()],
            template: "@${speaker}:<>".to_owned(),
        })),
        emitted: Some(EmittedShape {
            pattern: r"^@(?<speaker>[^:]*):\s*".to_owned(),
            content_group: Some("speaker".to_owned()),
            reserved_prefix: true,
        }),
        malformed: vec![MalformedRule {
            pattern: r"^@[^:]*$".to_owned(),
            message: "Character cue is missing the ':<>' terminator".to_owned(),
            severity: "warning".to_owned(),
        }],
    };

    let parenthetical = DialectElement {
        kind: "parenthetical".to_owned(),
        nature: ElementNature::Narrative,
        source: Some(SourceShape::Pattern(PatternShape {
            // `content` (outer, parens-inclusive) drives `content_span` —
            // the parens stay visible/editable/markup-scoped content (see
            // `screenplay.ts`: "Parenthetical's leading paren is content,
            // not hidden"). `content_inner` (nested, bare) is `template_group`
            // — the group whose value fills the template placeholder, so a
            // convert/strip row targeting `parenthetical` from a bare-content
            // source round-trips correctly (#406): the literal parens live in
            // `template` itself, matching every other convert/strip
            // consumer's "Parenthetical content is the bare text between the
            // parens" convention (`@brink/ink-operations`'s
            // `DEFAULT_CONVERTIBLE_SHAPES`, the built-in
            // `convertToParenthetical`/`stripToNarrative` actions).
            pattern: r"^(?<content>\((?<content_inner>[^)]*)\))(?<tail><>)$".to_owned(),
            content_group: Some("content".to_owned()),
            template_group: Some("content_inner".to_owned()),
            hidden: vec!["tail".to_owned()],
            template: "(${content_inner})<>".to_owned(),
        })),
        emitted: Some(EmittedShape {
            pattern: r"^(?<content>\([^)]*\))\s*".to_owned(),
            content_group: Some("content".to_owned()),
            reserved_prefix: false,
        }),
        malformed: vec![MalformedRule {
            pattern: r"^\([^)]*\)$".to_owned(),
            message: "Parenthetical is missing the '<>' terminator".to_owned(),
            severity: "warning".to_owned(),
        }],
    };

    let dialogue = DialectElement {
        kind: "dialogue".to_owned(),
        nature: ElementNature::Narrative,
        source: None,
        emitted: None,
        malformed: Vec::new(),
    };

    DialogueDialect {
        version: 1,
        name: "at-cue".to_owned(),
        elements: vec![character, parenthetical, dialogue],
        chain: vec![ChainRule {
            after: vec![
                "character".to_owned(),
                "parenthetical".to_owned(),
                "dialogue".to_owned(),
            ],
            is: vec!["narrative".to_owned()],
            becomes: "dialogue".to_owned(),
            carry: vec!["speaker".to_owned()],
        }],
        transitions: Vec::new(),
        templates: Templates {
            entries: vec![
                TemplateEntry {
                    kind: "character".to_owned(),
                    label: "Character cue".to_owned(),
                    picker_key: Some("@".to_owned()),
                    blank_tab: true,
                },
                TemplateEntry {
                    kind: "parenthetical".to_owned(),
                    label: "Parenthetical".to_owned(),
                    picker_key: Some("(".to_owned()),
                    blank_tab: false,
                },
            ],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_at_cue_preset() {
        let d = DialogueDialect::default();
        assert_eq!(d.name, "at-cue");
        assert_eq!(d.elements.len(), 3);
    }

    #[test]
    fn at_cue_preset_validates() {
        let d = at_cue_preset();
        assert_eq!(validate(&d), Ok(()));
    }

    #[test]
    fn character_cue_classifies() {
        let d = ResolvedDialect::compile(&at_cue_preset()).expect("compile");
        let m = d.classify("@Alice:<>", 0).expect("match");
        assert_eq!(m.kind, "character");
        assert_eq!(m.attrs, vec![("speaker".to_owned(), "Alice".to_owned())]);
        // hidden: '@' (0,1) and ':<>' (6,9)
        assert_eq!(m.hidden_spans, vec![(0, 1), (6, 9)]);
        assert_eq!(m.content_span, Some((1, 6)));
    }

    #[test]
    fn parenthetical_classifies_with_parens_in_content() {
        let d = ResolvedDialect::compile(&at_cue_preset()).expect("compile");
        let m = d.classify("(warmly)<>", 0).expect("match");
        assert_eq!(m.kind, "parenthetical");
        assert_eq!(m.content_span, Some((0, 8)));
        assert_eq!(m.hidden_spans, vec![(8, 10)]);
    }

    /// #406 — `content_group` (drives `content_span`/markup geometry) stays
    /// parens-inclusive (unchanged from the test above); `template_group`
    /// (new, additive) names the separate inner bare-text group used for
    /// convert/strip round-trips. The inner group must never leak into
    /// `attrs`/`data-*` (byte-identical-attrs contract) even though it's a
    /// real, visible (non-hidden) capture group.
    #[test]
    fn parenthetical_template_group_is_bare_and_excluded_from_attrs() {
        let d = ResolvedDialect::compile(&at_cue_preset()).expect("compile");
        let m = d.classify("(warmly)<>", 0).expect("match");
        assert_eq!(m.kind, "parenthetical");
        // Geometry unchanged: content_span still spans the parens-inclusive
        // outer group.
        assert_eq!(m.content_span, Some((0, 8)));
        // No new attr for the inner `content_inner` group — only the outer
        // `content` group (itself already an attr, per existing behavior).
        assert_eq!(m.attrs, vec![("content".to_owned(), "(warmly)".to_owned())]);
    }

    /// #406 — the round-trip a `convert` transition row actually needs: a
    /// dialect-declared kind converting INTO `parenthetical` from a
    /// bare-content source must produce a correctly-wrapped result. This
    /// mirrors what `executeDialectRow`'s `convert` action does:
    /// `dialect.convertibleShapes()` (fed by `template_group ?? content_group`)
    /// extracts the source's bare content, then `templateFor`/`renderTemplate`
    /// (fed by the SAME `template_group` name as the template's placeholder)
    /// fills the target's template.
    #[test]
    fn parenthetical_template_group_round_trips_bare_content_through_template() {
        let dialect = at_cue_preset();
        let el = dialect
            .elements
            .iter()
            .find(|e| e.kind == "parenthetical")
            .expect("parenthetical element");
        let source = el
            .source
            .as_ref()
            .expect("parenthetical has a source shape");
        let SourceShape::Pattern(shape) = source else {
            unreachable!("at_cue_preset's parenthetical is a raw Pattern shape, not Affix sugar");
        };
        // The group that should fill the template for a convert/strip round
        // trip is `template_group`, not `content_group` — `content_group`'s
        // own value ("(radio)") would double-wrap if substituted directly.
        let role = shape.template_group.as_deref().expect("template_group set");
        assert_eq!(role, "content_inner");
        let rendered = shape.template.replace(&format!("${{{role}}}"), "radio");
        assert_eq!(rendered, "(radio)<>");
        // And the rendered line re-classifies back to the same bare content.
        let re = regex::Regex::new(&shape.pattern).expect("pattern compiles");
        let caps = re.captures(&rendered).expect("rendered line re-matches");
        assert_eq!(caps.name(role).map(|m| m.as_str()), Some("radio"));
    }

    #[test]
    fn plain_prose_does_not_classify() {
        let d = ResolvedDialect::compile(&at_cue_preset()).expect("compile");
        assert!(d.classify("Hello world", 0).is_none());
        // Negative fixture from the spec: prose with '@' but no ':<>' tail
        // must NOT classify as a cue.
        assert!(d.classify("@channel: hello", 0).is_none());
        assert!(d.classify("(aside) unterminated", 0).is_none());
    }

    #[test]
    fn affix_sugar_compiles_to_equivalent_pattern() {
        let affix = AffixShape {
            prefix: Some("@".to_owned()),
            suffix: Some(":".to_owned()),
            glued: true,
            content_role: "speaker".to_owned(),
        };
        let compiled = compile_affix(&affix);
        let re = regex::Regex::new(&compiled.pattern).expect("valid regex");
        let caps = re.captures("@Bob:<>").expect("matches");
        assert_eq!(&caps["speaker"], "Bob");
        assert_eq!(compiled.template, "@${speaker}:<>");
    }

    #[test]
    fn portable_pattern_rejects_lookaround_and_backrefs() {
        assert!(check_portable_pattern(r"^(?=foo)bar$").is_err());
        assert!(check_portable_pattern(r"^(?!foo)bar$").is_err());
        assert!(check_portable_pattern(r"^(?<=foo)bar$").is_err());
        assert!(check_portable_pattern(r"^(?<!foo)bar$").is_err());
        assert!(check_portable_pattern(r"^(\w+)\1$").is_err());
        assert!(check_portable_pattern(r"^(?<name>\w+)\k<name>$").is_err());
        // Named groups (not lookaround) are fine.
        assert!(check_portable_pattern(r"^(?<lead>@)(?<speaker>[^:]*)$").is_ok());
    }

    #[test]
    fn validate_rejects_non_portable_pattern() {
        let mut d = at_cue_preset();
        d.elements[0].source = Some(SourceShape::Pattern(PatternShape {
            pattern: r"^(?=@)(?<speaker>[^:]*):<>$".to_owned(),
            content_group: Some("speaker".to_owned()),
            template_group: None,
            hidden: Vec::new(),
            template: "@${speaker}:<>".to_owned(),
        }));
        let result = validate(&d);
        assert!(result.is_err());
    }

    #[test]
    fn validate_rejects_undeclared_chain_kind() {
        let mut d = at_cue_preset();
        d.chain[0].after.push("nonexistent_kind".to_owned());
        let errs = validate(&d).expect_err("should fail");
        assert!(
            errs.iter().any(
                |e| matches!(e, DialectError::ChainUndeclaredKind(k) if k == "nonexistent_kind")
            )
        );
    }

    #[test]
    fn validate_allows_reserved_structural_chain_kind() {
        let mut d = at_cue_preset();
        d.chain[0].after.push("narrative".to_owned());
        assert_eq!(validate(&d), Ok(()));
    }

    /// Issue #2115: before this slice, `validate` checked `transitions` but
    /// never `templates` at all — a `TemplateEntry.kind` naming an
    /// undeclared, non-reserved kind validated silently. This is the direct,
    /// always-run (non-wasm) regression test for that gap; the wasm-facing
    /// equivalent through `set_dialect` lives in
    /// `crates/brink-web/src/editor/mod.rs`'s `dialect_wasm_tests`.
    #[test]
    fn validate_rejects_undeclared_template_kind() {
        let mut d = at_cue_preset();
        d.templates.entries.push(TemplateEntry {
            kind: "nonexistent_kind".to_owned(),
            label: "Nonexistent".to_owned(),
            picker_key: None,
            blank_tab: false,
        });
        let errs = validate(&d).expect_err("should fail");
        assert!(errs.iter().any(
            |e| matches!(e, DialectError::TemplateUndeclaredKind(k) if k == "nonexistent_kind")
        ));
    }

    #[test]
    fn validate_rejects_duplicate_kind() {
        let mut d = at_cue_preset();
        let dup = d.elements[0].clone();
        d.elements.push(dup);
        let errs = validate(&d).expect_err("should fail");
        assert!(
            errs.iter()
                .any(|e| matches!(e, DialectError::DuplicateKind(k) if k == "character"))
        );
    }

    #[test]
    fn json_roundtrip() {
        let d = at_cue_preset();
        let json = serde_json::to_string_pretty(&d).expect("serialize");
        let back: DialogueDialect = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(d, back);
    }

    #[test]
    fn chain_rule_after_lookup() {
        let d = ResolvedDialect::compile(&at_cue_preset()).expect("compile");
        let rule = d.chain_rule_after("character").expect("rule");
        assert_eq!(rule.becomes, "dialogue");
        assert_eq!(rule.carry, vec!["speaker".to_owned()]);
        assert!(d.chain_rule_after("choice").is_none());
    }

    #[test]
    fn pattern_less_kind_has_no_pattern() {
        let d = ResolvedDialect::compile(&at_cue_preset()).expect("compile");
        // "dialogue" is chain-only; classify() never matches it directly
        // since it has no compiled pattern to test against.
        assert!(d.classify("dialogue", 0).is_none());
    }
}
