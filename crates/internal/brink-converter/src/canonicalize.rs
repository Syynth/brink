//! Preprocessing pass: canonicalize paths and remove $r ceremony.
//!
//! Before index-building or codegen sees the tree, this pass:
//! 1. Rewrites all named-path aliases to their numeric equivalents
//!    (e.g. `knot.stitch.0.g-0.c-0` → `knot.stitch.0.0.c-0`).
//! 2. Blanks out $r-family elements with `Element::Nop`.

use std::collections::HashMap;

use brink_json::{ChoicePoint, Container, Divert, Element, InkValue, VariableAssignment};

use crate::path::resolve_path;

/// Name-to-index map: `parent_path → (child_name → numeric_index)`.
type NameMap = HashMap<String, HashMap<String, usize>>;

// ── Pass 1: Analyze ─────────────────────────────────────────────────

/// Walk the tree and build a map from `(parent_path, child_name)` to
/// the child's numeric index within `contents`.
///
/// Only indexed children (those in `contents`) that have a `#n` name
/// are recorded. Named-content children (knots/stitches) always use
/// string names and don't need mapping.
fn build_name_map(container: &Container, current_path: &str, map: &mut NameMap) {
    let mut child_names: HashMap<String, usize> = HashMap::new();

    for (i, element) in container.contents.iter().enumerate() {
        if let Element::Container(child) = element {
            // If this indexed child has a name, record the mapping.
            if let Some(name) = &child.name {
                child_names.insert(name.clone(), i);
            }

            // Recurse into the child using its numeric path.
            let child_path = if current_path.is_empty() {
                i.to_string()
            } else {
                format!("{current_path}.{i}")
            };
            build_name_map(child, &child_path, map);
        }
    }

    if !child_names.is_empty() {
        map.insert(current_path.to_string(), child_names);
    }

    // Recurse into named content (these use string keys, no remapping needed,
    // but their descendants may have indexed children with names).
    for (name, element) in &container.named_content {
        if let Element::Container(child) = element {
            let child_path = if current_path.is_empty() {
                name.clone()
            } else {
                format!("{current_path}.{name}")
            };
            build_name_map(child, &child_path, map);
        }
    }
}

/// Rewrite a resolved absolute path, replacing named components with
/// their numeric equivalents using the name map.
///
/// Walks components left-to-right, building the parent path at each
/// step. If the current component is a name that appears in the
/// name map for the current parent, replace it with the numeric index.
fn canonicalize_path(path: &str, name_map: &NameMap) -> String {
    let components: Vec<&str> = path.split('.').collect();
    if components.is_empty() {
        return path.to_string();
    }

    let mut canonical = Vec::with_capacity(components.len());
    let mut parent_path = String::new();

    for (i, component) in components.iter().enumerate() {
        // Check if this component is a named alias at this parent.
        let replacement = name_map
            .get(&parent_path)
            .and_then(|children| children.get(*component))
            .map(usize::to_string);

        let actual = if let Some(ref idx_str) = replacement {
            idx_str.as_str()
        } else {
            component
        };

        canonical.push(actual.to_string());

        // Update parent path for next iteration.
        if i == 0 {
            parent_path = actual.to_string();
        } else {
            parent_path = format!("{parent_path}.{actual}");
        }
    }

    canonical.join(".")
}

// ── Pass 2: Clone + Transform ───────────────────────────────────────

/// Check if a string references a `$r`-family name.
fn is_dollar_r(s: &str) -> bool {
    s.starts_with("$r")
}

/// Check if a path contains a `$r`-family component.
fn path_contains_dollar_r(path: &str) -> bool {
    path.split('.').any(|c| c.starts_with("$r"))
}

/// Transform a single element: canonicalize paths and blank $r elements.
fn transform_element(element: &Element, current_path: &str, name_map: &NameMap) -> Element {
    match element {
        // ── $r blanking ──

        // Container named $r*
        Element::Container(c) if c.name.as_deref().is_some_and(is_dollar_r) => Element::Nop,

        // DivertTarget pointing to $r
        Element::Value(InkValue::DivertTarget(p)) if path_contains_dollar_r(p) => Element::Nop,

        // temp=$r assignment
        Element::VariableAssignment(VariableAssignment::TemporaryAssignment {
            variable, ..
        }) if is_dollar_r(variable) => Element::Nop,

        // Variable divert to $r
        Element::Divert(Divert::Variable { path, .. }) if is_dollar_r(path) => Element::Nop,

        // ── Path canonicalization ──
        Element::Value(InkValue::DivertTarget(p)) => {
            let resolved = resolve_path(current_path, p);
            let canonical = canonicalize_path(&resolved, name_map);
            Element::Value(InkValue::DivertTarget(canonical))
        }

        Element::Divert(Divert::Target { conditional, path }) => {
            let resolved = resolve_path(current_path, path);
            let canonical = canonicalize_path(&resolved, name_map);
            Element::Divert(Divert::Target {
                conditional: *conditional,
                path: canonical,
            })
        }

        Element::Divert(Divert::Function { conditional, path }) => {
            let resolved = resolve_path(current_path, path);
            let canonical = canonicalize_path(&resolved, name_map);
            Element::Divert(Divert::Function {
                conditional: *conditional,
                path: canonical,
            })
        }

        Element::Divert(Divert::Tunnel { conditional, path }) => {
            let resolved = resolve_path(current_path, path);
            let canonical = canonicalize_path(&resolved, name_map);
            Element::Divert(Divert::Tunnel {
                conditional: *conditional,
                path: canonical,
            })
        }

        Element::ChoicePoint(cp) => {
            let resolved = resolve_path(current_path, &cp.target);
            let canonical = canonicalize_path(&resolved, name_map);
            Element::ChoicePoint(ChoicePoint {
                target: canonical,
                flags: cp.flags,
            })
        }

        Element::ReadCount(rc) => {
            let resolved = resolve_path(current_path, &rc.variable);
            let canonical = canonicalize_path(&resolved, name_map);
            Element::ReadCount(brink_json::ReadCountReference {
                variable: canonical,
            })
        }

        // ── Recurse into child containers ──
        Element::Container(_) => {
            // Non-$r containers are transformed recursively in transform_container.
            // This arm handles inline containers encountered in contents; the
            // actual deep transformation happens in transform_container.
            // We clone here; the caller (transform_container) handles recursion.
            element.clone()
        }

        // Everything else passes through unchanged.
        _ => element.clone(),
    }
}

/// Deep-clone and transform a container and all its descendants.
fn transform_container(container: &Container, current_path: &str, name_map: &NameMap) -> Container {
    let mut new_contents = Vec::with_capacity(container.contents.len());

    for (i, element) in container.contents.iter().enumerate() {
        let child_path = if current_path.is_empty() {
            i.to_string()
        } else {
            format!("{current_path}.{i}")
        };

        match element {
            Element::Container(child) if child.name.as_deref().is_some_and(is_dollar_r) => {
                // $r marker container → Nop
                new_contents.push(Element::Nop);
            }
            Element::Container(child) => {
                let transformed = transform_container(child, &child_path, name_map);
                new_contents.push(Element::Container(transformed));
            }
            other => {
                new_contents.push(transform_element(other, current_path, name_map));
            }
        }
    }

    // Transform named content
    let mut new_named = HashMap::with_capacity(container.named_content.len());
    for (name, element) in &container.named_content {
        let child_path = if current_path.is_empty() {
            name.clone()
        } else {
            format!("{current_path}.{name}")
        };

        match element {
            Element::Container(child) => {
                let transformed = transform_container(child, &child_path, name_map);
                new_named.insert(name.clone(), Element::Container(transformed));
            }
            other => {
                new_named.insert(
                    name.clone(),
                    transform_element(other, current_path, name_map),
                );
            }
        }
    }

    Container {
        flags: container.flags,
        name: container.name.clone(),
        named_content: new_named,
        contents: new_contents,
    }
}

// ── Public API ──────────────────────────────────────────────────────

/// Canonicalize all paths in the tree and blank out $r ceremony elements.
///
/// Returns a new root container with:
/// - All path-bearing elements rewritten to absolute numeric form
/// - All $r-family elements replaced with `Element::Nop`
pub fn canonicalize(root: &Container) -> Container {
    let mut name_map = NameMap::new();
    build_name_map(root, "", &mut name_map);
    transform_container(root, "", &name_map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_json::ChoicePointFlags;

    /// Helper: create a minimal container with given contents and named content.
    fn make_container(
        contents: Vec<Element>,
        named: Vec<(&str, Element)>,
        name: Option<&str>,
    ) -> Container {
        Container {
            flags: None,
            name: name.map(String::from),
            named_content: named.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
            contents,
        }
    }

    #[test]
    fn named_child_paths_are_canonicalized() {
        // Build a tree where contents[0] is a container named "g-0".
        // A divert to "root.g-0" should become "root.0".
        let inner = make_container(vec![], vec![], Some("g-0"));
        let divert = Element::Divert(Divert::Target {
            conditional: false,
            path: ".^.g-0".to_string(),
        });
        let outer = make_container(vec![Element::Container(inner), divert], vec![], None);
        let root = make_container(vec![], vec![("root", Element::Container(outer))], None);

        let result = canonicalize(&root);
        let Some(Element::Container(root_c)) = result.named_content.get("root") else {
            unreachable!("expected root named content");
        };

        assert!(
            matches!(
                &root_c.contents[1],
                Element::Divert(Divert::Target { path, .. }) if path == "root.0"
            ),
            "named alias should be canonicalized to numeric index, got {:?}",
            root_c.contents[1]
        );
    }

    #[test]
    fn dollar_r_elements_become_nop() {
        let contents = vec![
            Element::Value(InkValue::String("hello".into())),
            Element::Value(InkValue::DivertTarget("knot.0.$r1".into())),
            Element::VariableAssignment(VariableAssignment::TemporaryAssignment {
                variable: "$r".into(),
                reassign: false,
            }),
            Element::Container(make_container(vec![], vec![], Some("$r1"))),
            Element::Divert(Divert::Variable {
                conditional: false,
                path: "$r".into(),
            }),
        ];
        let root = make_container(contents, vec![], None);
        let result = canonicalize(&root);

        // Element 0 (string) should be unchanged.
        assert!(matches!(&result.contents[0], Element::Value(InkValue::String(s)) if s == "hello"));
        // Elements 1-4 should all be Nop.
        for (idx, elem) in result.contents.iter().enumerate().skip(1) {
            assert!(
                matches!(elem, Element::Nop),
                "element {idx} should be Nop, got {elem:?}"
            );
        }
    }

    #[test]
    fn choice_point_target_canonicalized() {
        let inner = make_container(vec![], vec![], Some("c-0"));
        let cp = Element::ChoicePoint(ChoicePoint {
            target: "root.c-0".to_string(),
            flags: ChoicePointFlags::empty(),
        });
        let outer = make_container(vec![Element::Container(inner), cp], vec![], None);
        let root = make_container(vec![], vec![("root", Element::Container(outer))], None);

        let result = canonicalize(&root);
        let Some(Element::Container(root_c)) = result.named_content.get("root") else {
            unreachable!("expected root named content");
        };

        assert!(
            matches!(
                &root_c.contents[1],
                Element::ChoicePoint(cp) if cp.target == "root.0"
            ),
            "choice target should be canonicalized, got {:?}",
            root_c.contents[1]
        );
    }

    #[test]
    fn deeply_nested_named_path_canonicalized() {
        // Simulate the I063 pattern: knot.stitch.0.g-0.c-0
        // g-0 is contents[0] of stitch.0, c-0 is named content of g-0.
        let c0 = make_container(vec![], vec![], None);
        let g0 = make_container(vec![], vec![("c-0", Element::Container(c0))], Some("g-0"));

        // A divert from somewhere targeting "knot.stitch.0.g-0.c-0"
        let divert = Element::Divert(Divert::Target {
            conditional: false,
            path: "knot.stitch.0.g-0.c-0".to_string(),
        });

        let stitch_0 = make_container(vec![Element::Container(g0)], vec![], None);

        let stitch = make_container(vec![Element::Container(stitch_0)], vec![], None);

        let knot = make_container(vec![], vec![("stitch", Element::Container(stitch))], None);

        let root = make_container(vec![divert], vec![("knot", Element::Container(knot))], None);

        let result = canonicalize(&root);

        // The divert in root.contents[0] should have "knot.stitch.0.0.c-0"
        assert!(
            matches!(
                &result.contents[0],
                Element::Divert(Divert::Target { path, .. }) if path == "knot.stitch.0.0.c-0"
            ),
            "g-0 should be replaced with 0 in nested path, got {:?}",
            result.contents[0]
        );
    }
}
