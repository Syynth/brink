//! Extract-selection ops — lift selected lines into a new knot or function (#315 H).
//!
//! Two backend ops, both session-aware and safe-by-default (#316):
//!
//! - [`extract_to_knot`] lifts the selected lines into a new top-level
//!   `=== name ===` knot and replaces the selection with a **tunnel** call
//!   `-> name ->`, so control flows into the extracted content and returns to
//!   the call site.
//! - [`extract_to_function`] lifts the selection into a new
//!   `=== function name() ===` and replaces it with the call — `{name()}` when
//!   the selection is a single value expression, or `~ name()` when it reads as
//!   a statement.
//!
//! Both compute a full-file rewrite and run the op-agnostic breakage gate
//! ([`gate_with_source`]): extraction can pull weave/gather labels or
//! local/temp references out of scope, so any diagnostics the edit *would*
//! introduce travel out in [`StructuralResult::introduced`] and the op is
//! marked unsafe. v1 does **not** auto-thread parameters — an out-of-scope
//! local surfaces as a diagnostic for the author to resolve.
//!
//! ## Deterministic choices (documented, per spec)
//!
//! - **Placement.** The new declaration is appended at the **end of the file**
//!   (after a blank-line separator). This keeps placement independent of where
//!   the selection lives and never splits an existing knot.
//! - **Selection snapping.** The byte range is **snapped to whole lines** — the
//!   start expands back to its line start, the end expands forward to the end of
//!   its line (including the trailing newline when present). Extraction operates
//!   on lines, not partial tokens.
//! - **Header crossing.** A selection that (after snapping) **intersects a knot
//!   or stitch header line is rejected** ([`ExtractError::CrossesHeader`]).
//!   Extraction moves body content; it never relocates a declaration header.

use brink_syntax::ast::{AstNode as _, KnotDef};

use crate::session::IdeSession;
use crate::structural_result::{StructuralResult, gate_with_source};

/// Errors that can occur while computing an extract op.
#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    #[error("file not loaded")]
    FileNotFound,
    #[error("empty selection: nothing to extract")]
    EmptySelection,
    #[error("selection crosses a knot or stitch header")]
    CrossesHeader,
    #[error("name collision: '{0}' already exists as a top-level knot")]
    NameCollision(String),
    #[error("name collision: '{0}' already exists as a variable, const, or list")]
    VarCollision(String),
    #[error("invalid extraction name: '{0}'")]
    InvalidName(String),
    #[error("selection cannot be a function body: it contains a divert, choice, or gather")]
    IllegalFunctionBody,
}

/// Extract the selected lines into a new top-level `=== name ===` knot,
/// replacing the selection with a tunnel call `-> name ->`.
///
/// `path` names the primary file; `start`/`end` are byte offsets into its
/// source (snapped to whole lines); `name` is the new knot's name. Returns the
/// rewritten primary source plus the breakage the extraction introduces (per
/// the #316 gate). The session is not mutated.
///
/// # Errors
/// See [`ExtractError`].
pub fn extract_to_knot(
    session: &IdeSession,
    path: &str,
    start: usize,
    end: usize,
    name: &str,
) -> Result<StructuralResult, ExtractError> {
    let source = session
        .source(session.file_id(path).ok_or(ExtractError::FileNotFound)?)
        .ok_or(ExtractError::FileNotFound)?;

    let plan = plan_extraction(source, start, end, name)?;

    // Replace the selection with a tunnel call, indentation preserved.
    let call_line = format!("{}-> {name} ->\n", plan.indent);
    let new_source = rebuild(source, &plan, &call_line, ExtractKind::Knot, name);

    Ok(gated(session, path, new_source))
}

/// Extract the selected lines into a new `=== function name() ===`, replacing
/// the selection with the call.
///
/// The call form is chosen deterministically from the selection: a single
/// value expression (one non-empty line, no statement/divert/choice markers)
/// becomes an inline `{name()}`; anything else becomes a statement `~ name()`.
/// v1 does not thread parameters — references to enclosing locals/temps that
/// fall out of scope surface as introduced diagnostics.
///
/// # Errors
/// See [`ExtractError`].
pub fn extract_to_function(
    session: &IdeSession,
    path: &str,
    start: usize,
    end: usize,
    name: &str,
) -> Result<StructuralResult, ExtractError> {
    let source = session
        .source(session.file_id(path).ok_or(ExtractError::FileNotFound)?)
        .ok_or(ExtractError::FileNotFound)?;

    let plan = plan_extraction(source, start, end, name)?;

    // Ink functions cannot divert or present choices/gathers (WritingWithInk.md
    // §Functions). A selection that contains any of those markers would produce
    // a structurally-illegal function body; refuse rather than silently accept
    // it as "safe". (extract_to_knot has no such restriction — a knot may divert
    // and branch freely.)
    if selection_has_flow_control(&plan.selected) {
        return Err(ExtractError::IllegalFunctionBody);
    }

    let call_line = if is_value_expression(&plan.selected) {
        format!("{}{{{name}()}}\n", plan.indent)
    } else {
        format!("{}~ {name}()\n", plan.indent)
    };
    let new_source = rebuild(source, &plan, &call_line, ExtractKind::Function, name);

    Ok(gated(session, path, new_source))
}

// ── Internals ────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum ExtractKind {
    Knot,
    Function,
}

/// The computed plan for an extraction: the snapped selection window, the
/// extracted text, and the indentation of the first selected line (reused for
/// the replacement call so it lands at the same nesting).
struct ExtractionPlan {
    sel_start: usize,
    sel_end: usize,
    selected: String,
    indent: String,
}

/// Validate the request and compute the snapped window + extracted text.
fn plan_extraction(
    source: &str,
    start: usize,
    end: usize,
    name: &str,
) -> Result<ExtractionPlan, ExtractError> {
    if !is_valid_name(name) {
        return Err(ExtractError::InvalidName(name.to_owned()));
    }

    let (lo, hi) = (start.min(end), start.max(end));
    // A caret (zero-width selection) has nothing to extract; require a real span.
    if lo == hi {
        return Err(ExtractError::EmptySelection);
    }
    let (sel_start, sel_end) = snap_to_lines(source, lo, hi);
    if sel_start >= sel_end {
        return Err(ExtractError::EmptySelection);
    }

    let parse = brink_syntax::parse(source);
    let tree = parse.tree();
    let knots: Vec<KnotDef> = tree.knots().collect();

    // Reject if the snapped selection intersects any knot or stitch header line.
    for knot in &knots {
        if let Some(h) = knot.header()
            && ranges_intersect(sel_start, sel_end, h.syntax().text_range())
        {
            return Err(ExtractError::CrossesHeader);
        }
        if let Some(body) = knot.body() {
            for stitch in body.stitches() {
                if let Some(h) = stitch.header()
                    && ranges_intersect(sel_start, sel_end, h.syntax().text_range())
                {
                    return Err(ExtractError::CrossesHeader);
                }
            }
        }
    }

    // Name collision with an existing top-level knot.
    if knots
        .iter()
        .any(|k| k.header().and_then(|h| h.name()).as_deref() == Some(name))
    {
        return Err(ExtractError::NameCollision(name.to_owned()));
    }

    // Name collision with a declared VAR / CONST / LIST. The analyzer does not
    // model var↔knot/function clashes, so the #316 overlay gate cannot catch
    // this — but the op knows the new name up front, so reject it deterministically
    // here. Both extract-to-knot and extract-to-function introduce a top-level
    // symbol that would shadow / clash with a global of the same name.
    let name_clashes = tree
        .var_decls()
        .filter_map(|v| v.name())
        .chain(tree.const_decls().filter_map(|c| c.name()))
        .chain(tree.list_decls().filter_map(|l| l.name()))
        .any(|n| n == name);
    if name_clashes {
        return Err(ExtractError::VarCollision(name.to_owned()));
    }

    let selected = source[sel_start..sel_end].to_owned();
    if selected.trim().is_empty() {
        return Err(ExtractError::EmptySelection);
    }
    let indent = leading_indent(source, sel_start);

    Ok(ExtractionPlan {
        sel_start,
        sel_end,
        selected,
        indent,
    })
}

/// Rebuild the full source: selection replaced by `call_line`, and the new
/// declaration appended at end of file (after a blank-line separator).
fn rebuild(
    source: &str,
    plan: &ExtractionPlan,
    call_line: &str,
    kind: ExtractKind,
    name: &str,
) -> String {
    let header = match kind {
        ExtractKind::Knot => format!("=== {name} ===\n"),
        ExtractKind::Function => format!("=== function {name}() ===\n"),
    };

    // The extracted body is the selection with its common indentation stripped
    // to the left margin (a top-level decl body is not nested). A knot body must
    // return to its caller — a tunnel returns via `->` at the end.
    let mut body = dedent(&plan.selected);
    if !body.ends_with('\n') {
        body.push('\n');
    }

    let mut out = String::with_capacity(source.len() + header.len() + body.len() + 8);
    out.push_str(&source[..plan.sel_start]);
    out.push_str(call_line);
    out.push_str(&source[plan.sel_end..]);

    // Separator before the appended declaration.
    if !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.ends_with("\n\n") {
        out.push('\n');
    }
    out.push_str(&header);
    out.push_str(&body);
    if matches!(kind, ExtractKind::Knot) {
        // A tunnel returns to its call site via the `->->` statement.
        out.push_str("->->\n");
    }
    out
}

/// Run the #316 breakage gate over the rewritten source and package the result.
fn gated(session: &IdeSession, path: &str, new_source: String) -> StructuralResult {
    let introduced = gate_with_source(session, path, &new_source, &[]);
    StructuralResult {
        new_source: Some(new_source),
        cross_file_edits: Vec::new(),
        safe: introduced.is_empty(),
        introduced,
    }
}

/// Snap `[lo, hi)` outward to whole lines: `lo` back to its line start, `hi`
/// forward to the end of its line (including the trailing newline, if any).
fn snap_to_lines(source: &str, lo: usize, hi: usize) -> (usize, usize) {
    let lo = lo.min(source.len());
    let hi = hi.min(source.len());
    let start = source[..lo].rfind('\n').map_or(0, |i| i + 1);
    // Extend `hi` to just past the next newline at or after `hi`; if `hi`
    // already sits at a line start (and hi > lo), keep it there so a selection
    // ending exactly at a boundary doesn't swallow the following line.
    let end = if hi > start && source.as_bytes().get(hi.wrapping_sub(1)) == Some(&b'\n') {
        hi
    } else {
        match source[hi..].find('\n') {
            Some(rel) => hi + rel + 1,
            None => source.len(),
        }
    };
    (start, end)
}

/// The leading whitespace of the line containing `offset` (its indentation).
fn leading_indent(source: &str, offset: usize) -> String {
    let line_start = source[..offset.min(source.len())]
        .rfind('\n')
        .map_or(0, |i| i + 1);
    let line = &source[line_start..];
    line.chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect()
}

/// Strip the common leading indentation from every non-blank line of `text`.
fn dedent(text: &str) -> String {
    let min_indent = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    if min_indent == 0 {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    for (i, line) in text.lines().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if line.len() >= min_indent && !line.trim().is_empty() {
            out.push_str(&line[min_indent..]);
        } else {
            out.push_str(line.trim_start());
        }
    }
    if text.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Whether the selection reads as a single inline value expression — one
/// non-empty logical line with no statement (`~`), divert (`->`), choice
/// (`*`/`+`), or gather (`-`) marker. Used to pick `{name()}` over `~ name()`.
fn is_value_expression(selected: &str) -> bool {
    let lines: Vec<&str> = selected.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.len() != 1 {
        return false;
    }
    let t = lines[0].trim_start();
    // Statement / control markers ⇒ not a value expression.
    !(t.starts_with('~')
        || t.starts_with("->")
        || t.starts_with('*')
        || t.starts_with('+')
        || t.starts_with('-')
        || t.starts_with('='))
}

/// Whether the selection contains ink flow-control that is illegal inside a
/// function body: a divert (`->`, including tunnel `->->` / `-> knot ->`), a
/// choice (`*` / `+` at line start), or a gather (`-` at line start, but not a
/// logic line `~`). Used to reject function extraction of a non-function-shaped
/// selection. This is intentionally conservative — a leading marker or an
/// anywhere-`->` flags the selection.
fn selection_has_flow_control(selected: &str) -> bool {
    for line in selected.lines() {
        let t = line.trim_start();
        if t.is_empty() {
            continue;
        }
        // A divert / tunnel anywhere on the line (`-> knot`, `text -> knot`,
        // `->->`). Functions cannot divert at all.
        if t.contains("->") {
            return true;
        }
        // Line-leading choice or gather markers. `-` is a gather unless it is the
        // start of a `->` divert (already handled above) or a logic line marker
        // (`~`). A bare leading `-` (followed by space or content) is a gather.
        let first = t.as_bytes().first().copied();
        match first {
            Some(b'*' | b'+') => return true,
            Some(b'-') => {
                // Not `->` (handled above); a leading `-` is a gather.
                return true;
            }
            _ => {}
        }
    }
    false
}

/// Two half-open byte windows overlap.
fn ranges_intersect(a_start: usize, a_end: usize, r: rowan::TextRange) -> bool {
    let (r_start, r_end): (usize, usize) = (r.start().into(), r.end().into());
    a_start < r_end && r_start < a_end
}

/// A syntactically valid extraction target name (ink identifier).
fn is_valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::{extract_to_function, extract_to_knot, is_valid_name};
    use crate::session::IdeSession;

    fn session(src: &str) -> IdeSession {
        let mut s = IdeSession::new();
        s.update_and_analyze("main.ink", src.to_owned());
        s
    }

    /// Byte offset of the start of the line beginning with `needle`.
    fn line_start(src: &str, needle: &str) -> usize {
        src.find(needle).expect("needle present")
    }

    #[test]
    fn extract_to_knot_lifts_content_and_leaves_tunnel_call() {
        let src = "=== start ===\nHello there.\nSecond line.\n-> END\n";
        let s = session(src);
        let start = line_start(src, "Hello there.");
        let end = line_start(src, "-> END");
        let r = extract_to_knot(&s, "main.ink", start, end, "greeting").expect("extract");
        let out = r.new_source.as_deref().expect("new_source");

        // A new top-level knot holds the extracted content.
        assert!(out.contains("=== greeting ==="), "new knot header: {out}");
        assert!(
            out.contains("Hello there.\nSecond line."),
            "content lifted verbatim: {out}"
        );
        // The selection is replaced by a tunnel call that returns.
        assert!(out.contains("-> greeting ->"), "tunnel call at site: {out}");
        // The extracted lines are gone from the original spot (only in the knot).
        assert_eq!(
            out.matches("Hello there.").count(),
            1,
            "content moved, not duplicated: {out}"
        );
        // The tunnel returns.
        assert!(
            out.trim_end().ends_with("->->"),
            "extracted knot ends with a tunnel return: {out}"
        );
    }

    #[test]
    fn extract_to_knot_runs_identically() {
        // The story: prints A, B, then ends. After extracting the two prints
        // into a tunnel, the visible output must be identical.
        //
        // A top-level `-> start` divert is required so the `start` knot is
        // actually entered and its content runs — without it the runtime emits
        // no output for either side and the identity assertion would pass
        // vacuously (`[] == []`), verifying nothing about the extraction.
        let src = "-> start\n=== start ===\nA\nB\n-> END\n";
        let s = session(src);
        let start = line_start(src, "A\n");
        let end = line_start(src, "-> END");
        let r = extract_to_knot(&s, "main.ink", start, end, "body").expect("extract");
        let out = r.new_source.as_deref().expect("new_source");

        let before = run_story(src);
        let after = run_story(out);
        // Guard against the vacuous case: the story must actually emit output.
        assert_eq!(before, vec!["A\n", "B\n"], "fixture actually runs");
        assert_eq!(before, after, "extraction preserves runtime output\n{out}");
        assert!(r.safe, "self-contained extraction is safe: {:?}", codes(&r));
    }

    #[test]
    fn extract_to_function_value_expression_uses_inline_call() {
        // A single value line → `{name()}`.
        let src = "=== start ===\n{2 + 3}\n-> END\n";
        let s = session(src);
        let start = line_start(src, "{2 + 3}");
        let end = line_start(src, "-> END");
        let r = extract_to_function(&s, "main.ink", start, end, "calc").expect("extract");
        let out = r.new_source.as_deref().expect("new_source");
        assert!(
            out.contains("=== function calc() ==="),
            "function decl: {out}"
        );
        assert!(out.contains("{calc()}"), "inline value call: {out}");
    }

    #[test]
    fn extract_to_function_statement_uses_tilde_call() {
        // Multiple lines / a statement → `~ name()`.
        let src = "VAR x = 0\n=== start ===\n~ x = 1\n~ x = 2\n-> END\n";
        let s = session(src);
        let start = line_start(src, "~ x = 1");
        let end = line_start(src, "-> END");
        let r = extract_to_function(&s, "main.ink", start, end, "mutate").expect("extract");
        let out = r.new_source.as_deref().expect("new_source");
        assert!(
            out.contains("=== function mutate() ==="),
            "function decl: {out}"
        );
        assert!(out.contains("~ mutate()"), "statement call: {out}");
    }

    #[test]
    fn extract_that_breaks_scope_reports_introduced_diagnostics() {
        // A temp declared in `start` is referenced by the extracted line; once
        // lifted into a separate knot, `count` is out of scope there.
        let src = "=== start ===\n~ temp count = 3\n{count}\n-> END\n";
        let s = session(src);
        let start = line_start(src, "{count}");
        let end = line_start(src, "-> END");
        let r = extract_to_knot(&s, "main.ink", start, end, "shower").expect("extract");
        assert!(
            !r.safe,
            "extracting a line that references an enclosing temp is unsafe: {:?}",
            codes(&r)
        );
        assert!(
            !r.introduced.is_empty(),
            "the out-of-scope reference is reported: {:?}",
            codes(&r)
        );
    }

    #[test]
    fn extract_rejects_header_crossing_selection() {
        let src = "=== a ===\nContent.\n=== b ===\n-> END\n";
        let s = session(src);
        // Selection spanning from a's content through b's header.
        let start = line_start(src, "Content.");
        let end = line_start(src, "-> END");
        let r = extract_to_knot(&s, "main.ink", start, end, "x");
        assert!(matches!(r, Err(super::ExtractError::CrossesHeader)));
    }

    #[test]
    fn extract_rejects_name_collision() {
        let src = "=== start ===\nContent.\n-> END\n=== taken ===\n-> END\n";
        let s = session(src);
        let start = line_start(src, "Content.");
        let end = line_start(src, "-> END\n=== taken");
        let r = extract_to_knot(&s, "main.ink", start, end, "taken");
        assert!(matches!(r, Err(super::ExtractError::NameCollision(_))));
    }

    #[test]
    fn extract_rejects_collision_with_var() {
        // A VAR of the same name is a genuine collision the analyzer does not
        // model; the extract op must reject it up front rather than reporting
        // safe=true.
        let src = "VAR foo = 0\n-> start\n=== start ===\nContent.\n-> END\n";
        let s = session(src);
        let start = line_start(src, "Content.");
        let end = line_start(src, "-> END");
        let r = extract_to_knot(&s, "main.ink", start, end, "foo");
        assert!(matches!(r, Err(super::ExtractError::VarCollision(_))));
    }

    #[test]
    fn extract_to_function_rejects_collision_with_var() {
        // Same clash, function form: `=== function foo() ===` vs `VAR foo`.
        let src = "VAR foo = 0\n=== start ===\n{2 + 3}\n-> END\n";
        let s = session(src);
        let start = line_start(src, "{2 + 3}");
        let end = line_start(src, "-> END");
        let r = extract_to_function(&s, "main.ink", start, end, "foo");
        assert!(matches!(r, Err(super::ExtractError::VarCollision(_))));
    }

    #[test]
    fn extract_rejects_collision_with_const_and_list() {
        let src_const = "CONST bar = 7\n=== start ===\n{2 + 3}\n-> END\n";
        let s = session(src_const);
        let start = line_start(src_const, "{2 + 3}");
        let end = line_start(src_const, "-> END");
        let r = extract_to_function(&s, "main.ink", start, end, "bar");
        assert!(
            matches!(r, Err(super::ExtractError::VarCollision(_))),
            "const collision"
        );

        let src_list = "LIST baz = a, b, c\n=== start ===\n{2 + 3}\n-> END\n";
        let s = session(src_list);
        let start = line_start(src_list, "{2 + 3}");
        let end = line_start(src_list, "-> END");
        let r = extract_to_function(&s, "main.ink", start, end, "baz");
        assert!(
            matches!(r, Err(super::ExtractError::VarCollision(_))),
            "list collision"
        );
    }

    #[test]
    fn extract_to_function_rejects_divert_in_selection() {
        // A function cannot divert; a selection containing `-> other` must be
        // refused, not marked safe.
        let src = "-> start\n=== start ===\nHi.\n-> other\n=== other ===\n-> END\n";
        let s = session(src);
        let start = line_start(src, "Hi.");
        let end = line_start(src, "=== other ===");
        let r = extract_to_function(&s, "main.ink", start, end, "f");
        assert!(
            matches!(r, Err(super::ExtractError::IllegalFunctionBody)),
            "divert in function body rejected"
        );
        // extract_to_knot has no such restriction — the same selection is fine.
        let k = extract_to_knot(&s, "main.ink", start, end, "f");
        assert!(k.is_ok(), "knot extraction allows diverts");
    }

    #[test]
    fn extract_to_function_rejects_choice_in_selection() {
        let src = "-> start\n=== start ===\n* [Go] Went.\n- Done.\n-> END\n";
        let s = session(src);
        let start = line_start(src, "* [Go]");
        let end = line_start(src, "-> END");
        let r = extract_to_function(&s, "main.ink", start, end, "f");
        assert!(
            matches!(r, Err(super::ExtractError::IllegalFunctionBody)),
            "choice/gather in function body rejected"
        );
    }

    #[test]
    fn extract_rejects_empty_selection() {
        let src = "=== start ===\nContent.\n-> END\n";
        let s = session(src);
        let at = line_start(src, "Content.");
        let r = extract_to_knot(&s, "main.ink", at, at, "x");
        assert!(matches!(r, Err(super::ExtractError::EmptySelection)));
    }

    #[test]
    fn extract_preserves_indented_choice_content() {
        // A choice with indented content; the selection preserves the markers.
        let src = "=== start ===\n* [Go] You went.\n- End of options.\n-> END\n";
        let s = session(src);
        let start = line_start(src, "* [Go]");
        let end = line_start(src, "-> END");
        let r = extract_to_knot(&s, "main.ink", start, end, "menu").expect("extract");
        let out = r.new_source.as_deref().expect("new_source");
        assert!(out.contains("* [Go] You went."), "choice preserved: {out}");
        assert!(out.contains("- End of options."), "gather preserved: {out}");
        assert!(out.contains("-> menu ->"), "tunnel call: {out}");
    }

    #[test]
    fn invalid_name_is_rejected() {
        assert!(is_valid_name("good_name"));
        assert!(is_valid_name("_x1"));
        assert!(!is_valid_name("1bad"));
        assert!(!is_valid_name("has space"));
        assert!(!is_valid_name(""));
    }

    // ── helpers ─────────────────────────────────────────────────────

    fn codes(r: &crate::structural_result::StructuralResult) -> Vec<&str> {
        r.introduced.iter().map(|d| d.code.as_str()).collect()
    }

    /// Compile + link + run a single-file story to its terminal, collecting the
    /// visible text of every emitted line.
    fn run_story(src: &str) -> Vec<String> {
        let out = brink_compiler::compile("main.ink", |p| {
            if p == "main.ink" {
                Ok(src.to_owned())
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    p.to_owned(),
                ))
            }
        })
        .expect("compile");
        let (program, line_tables) = brink_runtime::link(&out.data).expect("link");
        let mut story = brink_runtime::Story::<brink_runtime::FastRng>::new(&program, line_tables);
        let mut texts = Vec::new();
        for line in story.continue_maximally().expect("run") {
            let text = match line {
                brink_runtime::Line::Text { text, .. }
                | brink_runtime::Line::Done { text, .. }
                | brink_runtime::Line::End { text, .. }
                | brink_runtime::Line::Choices { text, .. } => text,
            };
            if !text.is_empty() {
                texts.push(text);
            }
        }
        texts
    }
}
