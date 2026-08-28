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
        let ctx = SectionCtx {
            analysis,
            db,
            file_id,
            info,
            meta: analysis.symbol_meta.get(&info.id),
        };
        let mut blocks = vec![head_line(&ctx)];
        for section in HOVER_SECTIONS {
            if let Some(block) = section(&ctx) {
                blocks.push(block);
            }
        }
        if let Some(note) = defined_in_section(info, project_files) {
            blocks.push(note);
        }
        blocks.join("\n\n")
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

// ── Hover sections — the flexible dispatch (#3054 review) ───────────
//
// The hover is one HEAD line (kind + name + signature/value) followed by
// zero-or-more Markdown BLOCKS, joined with blank lines. Every per-kind
// enrichment is a section provider in `HOVER_SECTIONS`; adding hover
// content means adding a provider here, not growing a format! call.

/// Everything a section provider may consult.
struct SectionCtx<'a> {
    analysis: &'a AnalysisResult,
    db: &'a ProjectDb,
    file_id: FileId,
    info: &'a brink_ir::SymbolInfo,
    meta: Option<&'a brink_analyzer::SymbolMeta>,
}

/// Ordered section providers. Each returns one Markdown block or `None`.
const HOVER_SECTIONS: &[fn(&SectionCtx) -> Option<String>] = &[
    doc_section,
    list_members_section,
    fn_value_section,
    effect_row_section,
    style_section,
];

/// The head line — bold kind, then the name with its type/value/signature
/// in a code span, then detail/external-kind tags.
fn head_line(ctx: &SectionCtx) -> String {
    let info = ctx.info;
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
    let (params_str, ret_str) = signature_strs(info, ctx.meta, ctx.db);
    let inferred_local_str = inferred_local_type_str(ctx.analysis, ctx.db, info);

    // Initializer info: `health: int`, `SPEED: float = 0.5`.
    let value_str = ctx
        .meta
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
    let kind_tag = ctx.meta.map_or(String::new(), |m| match m.kind {
        brink_ir::ExternalKind::Plain => String::new(),
        brink_ir::ExternalKind::Query => " [query]".to_string(),
        brink_ir::ExternalKind::Effect => " [effect]".to_string(),
        brink_ir::ExternalKind::Presentation => " [presentation]".to_string(),
    });
    let detail_str = info
        .detail
        .as_deref()
        .map_or(String::new(), |d| format!(" [{d}]"));

    format!(
        "**{kind_str}** `{}{inferred_local_str}{value_str}{params_str}{ret_str}`{detail_str}{kind_tag}",
        info.name
    )
}

/// The symbol's `///` doc block.
fn doc_section(ctx: &SectionCtx) -> Option<String> {
    ctx.meta.and_then(|m| m.doc.clone())
}

/// LIST / list-item member set — see [`list_members_hover`].
fn list_members_section(ctx: &SectionCtx) -> Option<String> {
    let s = list_members_hover(ctx.db, ctx.info);
    (!s.is_empty()).then_some(s)
}

/// T1c-4 (#702): a fn-value slot's bound-signature display form.
fn fn_value_section(ctx: &SectionCtx) -> Option<String> {
    ctx.db
        .hir(ctx.file_id)
        .and_then(|hir| crate::fn_value_hover::fn_value_slot_signature(ctx.analysis, hir, ctx.info))
        .map(|sig| format!("`{sig}`"))
}

/// T2-4 (#863): a knot/stitch's inferred effect row — advisory display.
fn effect_row_section(ctx: &SectionCtx) -> Option<String> {
    matches!(
        ctx.info.kind,
        brink_ir::SymbolKind::Knot | brink_ir::SymbolKind::Stitch
    )
    .then(|| ctx.db.effects(ctx.info.id))
    .flatten()
    .map(|row| {
        let view = crate::effects::EffectRowView::from_row(&row, &ctx.analysis.index);
        format!("**effects** `{}`", view.display_line())
    })
}

/// #1719: a native knot/stitch's own `@[style(...)]` annotation.
fn style_section(ctx: &SectionCtx) -> Option<String> {
    ctx.db
        .hir(ctx.file_id)
        .and_then(|hir| crate::style_hover::style_hover_text(hir, ctx.info))
        .map(|s| s.trim_start().to_string())
}

/// The trailing *Defined in `path`* note — placed last, outside the table
/// (it needs `project_files`, which sections don't).
fn defined_in_section(
    info: &brink_ir::SymbolInfo,
    project_files: &[(FileId, String, String)],
) -> Option<String> {
    project_files
        .iter()
        .find(|(fid, _, _)| *fid == info.file)
        .map(|(_, p, _)| format!("*Defined in `{p}`*"))
}

/// The member-set block for a LIST or list-item hover; empty for every
/// other symbol kind. The list is looked up in ITS defining file's HIR by
/// the (unqualified) list name; a hovered member renders bold.
fn list_members_hover(db: &ProjectDb, info: &brink_ir::SymbolInfo) -> String {
    let (list_name, hovered_member) = match info.kind {
        brink_ir::SymbolKind::List => (info.name.as_str(), None),
        brink_ir::SymbolKind::ListItem => {
            let mut parts = info.name.splitn(2, '.');
            let list = parts.next().unwrap_or("");
            (list, parts.next())
        }
        _ => return String::new(),
    };
    let Some(hir) = db.hir(info.file) else {
        return String::new();
    };
    let Some(list) = hir.lists.iter().find(|l| l.name.text == list_name) else {
        return String::new();
    };
    // Every member shows its numeric value, the defaulted ones included —
    // ink's ordinal rule: count from 1, an explicit value resets the
    // counter (`a, b = 5, c` → 1, 5, 6).
    let mut next_ordinal: i32 = 1;
    let rendered: Vec<String> = list
        .members
        .iter()
        .map(|m| {
            let value = m.value.unwrap_or(next_ordinal);
            next_ordinal = value.saturating_add(1);
            let mut t = format!("{} = {value}", m.name.text);
            if m.is_active {
                t = format!("({t})");
            }
            let code = format!("`{t}`");
            if hovered_member == Some(m.name.text.as_str()) {
                format!("**{code}**")
            } else {
                code
            }
        })
        .collect();
    format!(
        "

**LIST** `{}` — {}",
        list.name.text,
        rendered.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use rowan::TextSize;

    use brink_ir::FileId;

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

    /// Load `src` into a session the way a real native project reaches it —
    /// through a `brink.toml` declaring `dialect = "brink"`, parsed by
    /// [`brink_project_config::parse_str`] and applied via
    /// [`brink_analyzer::AnalysisOptions::apply_project_config`] +
    /// [`IdeSession::apply_analysis_options`] — instead of a bare,
    /// unconfigured `IdeSession::new()`.
    ///
    /// Issue #2885: a bare session's `language_dialect` defaults to
    /// `Dialect::StrictInk` (`AnalysisOptions::default()`), which is not
    /// what a native project actually resolves to once it opts into the
    /// native typed-mode track (`dialect = "brink"` in `brink.toml`) — the
    /// same seam `brink-cli`'s `Project::ide_session()` and `brink-web`'s
    /// `EditorSession::apply_parsed_config` both funnel through. A test
    /// named "native" that never goes through this seam is exercising a
    /// configuration no such author runs, the same class of gap #2324 (the
    /// playground never applying `brink.toml` at all) already burned this
    /// project on.
    ///
    /// Also fixes the db-direct road for free: `apply_analysis_options`
    /// re-analyzes through [`IdeSession`]'s `reanalyze`, which syncs the
    /// session's resolved options into its own `ProjectDb`
    /// ([`IdeSession::sync_db_options`] — issue #1553) — unlike
    /// `update_and_analyze` alone, which does not (see that method's own
    /// doc comment, and `tests/live_typing_db_divergence.rs`).
    fn native_session(path: &str, src: &str) -> (IdeSession, FileId) {
        let mut session = IdeSession::new();
        let file_id = session.update_source(path, src.to_string());

        let (config, config_warnings) =
            brink_project_config::parse_str("[project]\ndialect = \"brink\"\n")
                .expect("hand-written brink.toml literal must parse");
        assert!(
            config_warnings.is_empty(),
            "unexpected brink.toml warnings: {config_warnings:?}"
        );
        let mut options = brink_analyzer::AnalysisOptions::default();
        let apply_warnings = options.apply_project_config(&config, false, false);
        assert!(
            apply_warnings.is_empty(),
            "unexpected apply_project_config warnings: {apply_warnings:?}"
        );
        session.apply_analysis_options(&options);

        // Assert the dialect this helper exists to establish (per review
        // on #2885): if the config plumbing above or
        // `apply_analysis_options`'s change guard regresses, every
        // "native" hover test would silently fall back to
        // `Dialect::StrictInk` with no test noticing — the exact
        // silent-divergence class #2885 was filed about, reintroduced one
        // layer up. Pin both roads: the session's own resolved dialect,
        // and the db-direct road's copy that `apply_analysis_options`'s
        // doc comment claims `sync_db_options` keeps in step "for free".
        assert_eq!(session.language_dialect(), brink_analyzer::Dialect::Brink);
        assert_eq!(
            session.db().analysis_options().dialect,
            brink_analyzer::Dialect::Brink
        );

        (session, file_id)
    }

    /// Like `hover_at`, but for a native `.brink` fixture (B3a UFCS
    /// resolution is native-only — see `crate::ufcs_hover`'s module doc),
    /// analyzed through [`native_session`] rather than a bare session
    /// (issue #2885).
    fn hover_at_native(src: &str, needle: &str) -> Option<String> {
        let (session, file_id) = native_session("test.brink", src);
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
    fn list_and_item_hovers_show_the_member_set() {
        // #3054 review: hovering a list item (or the list) shows every
        // member — declared order, ordinals and default-active parens
        // preserved, the hovered member bold.
        let src = "LIST Boon = blessed, (cursed), spare = 5
~ temp b = Boon.cursed
";
        let item = hover_at(src, "Boon.cursed");
        assert!(item.contains("**LIST** `Boon`"), "{item}");
        // Defaulted ordinals render too — ink's rule: count from 1, an
        // explicit value resets the counter.
        assert!(item.contains("**`(cursed = 2)`**"), "{item}");
        assert!(item.contains("`blessed = 1`"), "{item}");
        assert!(item.contains("`spare = 5`"), "{item}");

        let list = hover_at(src, "Boon =");
        assert!(list.contains("**LIST** `Boon`"), "{list}");
        // No member is bold on the list's own hover.
        assert!(!list.contains("**`"), "{list}");
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
    fn the_rng_cell_is_named_rng_not_a_raw_handle() {
        // Shipped bug: hovering any function that calls `RANDOM` showed
        // `writes: GlobalVar(0x5eed0000d1ce)` — the compiler-owned RNG cell
        // has no symbol-index entry, so the name lookup fell through to the
        // id's debug form and put a raw internal handle in author-facing UI.
        //
        // `rng` is the spelling the assertion surface uses
        // (`@[effects(writes rng)]`), so the name an author reads here is
        // the name they would write.
        let content = hover_at(
            "=== function roll(lo, hi) ===\n~ return RANDOM(lo, hi)\n",
            "roll",
        );
        assert!(content.contains("writes: rng"), "{content}");
        assert!(
            !content.contains("GlobalVar("),
            "raw handle leaked: {content}"
        );
        assert!(!content.contains("0x"), "raw handle leaked: {content}");
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
            markup: Vec::new(),
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
            markup: Vec::new(),
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

    // ── Issue #2864: a shadowing declaration wins over the name-keyed
    // builtin table, on both analysis roads ──────────────────────────────
    //
    // #2856/PR #2859 fixed the *compiler* so a declared symbol (`VAR
    // RANDOM`, a knot named `FLOOR`, …) shadows a same-named classic ink
    // builtin at resolution/lowering time. #2864's investigation (see the
    // issue's comments) found `hover()` above already resolves correctly on
    // both roads, for a reason that predates #2859 and needed no change:
    // `find_def_at_offset(analysis, …)` — which reads `analysis.resolutions`,
    // the very map #2859 fixed — is tried *before* the fallback to the
    // name-keyed `builtin_hover_text` table a few lines up in this file.
    // Nothing pinned that ordering as a regression, so a future refactor of
    // `hover()` could silently reintroduce exactly the defect #2864
    // originally (and wrongly) reported. These tests pin it.
    //
    // "Both roads" (CLAUDE.md): the off-db `IdeSnapshot::analyze` result
    // (`session.analysis()` — what the studio's live-typing/off-db path
    // uses) and the db-direct `ProjectDb::analysis()` result
    // (`session.db().analysis()` — what the studio's Problems panel
    // renders, per `ProjectDb::analysis`'s own doc comment). A change could
    // be correct on one and wrong on the other, so both are exercised
    // explicitly and separately below rather than picking one.
    //
    // Reached the same way a real hover request is: through the exact
    // `brink_ide::hover::hover` entry point that both
    // `crates/brink-web/src/editor/hover.rs`'s `EditorSession::hover` (the
    // studio) and `crates/brink-lsp/src/backend.rs`'s LSP `hover` handler
    // call — not a private helper.

    /// Assert hover at `needle` in `src` resolves to the author's own
    /// declaration — never falling through to `builtin_hover_text`'s
    /// name-keyed table — on both analysis roads.
    ///
    /// Takes an already-built `(session, file_id)` pair rather than
    /// constructing one itself (issue #2885): the ink caller below still
    /// builds a bare `IdeSession::new()` (matching what an unconfigured
    /// `.ink` project actually runs under), while the native caller routes
    /// through [`native_session`] — the real `brink.toml`/
    /// `apply_analysis_options` configuration path a native project
    /// reaches `dialect = brink` through, rather than the same bare
    /// session under the wrong dialect.
    fn assert_shadowing_hover_wins_on_both_roads(
        session: &IdeSession,
        file_id: FileId,
        src: &str,
        needle: &str,
    ) {
        let pos = u32::try_from(src.find(needle).expect("needle present")).expect("offset");

        let off_db = session.analysis().expect("off-db (IdeSnapshot::analyze)");
        let off_db_content = hover(off_db, session.db(), file_id, src, TextSize::from(pos), &[])
            .expect("off-db hover")
            .content;
        assert!(
            off_db_content.contains("**knot**"),
            "off-db road (IdeSnapshot::analyze) must resolve to the author's own \
             declaration: {off_db_content}"
        );
        assert!(
            !off_db_content.contains("**built-in**"),
            "off-db road (IdeSnapshot::analyze) fell through to the name-keyed \
             builtin table despite the shadowing declaration: {off_db_content}"
        );

        let db_direct = session.db().analysis();
        let db_content = hover(
            db_direct,
            session.db(),
            file_id,
            src,
            TextSize::from(pos),
            &[],
        )
        .expect("db-direct hover")
        .content;
        assert!(
            db_content.contains("**knot**"),
            "db-direct road (ProjectDb::analysis) must resolve to the author's own \
             declaration: {db_content}"
        );
        assert!(
            !db_content.contains("**built-in**"),
            "db-direct road (ProjectDb::analysis) fell through to the name-keyed \
             builtin table despite the shadowing declaration: {db_content}"
        );
    }

    #[test]
    fn hover_on_a_shadowing_ink_function_knot_wins_over_the_builtin_table_on_both_roads() {
        // `FLOOR` is both a classic ink builtin (`text.rs`'s
        // `builtin_hover_text` table: "Round down to nearest integer") and,
        // here, an author-declared function knot. Hovering the call site
        // must show the author's own knot, never the builtin's doc.
        let src = "\
=== function FLOOR(x) ===
~ return x - 1

=== start ===
~ temp y = FLOOR(3)
Done.
-> END
";
        let mut session = IdeSession::new();
        let file_id = session.update_and_analyze("test.ink", src.to_string());
        assert_shadowing_hover_wins_on_both_roads(&session, file_id, src, "FLOOR(3)");
    }

    #[test]
    fn hover_on_a_shadowing_native_fn_wins_over_the_builtin_table_on_both_roads() {
        // Same shape as the ink case above, on the native `.brink` surface:
        // a free function named after a classic builtin, called directly
        // (not through UFCS method-call syntax, which `ufcs_hover` — a
        // different, narrower override — already covers separately).
        //
        // Analyzed through `native_session` (issue #2885), not a bare
        // `IdeSession::new()`: a native fixture named "native" must
        // actually reach `dialect = brink` the way a real native project
        // does, and the db-direct road must read the SAME resolved options
        // the off-db road does (`apply_analysis_options`, not
        // `update_and_analyze` alone — see `native_session`'s doc comment).
        let src = "\
fn FLOOR(x) {
  return x - 1;
}

flow main() {
  Sum: {FLOOR(3)} -> END
}
";
        let (session, file_id) = native_session("test.brink", src);
        assert_shadowing_hover_wins_on_both_roads(&session, file_id, src, "FLOOR(3)");
    }
}
