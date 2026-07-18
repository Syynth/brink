//! Shared semantic-type classification (#1027).
//!
//! `external_check::resolve_type` (hover / signature help / argument
//! pickers / literal-argument checks) and `infer::type_ref_to_ty` (TM-3
//! strict inference) both need to answer the same question — "does this
//! [`TypeRef`] name a base-type keyword, a registered [`SemanticTypeDef`],
//! or neither?" — and used to answer it with two independently-written
//! `match` chains. They agreed for base keywords and registered names, but
//! diverged for an **unregistered** semantic-type name: `resolve_type`
//! still built a `ResolvedType` for it (cosmetically "resolved", just with
//! `base: None`), while `type_ref_to_ty` returned `Ty::Unknown` outright.
//! That divergence is what made #1004 look like strict inference ignoring
//! the host manifest, when the real story was hover rendering an
//! unregistered type name as if it were trustworthy (`id: var_id`) — see
//! the #1004 investigation's evidence matrix.
//!
//! [`classify`] is now the single place that decides "base / registered /
//! unregistered" — both call sites match on its result, so an unregistered
//! name is classified identically (as [`TypeShape::Unregistered`])
//! everywhere. Each caller still decides *what to do* with that answer
//! (`resolve_type` diagnoses `E040` and keeps building presentational
//! metadata; `type_ref_to_ty` just types it `Ty::Unknown`) — this module
//! only unifies the classification, not the two callers' different
//! purposes.

use std::collections::BTreeMap;

use brink_ir::{BaseType, SemanticTypeDef, TypeRef};

/// How a [`TypeRef`] resolves against the registered semantic-type
/// vocabulary (`types`, from the merged [`brink_ir::HostManifest`]).
#[derive(Debug, Clone, Copy)]
pub(crate) enum TypeShape<'a> {
    /// An empty ref — no type was specified at all.
    Unspecified,
    /// A base-type keyword (`string`/`int`/`float`/`bool`/`void`/`handle`).
    /// Resolved directly from the keyword; never consults `types` (matching
    /// both callers' pre-existing behavior — a base keyword is never
    /// shadowed by a same-named manifest entry).
    Base(BaseType),
    /// A name found in `types` — resolved through its own definition.
    Registered(&'a SemanticTypeDef),
    /// Neither a base keyword nor a name in `types` — genuinely
    /// unresolved/unregistered. The one shape both callers must treat as
    /// "unknown", not "resolved".
    Unregistered,
}

/// Classify `t` against `types`. The single source of truth both
/// `external_check::resolve_type` and `infer::type_ref_to_ty` build on
/// (#1027).
pub(crate) fn classify<'a>(
    t: &TypeRef,
    types: &'a BTreeMap<String, SemanticTypeDef>,
) -> TypeShape<'a> {
    if t.is_unspecified() {
        return TypeShape::Unspecified;
    }
    if let Some(base) = t.as_base() {
        return TypeShape::Base(base);
    }
    match types.get(t.0.trim()) {
        Some(def) => TypeShape::Registered(def),
        None => TypeShape::Unregistered,
    }
}

#[cfg(test)]
mod tests {
    use brink_ir::Constraint;

    use super::*;

    fn types_with(name: &str, base: BaseType) -> BTreeMap<String, SemanticTypeDef> {
        let mut types = BTreeMap::new();
        types.insert(
            name.to_string(),
            SemanticTypeDef {
                name: name.to_string(),
                base,
                constraint: None,
                values: None,
                widget: None,
            },
        );
        types
    }

    #[test]
    fn unspecified_ref_classifies_as_unspecified() {
        assert!(matches!(
            classify(&TypeRef::default(), &BTreeMap::new()),
            TypeShape::Unspecified
        ));
    }

    #[test]
    fn base_keyword_never_consults_types() {
        // Even if `types` happens to have an entry with the same spelling, a
        // base keyword resolves directly — no shadowing.
        let types = types_with("int", BaseType::String);
        assert!(matches!(
            classify(&TypeRef("int".to_string()), &types),
            TypeShape::Base(BaseType::Int)
        ));
    }

    #[test]
    fn registered_name_resolves_through_its_def() {
        let types = types_with("var_id", BaseType::Int);
        let def = match classify(&TypeRef("var_id".to_string()), &types) {
            TypeShape::Registered(def) => Some(def),
            _ => None,
        }
        .expect("expected Registered");
        assert_eq!(def.base, BaseType::Int);
    }

    #[test]
    fn unregistered_name_classifies_as_unregistered() {
        // The #1004/#1027 case: `var_id` names neither a base keyword nor a
        // registered semantic type.
        assert!(matches!(
            classify(&TypeRef("var_id".to_string()), &BTreeMap::new()),
            TypeShape::Unregistered
        ));
    }

    #[test]
    fn unregistered_with_other_types_present_is_still_unregistered() {
        let types = types_with("actor_id", BaseType::String);
        assert!(matches!(
            classify(&TypeRef("var_id".to_string()), &types),
            TypeShape::Unregistered
        ));
    }

    #[test]
    fn registered_carries_through_constraint() {
        let mut types = BTreeMap::new();
        types.insert(
            "item_id".to_string(),
            SemanticTypeDef {
                name: "item_id".to_string(),
                base: BaseType::String,
                constraint: Some(Constraint::Enum {
                    values: vec!["sword".into()],
                }),
                values: None,
                widget: None,
            },
        );
        let def = match classify(&TypeRef("item_id".to_string()), &types) {
            TypeShape::Registered(def) => Some(def),
            _ => None,
        }
        .expect("expected Registered");
        assert!(matches!(def.constraint, Some(Constraint::Enum { .. })));
    }
}
