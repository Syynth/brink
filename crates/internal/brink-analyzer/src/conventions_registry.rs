//! The project-layer half of issue #1863's injection point: the "join on
//! `DefinitionId`" step named by Q1 of the 2026-08-01 decision-log ruling
//! ("Conventions comptime: the four blocking rulings"):
//!
//! > The compiler reads the conventions module's CST for `ClaimHandler`
//! > records... it separately comptime-evaluates `fn conventions()` for an
//! > ORDERED LIST OF IDENTITIES; it joins them. `DefinitionId` is the join
//! > key... the join is on a hash both sides compute independently, so no
//! > cross-file *name* resolution is reintroduced.
//!
//! [`brink_ir::hir::lower_native::external_conventions`] owns the
//! injection point itself (the seam a file's own lowering consumes).
//! This module owns the piece that crate cannot: attaching a
//! `DefinitionId` to each of the conventions module's CST-declared
//! handlers, via the project [`SymbolIndex`] — the same identity every
//! other declared symbol in the project already carries, computed the
//! same way (`hash_qualified_name`, this crate's `manifest` module),
//! never re-derived here.
//!
//! # Deliberately not here
//!
//! Comptime-evaluating `fn conventions()` for the real ordered identity
//! list is issue #1840's job. [`join_conventions_registry`] takes that
//! list as a plain `&[DefinitionId]` — hand-constructed by a test today,
//! eventually #1840's evaluator output — and is agnostic to which; this
//! module runs no brink code and reads no comptime evaluator.
//!
//! Resolving *which file* is the project's conventions module (the
//! `[project] elements` pointer against `module_map_query`) is
//! `brink-db`'s job, exactly as `conventions_confinement.rs`'s own doc
//! explains for the sibling #1844 check — this module takes the resolved
//! `conventions_file: FileId` as a plain argument, the same posture.

use brink_format::DefinitionId;
use brink_ir::hir::lower_native::external_conventions::{
    ExternalClaimHandler, ExternalConventions,
};
use brink_ir::{ClaimHandlerDecl, FileId, Name, SymbolIndex};
use rowan::TextRange;

/// One of the conventions module's declared claiming handlers, joined
/// with its own `DefinitionId` — the CST-read half of Q1's join, ready to
/// be matched against a comptime-evaluated (or, before #1840 lands,
/// hand-constructed) ordered identity list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimHandlerCandidate {
    /// The join key: this handler's own definition identity, resolved
    /// against the project `SymbolIndex`.
    pub id: DefinitionId,
    /// The handler's own name, carrying its declaration-site range.
    pub name: Name,
    /// Parameter names in declaration order.
    pub params: Vec<String>,
    /// The claiming pattern's regex source.
    pub pattern: String,
    /// Range of the `@[element(claims = "…")]` annotation line.
    pub annotation: TextRange,
}

/// Attach each of `handlers`' own `DefinitionId` via `index`, keeping only
/// the ones that actually resolve to a **function** declared in
/// `conventions_file` — the same `is_function_definition` gate every
/// other native-`fn`-identity lookup in this crate uses (e.g.
/// `signature.rs`).
///
/// A handler whose name resolves to nothing in `conventions_file` (should
/// never happen for `HirFile::claim_handlers` sourced from that exact
/// file's own lowering, but a caller could pass a mismatched pair) is
/// silently dropped — the id is what makes the join possible, and a
/// candidate with no id cannot be joined against anything.
#[must_use]
pub fn candidate_claim_handlers(
    index: &SymbolIndex,
    conventions_file: FileId,
    handlers: &[ClaimHandlerDecl],
) -> Vec<ClaimHandlerCandidate> {
    handlers
        .iter()
        .filter_map(|handler| {
            let id = index
                .by_name
                .get(handler.name.text.as_str())?
                .iter()
                .find(|id| {
                    index.symbols.get(id).is_some_and(|info| {
                        info.file == conventions_file && info.is_function_definition()
                    })
                })?;
            Some(ClaimHandlerCandidate {
                id: *id,
                name: handler.name.clone(),
                params: handler.params.clone(),
                pattern: handler.pattern.clone(),
                annotation: handler.annotation,
            })
        })
        .collect()
}

/// Join an ordered identity list against the conventions module's own
/// CST-derived candidate set, producing the ordered, applicable external
/// handler set — issue #1863's injection point payload, ready to hand to
/// [`brink_ir::hir::lower_native::lower_with_conventions`] for every
/// OTHER file in the project.
///
/// An id in `order` with no matching candidate — Q1's
/// "registered-but-not-declared" case — is silently absent from the
/// result rather than fabricating a matcher with no CST payload to draw
/// from; that mismatch is a diagnostic concern for whichever build step
/// owns the real evaluator (#1840), not this join. Equally, a candidate
/// whose id never appears in `order` — "declared-but-not-registered" —
/// simply never makes it into the result, the same silent-omission shape.
#[must_use]
pub fn join_conventions_registry(
    order: &[DefinitionId],
    candidates: &[ClaimHandlerCandidate],
) -> ExternalConventions {
    let handlers = order
        .iter()
        .filter_map(|id| {
            candidates
                .iter()
                .find(|c| &c.id == id)
                .map(|c| ExternalClaimHandler {
                    name: c.name.clone(),
                    params: c.params.clone(),
                    pattern: c.pattern.clone(),
                    annotation: c.annotation,
                })
        })
        .collect();
    ExternalConventions::new(handlers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_format::DefinitionTag;
    use brink_ir::hir::lower_native;
    use brink_ir::{SymbolInfo, SymbolKind, Visibility};

    fn conventions_hir(src: &str) -> brink_ir::HirFile {
        let parsed = brink_syntax_native::parse(src);
        assert!(parsed.errors().is_empty(), "{:?}", parsed.errors());
        let (hir, _manifest, _diags) = lower_native::lower(FileId(0), &parsed.tree());
        hir
    }

    fn function_symbol(file: FileId, range: TextRange, name: &str, id: DefinitionId) -> SymbolInfo {
        SymbolInfo {
            kind: SymbolKind::Knot,
            file,
            range,
            id,
            name: name.to_string(),
            params: Vec::new(),
            detail: Some("function".to_string()),
            scope: None,
            param_detail: None,
            module: None,
            visibility: Visibility::Private,
        }
    }

    const CONVENTIONS_SRC: &str = "@[element(claims = \"^INT\\\\. (?<place>.+)$\")]\n\
        fn interior(place: content) {\n  return place;\n}\n";

    #[test]
    fn candidate_claim_handlers_attaches_the_index_backed_id() {
        let hir = conventions_hir(CONVENTIONS_SRC);
        let id = DefinitionId::new(DefinitionTag::Address, 0xC0FF_EE00);
        let mut index = SymbolIndex::default();
        index
            .by_name
            .insert(hir.claim_handlers[0].name.text.clone(), vec![id]);
        index.symbols.insert(
            id,
            function_symbol(
                FileId(0),
                hir.claim_handlers[0].annotation,
                &hir.claim_handlers[0].name.text,
                id,
            ),
        );

        let candidates = candidate_claim_handlers(&index, FileId(0), &hir.claim_handlers);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].id, id);
        assert_eq!(candidates[0].name.text, "interior");
        assert_eq!(candidates[0].params, vec!["place".to_string()]);
    }

    #[test]
    fn candidate_claim_handlers_drops_a_wrong_file_namesake() {
        let hir = conventions_hir(CONVENTIONS_SRC);
        let elsewhere = DefinitionId::new(DefinitionTag::Address, 0xBAD_F11E);
        let mut index = SymbolIndex::default();
        index
            .by_name
            .insert(hir.claim_handlers[0].name.text.clone(), vec![elsewhere]);
        index.symbols.insert(
            elsewhere,
            function_symbol(
                FileId(99),
                TextRange::default(),
                &hir.claim_handlers[0].name.text,
                elsewhere,
            ),
        );

        let candidates = candidate_claim_handlers(&index, FileId(0), &hir.claim_handlers);

        assert!(candidates.is_empty(), "{candidates:?}");
    }

    #[test]
    fn join_orders_by_the_external_identity_list_not_declaration_order() {
        let first = DefinitionId::new(DefinitionTag::Address, 1);
        let second = DefinitionId::new(DefinitionTag::Address, 2);
        let name = |text: &str| Name {
            text: text.to_string(),
            range: TextRange::default(),
        };
        let candidates = vec![
            ClaimHandlerCandidate {
                id: first,
                name: name("interior"),
                params: vec!["place".to_string()],
                pattern: "^INT\\. (?<place>.+)$".to_string(),
                annotation: TextRange::default(),
            },
            ClaimHandlerCandidate {
                id: second,
                name: name("exterior"),
                params: vec!["place".to_string()],
                pattern: "^EXT\\. (?<place>.+)$".to_string(),
                annotation: TextRange::default(),
            },
        ];

        // The identity list — the "one thing the comptime boundary
        // uniquely knows" — orders `exterior` BEFORE `interior`, the
        // reverse of `candidates`' own declaration order: proves the join
        // takes its order from `order`, not from candidate position.
        let registry = join_conventions_registry(&[second, first], &candidates);

        assert_eq!(
            registry
                .handlers()
                .iter()
                .map(|h| h.name.text.as_str())
                .collect::<Vec<_>>(),
            vec!["exterior", "interior"]
        );
    }

    #[test]
    fn join_silently_drops_a_registered_but_undeclared_id() {
        let declared = DefinitionId::new(DefinitionTag::Address, 1);
        let registered_only = DefinitionId::new(DefinitionTag::Address, 2);
        let candidates = vec![ClaimHandlerCandidate {
            id: declared,
            name: Name {
                text: "interior".to_string(),
                range: TextRange::default(),
            },
            params: Vec::new(),
            pattern: "^INT\\.".to_string(),
            annotation: TextRange::default(),
        }];

        let registry = join_conventions_registry(&[registered_only, declared], &candidates);

        assert_eq!(registry.handlers().len(), 1);
        assert_eq!(registry.handlers()[0].name.text, "interior");
    }

    #[test]
    fn join_silently_drops_a_declared_but_unregistered_candidate() {
        let declared = DefinitionId::new(DefinitionTag::Address, 1);
        let candidates = vec![ClaimHandlerCandidate {
            id: declared,
            name: Name {
                text: "interior".to_string(),
                range: TextRange::default(),
            },
            params: Vec::new(),
            pattern: "^INT\\.".to_string(),
            annotation: TextRange::default(),
        }];

        // `order` is empty — nothing was ever registered.
        let registry = join_conventions_registry(&[], &candidates);

        assert!(registry.is_empty());
    }
}
