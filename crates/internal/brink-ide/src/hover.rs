use std::fmt::Write as _;

use brink_analyzer::AnalysisResult;
use brink_db::ProjectDb;
use brink_ir::FileId;
use rowan::{TextRange, TextSize};

use crate::inferred_types::enclosing_callable;
use crate::navigation::find_def_at_offset;
use crate::{builtin_hover_text, stdlib_hover_text, word_at_offset, word_range_at_offset};

/// Hover information for a symbol.
pub struct HoverInfo {
    /// Markdown-formatted content.
    pub content: String,
    /// The range of the hovered symbol.
    pub range: Option<TextRange>,
}

/// Compute hover info for the symbol at `offset`.
///
/// `project_files` provides `(FileId, path, source)` tuples for cross-file
/// definition lookup (e.g. showing "Defined in `path`"). `db` is the
/// FG-narrowed per-def inference seam (TM-5, #621,
/// docs/typed-mode-spec.md §9 step 5): when a param/temp or a knot/stitch
/// signature has no *declared* type (no TM-2 annotation, no host-manifest
/// entry), hover falls back to `db.infer_body`/`db.inferred_signature`
/// rather than showing nothing — never the whole-project
/// `db.type_inference()`, which would recompute on every keystroke.
pub fn hover(
    analysis: &AnalysisResult,
    db: &ProjectDb,
    file_id: FileId,
    source: &str,
    offset: TextSize,
    project_files: &[(FileId, String, String)],
) -> Option<HoverInfo> {
    let content = if let Some(info) = find_def_at_offset(analysis, file_id, offset) {
        let kind_str = match info.kind {
            brink_ir::SymbolKind::Knot => "knot",
            brink_ir::SymbolKind::Stitch => "stitch",
            brink_ir::SymbolKind::Variable => "variable",
            brink_ir::SymbolKind::Constant => "constant",
            brink_ir::SymbolKind::List => "list",
            brink_ir::SymbolKind::ListItem => "list item",
            brink_ir::SymbolKind::External => "external function",
            brink_ir::SymbolKind::Label => "label",
            brink_ir::SymbolKind::Param => "parameter",
            brink_ir::SymbolKind::Temp => "temp variable",
            brink_ir::SymbolKind::Struct => "struct",
        };

        // Symbol-metadata enrichment: docs and typed params/returns for
        // externals, knots, and stitches; initializer info for VAR/CONST.
        let meta = analysis.symbol_meta.get(&info.id);

        let (params_str, ret_str) = signature_strs(info, meta, db);
        let inferred_local_str = inferred_local_type_str(analysis, db, info);

        // Initializer info: `health: int`, `SPEED: float = 0.5`.
        let value_str = meta
            .and_then(|m| m.value.as_ref())
            .map_or(String::new(), |v| {
                let mut s = String::new();
                if let Some(ty) = v.ty {
                    let _ = write!(s, ": {}", ty.name());
                }
                if let Some(text) = &v.value_text {
                    let _ = write!(s, " = {text}");
                }
                s
            });

        let kind_tag = meta.map_or(String::new(), |m| match m.kind {
            brink_ir::ExternalKind::Plain => String::new(),
            brink_ir::ExternalKind::Query => " [query]".to_string(),
            brink_ir::ExternalKind::Effect => " [effect]".to_string(),
            brink_ir::ExternalKind::Presentation => " [presentation]".to_string(),
        });

        let detail_str = info
            .detail
            .as_deref()
            .map_or(String::new(), |d| format!(" [{d}]"));

        let doc_block = meta
            .and_then(|m| m.doc.as_deref())
            .map_or(String::new(), |d| format!("\n\n{d}"));

        let file_note = project_files
            .iter()
            .find(|(fid, _, _)| *fid == info.file)
            .map_or(String::new(), |(_, p, _)| format!("\n\n*Defined in `{p}`*"));

        // T1c-4 (#702, docs/t1c-spec.md §11): a fn-value slot — a
        // VAR/CONST/temp whose declaration initializer is a direct
        // `#fn(target, args…)` literal — shows the same bound-signature
        // display form `string(f)` produces at runtime (spec §5), built
        // statically from the HIR since there is no compiled `Program` at
        // hover time. `None` for every other symbol and for indirect
        // (`bind()`, copy-of-a-variable) bindings — see
        // `fn_value_hover`'s module doc.
        let fn_value_str = db
            .hir(file_id)
            .and_then(|hir| crate::fn_value_hover::fn_value_slot_signature(analysis, hir, info))
            .map_or(String::new(), |sig| format!("\n\n`{sig}`"));

        // T2-4 (docs/effects-spec.md §10, issue #863): a knot/stitch's
        // inferred effect row — the display form spec §10's IDE-hover
        // tooling commitment names ("boring and stable"), one line, names
        // resolved through the same `analysis.index` every other hover
        // enrichment above reads. `None` for anything that isn't a
        // knot/stitch, or for a def `db.effects` has no row for — same
        // `None` contract as `db.inferred_signature`.
        let effects_str = matches!(
            info.kind,
            brink_ir::SymbolKind::Knot | brink_ir::SymbolKind::Stitch
        )
        .then(|| db.effects(info.id))
        .flatten()
        .map_or(String::new(), |row| {
            format!("\n\n{}", effect_row_line(&row, &analysis.index))
        });

        format!(
            "**{kind_str}** `{}{inferred_local_str}{value_str}{params_str}{ret_str}`{detail_str}{kind_tag}{doc_block}{file_note}{fn_value_str}{effects_str}",
            info.name
        )
    } else {
        let word = word_at_offset(source, offset)?;
        builtin_hover_text(word).or_else(|| stdlib_hover_text(word))?
    };

    let range = analysis
        .resolutions
        .iter()
        .find(|r| r.file == file_id && (r.range.contains(offset) || r.range.start() == offset))
        .map(|r| r.range)
        .or_else(|| word_range_at_offset(source, offset));

    Some(HoverInfo { content, range })
}

/// T2-4 (docs/effects-spec.md §10, issue #863): render a knot/stitch's
/// inferred [`brink_db::EffectRow`] as one `*effects:* …` hover line —
/// `reads`/`writes`/`calls` sets, resolved from raw `DefinitionId`s /
/// call-kind names back to source identifiers via `index`. Deliberately
/// boring and stable (spec §10): the same three-word-per-category shape
/// every build produces, so it reads as data, not prose. `pure` sugars the
/// empty, non-opaque row; `opaque` is the pessimal "touches everything"
/// floor (a call through a function value, or an unresolved callee) — no
/// atom list can bound it, so it gets its own line instead of an empty one.
fn effect_row_line(row: &brink_db::EffectRow, index: &brink_ir::SymbolIndex) -> String {
    if row.opaque {
        return "*effects:* opaque (calls through a function value, or an unresolved callee)"
            .to_string();
    }
    if row.is_empty() {
        return "*effects:* pure".to_string();
    }
    let name_of = |id: &brink_format::DefinitionId| {
        index
            .symbols
            .get(id)
            .map_or_else(|| format!("{id:?}"), |info| info.name.clone())
    };
    let mut parts = Vec::new();
    if !row.reads.is_empty() {
        let names: Vec<String> = row.reads.iter().map(name_of).collect();
        parts.push(format!("reads {}", names.join(", ")));
    }
    if !row.writes.is_empty() {
        let names: Vec<String> = row.writes.iter().map(name_of).collect();
        parts.push(format!("writes {}", names.join(", ")));
    }
    if !row.calls.is_empty() {
        let names: Vec<&str> = row.calls.iter().map(String::as_str).collect();
        parts.push(format!("calls {}", names.join(", ")));
    }
    format!("*effects:* {}", parts.join("; "))
}

/// TM-5 (#621): a knot/stitch's parameter-list and return-type hover
/// strings — `(params, return)` — layering three sources per position,
/// most-authoritative first:
///
/// 1. `meta` — doc-tag (`/// @param`) or host-manifest types (externals).
/// 2. `db.signature`'s `param_annotations`/`return_annotation` — TM-2's
///    inline `name: type` syntax. A completely different channel from (1);
///    the firewall rule (docs/typed-mode-spec.md §2) means this always
///    wins over inference.
/// 3. `db.inferred_signature` — the FG-narrowed per-def inference seam,
///    consulted only when neither declared source above covers a position.
///
/// `None` (empty strings) for anything that isn't a knot/stitch.
fn signature_strs(
    info: &brink_ir::SymbolInfo,
    meta: Option<&brink_analyzer::SymbolMeta>,
    db: &ProjectDb,
) -> (String, String) {
    // `db.signature`/`db.inferred_signature` are per-def queries over an
    // inferable knot/stitch body — meaningless for an `External` (no body
    // to infer), which still needs `meta`-only params/return below.
    let is_inferable_callable = matches!(
        info.kind,
        brink_ir::SymbolKind::Knot | brink_ir::SymbolKind::Stitch
    );
    let declared_sig = is_inferable_callable
        .then(|| db.signature(info.id))
        .flatten();
    let inferred_sig = is_inferable_callable
        .then(|| db.inferred_signature(info.id))
        .flatten();

    let params_str = if info.params.is_empty() {
        String::new()
    } else {
        let parts: Vec<_> = info
            .params
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let mut s = String::new();
                if p.is_ref {
                    s.push_str("ref ");
                }
                if p.is_divert {
                    s.push_str("-> ");
                }
                s.push_str(&p.name);
                if let Some(ty) = meta
                    .and_then(|m| m.params.get(i))
                    .and_then(|rp| rp.ty.as_ref())
                {
                    let _ = write!(s, ": {}", ty.name);
                } else if let Some(ty) = declared_sig
                    .as_ref()
                    .and_then(|sig| sig.param_annotations.get(i))
                    .and_then(|opt| opt.as_ref())
                {
                    let _ = write!(s, ": {}", ty.display());
                } else if let Some(ty) = inferred_sig
                    .as_ref()
                    .and_then(|sig| sig.params.get(i))
                    .filter(|t| !t.is_unknown())
                {
                    let _ = write!(s, ": {}", ty.display());
                }
                s
            })
            .collect();
        format!("({})", parts.join(", "))
    };

    let ret_str = meta
        .and_then(|m| m.returns.as_ref())
        .map(|t| format!(" -> {}", t.name))
        .or_else(|| {
            declared_sig
                .as_ref()
                .and_then(|sig| sig.return_annotation.as_ref())
                .map(|t| format!(" -> {}", t.display()))
        })
        .or_else(|| {
            inferred_sig
                .as_ref()
                .map(|sig| &sig.return_ty)
                .filter(|t| !t.is_unknown())
                .map(|t| format!(" -> {}", t.display()))
        })
        .unwrap_or_default();

    (params_str, ret_str)
}

/// TM-5 (#621): the inferred (or declared-but-not-`symbol_meta`-tracked)
/// type suffix for a `Param`/`Temp` symbol — `symbol_meta` never carries
/// these (it only covers knots/stitches/externals/VAR/CONST/List). For a
/// `Param`, the enclosing knot/stitch's declared annotation (matched by
/// position, same source `signature_strs` reads) still wins over
/// inference. Either way, falls back to the enclosing callable's inferred
/// body locals (`params ∪ temps`, keyed by name) so hovering an
/// unannotated parameter or `temp` still shows a type instead of nothing.
/// Empty string when nothing resolves (including `Unknown` — showing that
/// would be noise, not information) or `info` isn't a `Param`/`Temp`.
fn inferred_local_type_str(
    analysis: &AnalysisResult,
    db: &ProjectDb,
    info: &brink_ir::SymbolInfo,
) -> String {
    if !matches!(
        info.kind,
        brink_ir::SymbolKind::Temp | brink_ir::SymbolKind::Param
    ) {
        return String::new();
    }
    let enclosing = enclosing_callable(analysis, info);
    let declared = (info.kind == brink_ir::SymbolKind::Param)
        .then(|| enclosing.and_then(|def| db.signature(def)))
        .flatten()
        .and_then(|sig| {
            let idx = sig.params.iter().position(|p| p.name == info.name)?;
            sig.param_annotations.get(idx)?.clone()
        });
    declared
        .or_else(|| {
            enclosing
                .and_then(|def| db.infer_body(def))
                .and_then(|body| body.locals.get(&info.name).cloned())
                .filter(|ty| !ty.is_unknown())
        })
        .map(|ty| format!(": {}", ty.display()))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use rowan::TextSize;

    use super::hover;
    use crate::session::IdeSession;

    /// Hover content for the first occurrence of `needle` in `src`.
    fn hover_at(src: &str, needle: &str) -> String {
        let mut session = IdeSession::new();
        let file_id = session.update_and_analyze("test.ink", src.to_string());
        let analysis = session.analysis().expect("analysis");
        let pos = u32::try_from(src.find(needle).expect("needle present")).expect("offset");
        hover(
            analysis,
            session.db(),
            file_id,
            src,
            TextSize::from(pos),
            &[],
        )
        .expect("hover")
        .content
    }

    #[test]
    fn hover_shows_function_knot_doc_and_types() {
        let src = "\
/// Damage roll for an attack.
/// @param weapon {int}
/// @returns {int}
== function damage(weapon) ==
~ return weapon
";
        let content = hover_at(src, "damage(weapon)");
        assert!(content.contains("**knot**"), "{content}");
        assert!(content.contains("weapon: int"), "{content}");
        assert!(content.contains("-> int"), "{content}");
        assert!(content.contains("[function]"), "{content}");
        assert!(content.contains("Damage roll for an attack."), "{content}");
    }

    #[test]
    fn hover_shows_var_inferred_type_and_doc() {
        let src = "/// Player health.\nVAR health = 100\n-> END\n";
        let content = hover_at(src, "health = 100");
        assert!(content.contains("`health: int`"), "{content}");
        assert!(content.contains("Player health."), "{content}");
        assert!(
            !content.contains(" = 100"),
            "VARs don't show values: {content}"
        );
    }

    #[test]
    fn hover_shows_const_type_and_value() {
        let src = "CONST SPEED = 0.5\n-> END\n";
        let content = hover_at(src, "SPEED");
        assert!(content.contains("`SPEED: float = 0.5`"), "{content}");
    }

    // ── T2-4 (docs/effects-spec.md §10, issue #863): hover shows a
    // knot/stitch's inferred effect row ──────────────────────────────────

    #[test]
    fn hover_shows_inferred_effect_row_for_a_knot() {
        let src = "\
VAR gold = 0
EXTERNAL play_sfx(x)
== function spend(cost) ==
~ gold = gold - cost
~ play_sfx(cost)
~ return gold
";
        let content = hover_at(src, "spend(cost)");
        assert!(
            content.contains("*effects:* reads gold; writes gold; calls play_sfx"),
            "{content}"
        );
    }

    #[test]
    fn hover_shows_pure_for_a_genuinely_pure_knot() {
        let src = "== function double(x) ==\n~ return x * 2\n";
        let content = hover_at(src, "double(x)");
        assert!(content.contains("*effects:* pure"), "{content}");
    }

    #[test]
    fn hover_shows_opaque_for_a_call_through_a_function_value() {
        let src = "\
== function apply(f, x) ==
~ return f(x)
";
        let content = hover_at(src, "apply(f, x)");
        assert!(
            content.contains(
                "*effects:* opaque (calls through a function value, or an unresolved callee)"
            ),
            "{content}"
        );
    }

    #[test]
    fn hover_omits_effects_for_a_variable() {
        let src = "VAR gold = 0\n-> END\n";
        let content = hover_at(src, "gold = 0");
        assert!(!content.contains("*effects:*"), "{content}");
    }

    #[test]
    fn hover_shows_stitch_doc() {
        let src = "\
== hub ==
intro
/// The market square.
= market
stalls
";
        let content = hover_at(src, "market\n");
        assert!(content.contains("**stitch**"), "{content}");
        assert!(content.contains("The market square."), "{content}");
    }

    #[test]
    fn hover_shows_inline_types_kind_and_doc() {
        let src = "/// Whether the player holds an item.\n/// @param item {bool}\n/// @returns {bool}\n/// @kind query\nEXTERNAL holds(item)\n-> END\n";
        let mut session = IdeSession::new();
        let file_id = session.update_and_analyze("test.ink", src.to_string());
        let analysis = session.analysis().expect("analysis");

        let pos = u32::try_from(src.find("holds(item)").expect("decl present")).expect("offset");
        let info = hover(
            analysis,
            session.db(),
            file_id,
            src,
            TextSize::from(pos),
            &[],
        )
        .expect("hover");
        assert!(info.content.contains("item: bool"), "{}", info.content);
        assert!(info.content.contains("-> bool"), "{}", info.content);
        assert!(info.content.contains("[query]"), "{}", info.content);
        assert!(
            info.content.contains("Whether the player holds an item."),
            "{}",
            info.content
        );
    }

    // ── Stdlib slice 1 hover (#589) ─────────────────────────────────────

    #[test]
    fn hover_shows_stdlib_pure_function_signature_and_semantics() {
        let src = "~ temp n = len(inventory)\n-> END\n";
        let content = hover_at(src, "len(inventory)");
        assert!(content.contains("**brink stdlib**"), "{content}");
        assert!(content.contains("len(x) -> int"), "{content}");
        assert!(content.contains("keys in a map"), "{content}");
    }

    #[test]
    fn hover_shows_stdlib_mutator_with_lvalue_signature() {
        let src = "~ push(inventory, \"sword\")\n-> END\n";
        let content = hover_at(src, "push(inventory");
        assert!(
            content.contains("push(a: lvalue, v)"),
            "shows the lvalue-mutator signature: {content}"
        );
        assert!(content.contains("mutates its first argument"), "{content}");
    }

    #[test]
    fn hover_stdlib_name_is_available_even_when_unresolved() {
        // No `inventory` symbol declared at all — hover on `contains` must
        // still explain the stdlib function rather than falling through to
        // nothing, mirroring `builtin_hover_text`'s unconditional shape.
        let src = "~ temp ok = contains(items, 1)\n-> END\n";
        let content = hover_at(src, "contains(items");
        assert!(content.contains("contains(x, v) -> bool"), "{content}");
        assert!(
            content.contains("totality") || content.contains("total:"),
            "{content}"
        );
    }

    #[test]
    fn hover_non_stdlib_word_has_no_stdlib_content() {
        let src = "~ temp x = 1\n-> END\n";
        let mut session = IdeSession::new();
        let file_id = session.update_and_analyze("test.ink", src.to_string());
        let analysis = session.analysis().expect("analysis");
        let pos = u32::try_from(src.find("temp x").expect("present") + 5).expect("offset");
        let info = hover(
            analysis,
            session.db(),
            file_id,
            src,
            TextSize::from(pos),
            &[],
        );
        assert!(
            info.is_none() || !info.expect("checked").content.contains("brink stdlib"),
            "`x` is not a stdlib name"
        );
    }

    // ── TM-5 (#621): inferred-type hover for unannotated params/temps ────

    #[test]
    fn hover_shows_inferred_type_for_an_unannotated_temp() {
        let src = "=== function heal(hp) ===\n~ temp bonus = hp + 1\n~ return bonus\n";
        let content = hover_at(src, "bonus = hp");
        assert!(content.contains("**temp variable**"), "{content}");
        assert!(content.contains("`bonus: int`"), "{content}");
    }

    #[test]
    fn hover_shows_inferred_type_for_an_unannotated_param_at_a_use_site() {
        // Hovering `hp` *inside the body* (not the declaration) resolves
        // through `analysis.resolutions` to the same `Param` symbol.
        let src = "=== function heal(hp) ===\n~ temp bonus = hp + 1\n~ return bonus\n";
        let content = hover_at(src, "hp + 1");
        assert!(content.contains("**parameter**"), "{content}");
        assert!(content.contains("`hp: int`"), "{content}");
    }

    #[test]
    fn hover_annotation_wins_over_inference_for_a_param() {
        // `hp` is annotated `string` but the body only ever uses it as an
        // int (`hp + 1`) — the declared annotation must still be what
        // hover shows, never the (disagreeing) inferred body type.
        let src = "=== function heal(hp: string) ===\n~ temp bonus = hp + 1\n~ return bonus\n";
        let content = hover_at(src, "heal(hp: string)");
        assert!(content.contains("hp: string"), "{content}");
        assert!(!content.contains("hp: int"), "{content}");
    }

    #[test]
    fn hover_falls_back_to_inferred_signature_for_an_unannotated_knot_header() {
        // No TM-2 annotations anywhere — `symbol_meta` has no param/return
        // type for `heal` at all, so hover must fall back to
        // `db.inferred_signature`.
        let src = "=== function heal(hp) ===\n~ return hp + 1\n";
        let content = hover_at(src, "heal(hp)");
        assert!(content.contains("hp: int"), "{content}");
        assert!(content.contains("-> int"), "{content}");
    }

    // ── T1c-4 (#702): fn-value slot hover shows the bound signature ──────

    #[test]
    fn hover_shows_the_bound_signature_for_a_fn_value_var_slot() {
        let src = "\
VAR player_hp = 10
VAR healer = 0

~ healer = #fn(heal, player_hp)
-> END

=== function heal(ref hp, amount) ===
~ hp = hp + amount
~ return hp
";
        let content = hover_at(src, "healer = 0");
        assert!(content.contains("**variable**"), "{content}");
        assert!(
            content.contains("fn heal(ref hp = player_hp, amount)"),
            "{content}"
        );
    }

    #[test]
    fn hover_shows_no_bound_signature_for_an_ordinary_var() {
        let src = "VAR health = 100\n-> END\n";
        let content = hover_at(src, "health = 100");
        assert!(!content.contains("fn "), "{content}");
    }

    #[test]
    fn hover_shows_no_type_suffix_when_inference_cannot_resolve_one() {
        // `x` is a parameter never used in the body — inference can't pin
        // it down (stays `Unknown`), and hover must show nothing rather
        // than a noisy `x: Unknown`.
        let src = "=== function f(x) ===\n~ return 1\n";
        let content = hover_at(src, "f(x)");
        assert!(!content.contains("Unknown"), "{content}");
        assert!(!content.contains("x:"), "{content}");
    }
}
