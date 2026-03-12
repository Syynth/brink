use std::hash::{DefaultHasher, Hash, Hasher};

use brink_format::{DefinitionId, DefinitionTag};

/// Deterministic 56-bit hash of a path string.
fn hash_path(path: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
}

/// Create a `DefinitionId` for an address (container or intra-container target).
pub fn address_id(path: &str) -> DefinitionId {
    DefinitionId::new(DefinitionTag::Address, hash_path(path))
}

/// Create a `DefinitionId` for a global variable.
pub fn global_var_id(name: &str) -> DefinitionId {
    DefinitionId::new(DefinitionTag::GlobalVar, hash_path(name))
}

/// Create a `DefinitionId` for a list definition.
pub fn list_def_id(name: &str) -> DefinitionId {
    DefinitionId::new(DefinitionTag::ListDef, hash_path(name))
}

/// Create a `DefinitionId` for a list item (`"ListName.ItemName"`).
pub fn list_item_id(qualified: &str) -> DefinitionId {
    DefinitionId::new(DefinitionTag::ListItem, hash_path(qualified))
}

/// Create a `DefinitionId` for an external function.
pub fn external_fn_id(name: &str) -> DefinitionId {
    DefinitionId::new(DefinitionTag::ExternalFn, hash_path(name))
}

/// Create a `DefinitionId` for an intra-container address (index target).
///
/// Uses the same `Address` tag as `address_id` — the only difference is the
/// path string hashed. Both containers and intra-container targets live in
/// the same namespace now.
pub fn intra_address_id(path: &str) -> DefinitionId {
    DefinitionId::new(DefinitionTag::Address, hash_path(path))
}

/// Resolve an ink.json path reference against a current container path.
///
/// - Absolute paths (not starting with `.`) are returned as-is.
/// - Relative paths start with `.` and use `^` for parent navigation.
///
/// In ink's path semantics, relative paths are resolved from the element's
/// position within its container. The first `^` goes to the container itself
/// (effectively a no-op since `current_path` already names the container),
/// and subsequent `^` components go to ancestor containers.
pub(crate) fn resolve_path(current_path: &str, ink_path: &str) -> String {
    // Absolute path — return as-is
    if !ink_path.starts_with('.') {
        return ink_path.to_string();
    }

    // Relative path: resolve from current container.
    //
    // In ink's runtime, the content pointer is *inside* a container. The
    // first `^` goes from the content pointer to the container itself (a
    // no-op relative to `current_path`), and subsequent `^` components go
    // to ancestor containers.
    let mut components: Vec<&str> = if current_path.is_empty() {
        Vec::new()
    } else {
        current_path.split('.').collect()
    };

    let relative = &ink_path[1..]; // skip leading '.'
    let mut first_caret = true;

    for part in relative.split('.') {
        if part.is_empty() {
            continue;
        }
        if part == "^" {
            if first_caret {
                // First `^`: goes from element to its container — no-op
                first_caret = false;
            } else {
                // Subsequent `^`: go to parent container
                components.pop();
            }
        } else {
            first_caret = false;
            components.push(part);
        }
    }

    components.join(".")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_passthrough() {
        assert_eq!(resolve_path("foo.bar", "baz.qux"), "baz.qux");
    }

    #[test]
    fn relative_parent_then_sibling() {
        // `.^.c-0` from `foo.bar`: first `^` goes to container `foo.bar`,
        // then `c-0` is its named content → `foo.bar.c-0`
        assert_eq!(resolve_path("foo.bar", ".^.c-0"), "foo.bar.c-0");
    }

    #[test]
    fn relative_two_parents() {
        // `.^.^.d` from `a.b.c`: first `^` → container `a.b.c`,
        // second `^` → parent `a.b`, then `d` → `a.b.d`
        assert_eq!(resolve_path("a.b.c", ".^.^.d"), "a.b.d");
    }

    #[test]
    fn relative_sibling() {
        // `.sibling` from `foo.bar`: appends to container path
        assert_eq!(resolve_path("foo.bar", ".sibling"), "foo.bar.sibling");
    }

    #[test]
    fn relative_from_root() {
        assert_eq!(resolve_path("", ".child"), "child");
    }

    #[test]
    fn relative_named_content() {
        // `.^.b` from inside `0.5`: `^` → `0.5`, `b` → `0.5.b`
        assert_eq!(resolve_path("0.5", ".^.b"), "0.5.b");
    }

    #[test]
    fn address_id_deterministic() {
        let a = address_id("foo.bar");
        let b = address_id("foo.bar");
        assert_eq!(a, b);
    }

    #[test]
    fn different_tags() {
        let c = address_id("x");
        let g = global_var_id("x");
        assert_ne!(c, g);
    }
}
