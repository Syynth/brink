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
    // B3a UFCS resolution (issue #1507): a method-call-shaped callee's
    // `ResolutionMap` entry spans the whole `recv.verb` range and targets
    // the *receiver* (see `crate::ufcs_hover`'s module doc) — checked first
    // and narrowly (only the method segment's own range), so it overrides
    // the receiver-shaped hover below exactly where that would otherwise be
    // misleading, and is a no-op everywhere else (including hovering `recv`
    // itself, which still falls through to the generic path).
    if let Some(hir) = db.hir(file_id)
        && let Some(info) = crate::ufcs_hover::ufcs_hover(db, hir, file_id, offset, project_files)
    {
        return Some(info);
    }

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
                if let Some(ty) = &v.ty {
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

        // T2-4 (#863, docs/effects-spec.md §10): a knot/stitch's *inferred*
        // effect row — the boring, stable reads/writes/calls display. Only
        // knots/stitches have a `DefinitionId → row` (`db.effects` is `None`
        // for every other symbol), so this suffix is empty everywhere else.
        // Purely advisory: it *shows* the row; the only contract is the
        // optional `#@effects` assertion (checked in the analyzer, `E103`).
        let effect_row_str = matches!(
            info.kind,
            brink_ir::SymbolKind::Knot | brink_ir::SymbolKind::Stitch
        )
        .then(|| db.effects(info.id))
        .flatten()
        .map_or(String::new(), |row| {
            let view = crate::effects::EffectRowView::from_row(&row, &analysis.index);
            format!("\n\n**effects** `{}`", view.display_line())
        });

        format!(
            "**{kind_str}** `{}{inferred_local_str}{value_str}{params_str}{ret_str}`{detail_str}{kind_tag}{doc_block}{file_note}{fn_value_str}{effect_row_str}",
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

/// Honest hover display for a resolved semantic type (#1027 — closes the
/// #1004 divergence). `resolve_type` (`external_check`) still builds a
/// [`brink_analyzer::ResolvedType`] for a semantic-type name that isn't
/// registered in the host manifest — `name` is the bare written name (so
/// callers keep it for the diagnostic message / partial rendering), but
/// [`brink_analyzer::ResolvedType::is_registered`] is `false`. Rendering
/// that bare name with no qualifier is exactly what made #1004 look like a
/// strict-inference bug: hover showed `id: var_id` with full confidence
/// while inference correctly resolved the same unregistered name to
/// `Unknown`. A registered type (base keyword or a manifest-registered
/// name) still renders as the bare name; an unregistered one gets an
/// explicit warning marker and an `E040` cross-reference so a reader can't
/// mistake it for a checked, host-backed type.
pub(crate) fn honest_type_display(ty: &brink_analyzer::ResolvedType) -> String {
    if ty.is_registered() {
        ty.name.clone()
    } else {
        format!("{} ⚠ unregistered semantic type — E040", ty.name)
    }
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
                    let _ = write!(s, ": {}", honest_type_display(ty));
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
        .map(|t| format!(" -> {}", honest_type_display(t)))
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
/// these (it only covers knots/stitches/externals/VAR/CONST/List). The
/// local's own TM-2 `: type` annotation (`db.local_signature`, issue #530
/// — the per-file locals path `signature`/`db.signature` itself can't
/// take) still wins over inference for *both* a `Param` and a `Temp`
/// (before #530 only a `Param`'s annotation reached hover, read positionally
/// off the *enclosing* knot/stitch's own `signature`; a `~ temp x: type`
/// ascription was silently skipped straight to inference). Either way,
/// falls back to the enclosing callable's inferred body locals (`params ∪
/// temps`, keyed by name) so hovering an unannotated parameter or `temp`
/// still shows a type instead of nothing. Empty string when nothing
/// resolves (including `Unknown` — showing that would be noise, not
/// information) or `info` isn't a `Param`/`Temp`.
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
    let declared = db
        .local_signature(info.file, info.id)
        .and_then(|sig| sig.value_ty.clone());
    declared
        .or_else(|| {
            enclosing_callable(analysis, info)
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

    /// Like `hover_at`, but for a native `.brink` fixture (B3a UFCS
    /// resolution is native-only — see `crate::ufcs_hover`'s module doc).
    fn hover_at_native(src: &str, needle: &str) -> Option<String> {
        let mut session = IdeSession::new();
        let file_id = session.update_and_analyze("test.brink", src.to_string());
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
        .map(|info| info.content)
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
    fn hover_annotation_wins_over_inference_for_a_temp() {
        // Before #530 a `~ temp x: type` ascription was skipped straight to
        // inferred-body display; the declared annotation (`step: float`)
        // must be what hover shows for the *declaration itself*, not the
        // inferred int type of the literal `1`.
        let src = "=== quest ===\n~ temp step: float = 1\nOnward.\n-> END\n";
        let content = hover_at(src, "step: float");
        assert!(content.contains("step: float"), "{content}");
        assert!(!content.contains("step: int"), "{content}");
    }

    #[test]
    fn hover_shows_an_annotated_params_declared_type_at_a_use_site() {
        // Hovering `hp` *inside the body* (not the declaration) must still
        // resolve to the param's own declared annotation via
        // `db.local_signature`, not the enclosing knot header's
        // `signature_strs` path.
        let src = "=== function heal(hp: string) ===\n~ temp bonus = hp + 1\n~ return bonus\n";
        let content = hover_at(src, "hp + 1");
        assert!(content.contains("hp: string"), "{content}");
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

    // ── T2-4 (#863): inferred effect row in hover ───────────────────────

    #[test]
    fn hover_shows_a_knots_inferred_effect_row() {
        let src = "\
VAR gold = 10
EXTERNAL PlaySound(id)
-> spend

=== spend ===
~ gold = gold - 1
~ PlaySound(1)
Spent.
-> END
";
        let content = hover_at(src, "spend ===");
        assert!(content.contains("**knot**"), "{content}");
        assert!(content.contains("**effects**"), "{content}");
        assert!(content.contains("reads: gold"), "{content}");
        assert!(content.contains("writes: gold"), "{content}");
        assert!(content.contains("calls: PlaySound"), "{content}");
    }

    #[test]
    fn hover_shows_pure_for_an_effectless_knot() {
        let src = "=== function double(n) ===\n~ return n + n\n";
        let content = hover_at(src, "double(n)");
        assert!(
            content.contains("**effects** `pure, silent, total`"),
            "{content}"
        );
    }

    #[test]
    fn hover_shows_no_effect_row_for_a_non_callable_symbol() {
        // A VAR is not a knot/stitch — `db.effects` is `None`, so no effect
        // row suffix appears (only knots/stitches ship a row, spec §10).
        let src = "VAR health = 100\n-> END\n";
        let content = hover_at(src, "health = 100");
        assert!(!content.contains("**effects**"), "{content}");
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

    // ── Issue #1027: hover must be honest about an unregistered semantic
    // type, not render it with the same confidence as a registered one ────

    fn actor_id_type() -> brink_ir::SemanticTypeDef {
        brink_ir::SemanticTypeDef {
            name: "actor_id".to_string(),
            base: brink_ir::BaseType::String,
            constraint: None,
            values: None,
            widget: None,
        }
    }

    #[test]
    fn hover_renders_unregistered_semantic_type_with_a_warning_not_a_bare_name() {
        // The #1004 divergence, reproduced directly: `var_id` is named in an
        // inline `@param` doc, but the registered manifest only defines a
        // *sibling* type (`actor_id`) — the vocabulary genuinely reached the
        // analyzer, `var_id` just isn't in it. Hover must not render
        // `id: var_id` with the same bare confidence a registered type gets.
        let src = "/// @param id {var_id}\nEXTERNAL get_variable(id)\n-> DONE\n";
        let mut session = IdeSession::new();
        let file_id = session.update_and_analyze("test.ink", src.to_string());
        session.set_host_manifest(brink_ir::HostManifest {
            externals: vec![],
            types: vec![actor_id_type()],
        });
        let analysis = session.analysis().expect("analysis");
        let pos =
            u32::try_from(src.find("get_variable(id)").expect("decl present")).expect("offset");
        let content = hover(
            analysis,
            session.db(),
            file_id,
            src,
            TextSize::from(pos),
            &[],
        )
        .expect("hover")
        .content;

        assert!(
            !content.contains("id: var_id)"),
            "must not render var_id with bare, unqualified confidence: {content}"
        );
        assert!(
            content.contains("var_id"),
            "still shows the written name so the author can spot the typo: {content}"
        );
        assert!(
            content.contains('\u{26A0}'),
            "must carry an explicit warning marker: {content}"
        );
        assert!(
            content.contains("E040"),
            "must cross-reference the E040 diagnostic code: {content}"
        );
    }

    #[test]
    fn hover_renders_registered_semantic_type_with_no_warning() {
        // Same shape as the unregistered case above, but `id`'s type
        // (`actor_id`) IS registered — hover must render it exactly as
        // before, no warning noise on a genuinely resolved type.
        let src = "/// @param id {actor_id}\nEXTERNAL get_variable(id)\n-> DONE\n";
        let mut session = IdeSession::new();
        let file_id = session.update_and_analyze("test.ink", src.to_string());
        session.set_host_manifest(brink_ir::HostManifest {
            externals: vec![],
            types: vec![actor_id_type()],
        });
        let analysis = session.analysis().expect("analysis");
        let pos =
            u32::try_from(src.find("get_variable(id)").expect("decl present")).expect("offset");
        let content = hover(
            analysis,
            session.db(),
            file_id,
            src,
            TextSize::from(pos),
            &[],
        )
        .expect("hover")
        .content;

        assert!(
            content.contains("id: actor_id)"),
            "a registered type still renders as the bare name: {content}"
        );
        assert!(
            !content.contains('\u{26A0}'),
            "no warning marker for a registered type: {content}"
        );
    }

    // ── Issue #1507: UFCS hover reports the D2-resolved target ───────────

    const UFCS_FREE_FN_SRC: &str = "\
struct Guest {
  name: string
}

fn greet(g, loudness) {
  return loudness;
}

fn main() {
  let g = Guest { name: \"ada\" };
  let n = g.greet(3);
}
";

    #[test]
    fn hover_on_a_ufcs_method_segment_shows_the_free_fn_verdict() {
        let content =
            hover_at_native(UFCS_FREE_FN_SRC, "greet(3)").expect("hover on the method segment");
        assert!(content.contains("**free function**"), "{content}");
        assert!(content.contains("greet(g, "), "{content}");
        assert!(content.contains("Desugared from `g.greet(…)`"), "{content}");
    }

    #[test]
    fn hover_on_the_receiver_segment_of_a_ufcs_call_is_unaffected() {
        // Hovering `g` itself (before the dot) must keep showing the
        // receiver, not the method verdict — the override is narrowly
        // scoped to the method segment (`crate::ufcs_hover`'s module doc).
        let content =
            hover_at_native(UFCS_FREE_FN_SRC, "g.greet(3)").expect("hover on the receiver");
        assert!(
            !content.contains("**free function**"),
            "receiver hover must not show the UFCS verdict: {content}"
        );
    }

    #[test]
    fn hover_on_a_ufcs_prelude_desugar_reuses_the_stdlib_hover_text() {
        let src = "\
struct Guest {
  name: string
}

fn main() {
  let g = Guest { name: \"ada\" };
  let n = g.len();
}
";
        let content = hover_at_native(src, "len()").expect("hover on the method segment");
        assert!(content.contains("**brink stdlib**"), "{content}");
        assert!(content.contains("len(x) -> int"), "{content}");
        assert!(content.contains("desugared from `g.len(…)`"), "{content}");
    }

    #[test]
    fn hover_on_a_ufcs_field_call_shows_the_field_call_verdict() {
        // Fixture mirrors `brink-analyzer`'s
        // `a_function_typed_field_wins_and_is_recorded_as_a_field_call`
        // (crates/internal/brink-analyzer/tests/ufcs_resolution.rs) — a
        // function-typed field wins over a same-named free function (D1).
        let src = "\
struct Guest {
  greet: fn(int): int
}

fn main() {
  let g = Guest { greet: \"hi\" };
  let n = g.greet(3);
}
";
        let content = hover_at_native(src, "greet(3)").expect("hover on the method segment");
        assert!(content.contains("**field call**"), "{content}");
        assert!(content.contains("g.greet(…)"), "{content}");
    }

    #[test]
    fn hover_on_a_ufcs_free_fn_auto_ref_shows_the_by_ref_verdict() {
        // Fixture mirrors `brink-analyzer`'s
        // `a_ref_first_param_auto_refs_a_frame_local_receiver`
        // (crates/internal/brink-analyzer/tests/ufcs_resolution.rs) — `bump`'s
        // first parameter is `ref`, so the desugar passes the receiver by
        // reference (D5 auto-ref, issue #1462).
        let src = "\
fn bump(ref n, amount) {
  n = n + amount;
}

fn main() {
  let g = 1;
  g.bump(5);
}
";
        let content = hover_at_native(src, "bump(5)").expect("hover on the method segment");
        assert!(content.contains("**free function (by ref)**"), "{content}");
        assert!(content.contains("bump(ref g, …)"), "{content}");
        assert!(content.contains("passed by reference"), "{content}");
    }
}
