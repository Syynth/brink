//! `var`/`const`/`flags`/`struct`/`extern` → their HIR decl nodes
//! (`docs/b0-sequencing.md` §B0.6).
//!
//! The directive channels that *do* have native syntax are wired elsewhere:
//! the `@[…]` annotation channel in [`super::annotation`] (`@[effects(…)]`
//! → `effects_assertion` on a `flow`/`fn`, issue #1563) and the file-level
//! `@[was]` module-rename record in [`super::module`]. Neither has a ruled
//! meaning on the `var`/`const`/`flags`/`struct`/`extern` declarations this
//! module owns, so every decl node below carries `is_local`/`visibility`/
//! `was` as their empty default (`None`/`false`) — honest (no syntax exists
//! to consume) rather than fabricated. `///` docs ARE wired (B0.6b,
//! `docs/decision-log.md` 2026-07-20) — see [`super::doc_comment`].

use brink_syntax_native::SyntaxKind as N;
use brink_syntax_native::ast::{self, AstNode as _};
use brink_syntax_native::{SyntaxNode, SyntaxToken};

use crate::hir::FileId;
use crate::hir::doc_block::DocPolicy;
use crate::provenance::NodeClass;
use crate::{
    ConstDecl, Diagnostic, DiagnosticCode, ExternalDecl, ListDecl, ListMember, Name, ParamInfo,
    StructDecl, StructFieldDecl, TypeExpr, VarDecl,
};

use super::doc_comment::lower_doc_comment;
use super::expr::{lower_expr, lower_path};
use super::provenance::native_provenance;

fn diag(file: FileId, range: rowan::TextRange, code: DiagnosticCode) -> Diagnostic {
    Diagnostic {
        file,
        range,
        message: code.title().to_string(),
        code,
    }
}

fn name_from(tok: Option<SyntaxToken>) -> Option<Name> {
    tok.map(|t| Name {
        text: t.text().to_string(),
        range: t.text_range(),
    })
}

/// The `: type` annotation on a `var`/`const` binding or a `struct` field,
/// lowered to HIR (NG-B, issue #1488; reused for struct fields by NG-E,
/// issue #1505). `None` when unwritten — the same shape the ink dialect's
/// TM-2 `VAR x: int = …` produces, so an annotated native global is exempt
/// from `brink-analyzer::strict`'s `E065` Unknown-escape exactly as an
/// annotated ink one is.
fn binding_annotation(annotation: Option<&ast::TypeAnnotation>) -> Option<TypeExpr> {
    annotation.and_then(super::types::lower_type_annotation)
}

/// `var name = expr` / `var name: type = expr`.
pub(super) fn lower_var_decl(
    file_id: FileId,
    node: &ast::VarDecl,
    diags: &mut Vec<Diagnostic>,
) -> Option<VarDecl> {
    let range = node.syntax().text_range();
    let Some(name) = name_from(node.name_token()) else {
        diags.push(diag(file_id, range, DiagnosticCode::E004));
        return None;
    };
    let Some(value_node) = node.value() else {
        diags.push(diag(file_id, range, DiagnosticCode::E005));
        return None;
    };
    let value = lower_expr(file_id, &value_node, diags);
    let doc = lower_doc_comment(file_id, node.doc(), DocPolicy::VALUE, diags);
    Some(VarDecl {
        ptr: native_provenance(file_id, NodeClass::VarDecl, node.syntax()),
        name,
        value,
        is_local: false,
        annotation: binding_annotation(node.type_annotation().as_ref()),
        doc,
        visibility: None,
        was: None,
    })
}

/// `const name = expr` / `const name: type = expr`.
pub(super) fn lower_const_decl(
    file_id: FileId,
    node: &ast::ConstDecl,
    diags: &mut Vec<Diagnostic>,
) -> Option<ConstDecl> {
    let range = node.syntax().text_range();
    let Some(name) = name_from(node.name_token()) else {
        diags.push(diag(file_id, range, DiagnosticCode::E006));
        return None;
    };
    let Some(value_node) = node.value() else {
        diags.push(diag(file_id, range, DiagnosticCode::E007));
        return None;
    };
    let value = lower_expr(file_id, &value_node, diags);
    let doc = lower_doc_comment(file_id, node.doc(), DocPolicy::VALUE, diags);
    Some(ConstDecl {
        ptr: native_provenance(file_id, NodeClass::ConstDecl, node.syntax()),
        name,
        value,
        annotation: binding_annotation(node.type_annotation().as_ref()),
        doc,
        visibility: None,
        was: None,
    })
}

/// `flags Name = (member), member, …` — the charter's renamed `LIST`
/// (§11), reusing ink's `ListDecl`/`ListMember` HIR shape verbatim.
pub(super) fn lower_flags_decl(
    file_id: FileId,
    node: &ast::FlagsDecl,
    diags: &mut Vec<Diagnostic>,
) -> Option<ListDecl> {
    let range = node.syntax().text_range();
    let Some(name) = name_from(node.name_token()) else {
        diags.push(diag(file_id, range, DiagnosticCode::E008));
        return None;
    };
    let members: Vec<ListMember> = node
        .member_list()
        .into_iter()
        .flat_map(|ml| ml.members().collect::<Vec<_>>())
        .filter_map(|m| {
            let member_name = name_from(m.name_token());
            if member_name.is_none() {
                diags.push(diag(file_id, m.syntax().text_range(), DiagnosticCode::E009));
            }
            member_name.map(|n| ListMember {
                name: n,
                // No ordinal-value grammar in this skeleton
                // (`parser/decl.rs::flags_member`) — unlike ink's
                // `item = 5`, native flags members are name(+active)-only.
                value: None,
                is_active: m.is_active(),
            })
        })
        .collect();
    let doc = lower_doc_comment(file_id, node.doc(), DocPolicy::VALUE, diags);
    Some(ListDecl {
        ptr: native_provenance(file_id, NodeClass::ListDecl, node.syntax()),
        name,
        members,
        doc,
        visibility: None,
        was: None,
    })
}

/// `struct Name { field: type, … }`.
pub(super) fn lower_struct_decl(
    file_id: FileId,
    node: &ast::StructDecl,
    diags: &mut Vec<Diagnostic>,
) -> Option<StructDecl> {
    let range = node.syntax().text_range();
    let Some(name) = name_from(node.name_token()) else {
        // Reuses E001 ("knot missing a name")'s sibling shape — there is no
        // dedicated ink STRUCT-missing-name code (ink's own STRUCT grammar
        // always materializes a name node), so this borrows the nearest
        // "declaration missing a name" code in the same family (E004: VAR)
        // rather than minting a new one for a shape ink's own frontend
        // never needed to diagnose.
        diags.push(diag(file_id, range, DiagnosticCode::E004));
        return None;
    };
    let fields: Vec<StructFieldDecl> = node
        .fields()
        .filter_map(|f| {
            let field_name = name_from(f.name_token());
            // NG-E (issue #1505): the field's `: type` clause is now a full
            // `type_expr` (bare name, generic instantiation, or function
            // type), not just a dotted path — same lowering helper every
            // other `: type` position in this dialect uses.
            let ty = binding_annotation(f.type_annotation().as_ref());
            if let (Some(n), Some(ty)) = (field_name, ty) {
                Some(StructFieldDecl { name: n, ty })
            } else {
                diags.push(diag(file_id, f.syntax().text_range(), DiagnosticCode::E003));
                None
            }
        })
        .collect();
    let doc = lower_doc_comment(file_id, node.doc(), DocPolicy::VALUE, diags);
    Some(StructDecl {
        ptr: native_provenance(file_id, NodeClass::StructDecl, node.syntax()),
        name,
        fields,
        doc,
        visibility: None,
    })
}

/// `extern name(params)`.
pub(super) fn lower_extern_decl(
    file_id: FileId,
    node: &ast::ExternDecl,
    diags: &mut Vec<Diagnostic>,
) -> Option<ExternalDecl> {
    let range = node.syntax().text_range();
    let Some(name) = name_from(node.name_token()) else {
        diags.push(diag(file_id, range, DiagnosticCode::E010));
        return None;
    };
    let params: Vec<ParamInfo> = node
        .param_list()
        .into_iter()
        .flat_map(|pl| pl.params().collect::<Vec<_>>())
        .filter_map(|p| {
            // `is_ref`/`is_divert` are always `false` for an `EXTERNAL`
            // parameter — matches ink's own `ExternalDecl.params` convention
            // (`hir/types.rs` doc: "mirroring the pre-B0.4 manifest
            // population in `decl::external::declare_and_lower`"), which
            // discards `ref`-ness even though ink's EXTERNAL grammar
            // syntactically permits it too (both frontends reuse the
            // general param-list rule). A native `extern` writer who marks
            // a param `ref` gets the same silent no-op ink already has —
            // an existing wart, not one this slice introduces; flagged for
            // #1106 as worth a shared diagnostic in a follow-up rather than
            // diverging the two frontends' `ExternalDecl.params` semantics.
            //
            // A `: type` annotation (NG-A, issue #1487) lands in the same
            // place for the same reason: `ParamInfo` has no annotation slot,
            // and `brink-analyzer::strict`'s `check_external` already
            // records that no inline-annotation exemption exists for an
            // `EXTERNAL` at all. So an annotated native `extern` parameter is
            // *accepted by the grammar* (it rides the shared `param_list`
            // rule) and carries no HIR meaning — widening `ParamInfo` is the
            // same cross-frontend change as the `ref` wart above and belongs
            // with it, not here.
            p.name_token().map(|t| ParamInfo {
                name: t.text().to_string(),
                is_ref: false,
                is_divert: false,
            })
        })
        .collect();
    #[expect(
        clippy::cast_possible_truncation,
        reason = "external params won't exceed 255, mirrors the ink lowering's own cast"
    )]
    let param_count = params.len() as u8;
    let doc = lower_doc_comment(file_id, node.doc(), DocPolicy::EXTERNAL, diags);
    Some(ExternalDecl {
        ptr: native_provenance(file_id, NodeClass::ExternalDecl, node.syntax()),
        name,
        param_count,
        params,
        doc,
        visibility: None,
        was: None,
    })
}

/// Every `IDENT` a native `PATH_SEGMENT` chain visits — used by
/// `import`/`use` lowering (`super::import`). Struct-field types now go
/// through `super::types::lower_type_annotation` instead (NG-E, #1505).
pub(super) fn joined_path_text(path: &ast::Path) -> String {
    lower_path(path)
        .segments
        .iter()
        .map(|n| n.text.as_str())
        .collect::<Vec<_>>()
        .join(".")
}

/// `true` if `node` sits directly inside `SOURCE_FILE` or a (possibly
/// nested) `MODULE_DECL`'s `BLOCK` — the "flattened top-level scope" a
/// struct/extern/use/import declaration must be declared in (mirrors ink's
/// D6 posture: these four kinds are NOT hoisted by a whole-tree walk the
/// way `var`/`const`/`flags` are — `docs/hir-admission-contract.md` D6,
/// `hir/lower/structure/mod.rs`'s `file.struct_decls()`/`file.externals()`
/// direct-children comments). `var`/`const`/`flags` skip this check
/// entirely (collected via `.descendants()`, unconditionally hoisted, same
/// as ink) — see the `lower_native` module doc's numbered judgment calls.
pub(super) fn in_flattened_scope(node: &SyntaxNode) -> bool {
    let Some(parent) = node.parent() else {
        return true;
    };
    if parent.kind() == N::SOURCE_FILE {
        return true;
    }
    if parent.kind() == N::BLOCK
        && let Some(grandparent) = parent.parent()
        && grandparent.kind() == N::MODULE_DECL
    {
        return in_flattened_scope(&grandparent);
    }
    false
}
