//! Round-trip-preserving edits to `brink.toml`.
//!
//! The rest of this crate PARSES config into [`ProjectConfig`] with `toml`,
//! which is the right tool for reading: it produces owned Rust values and
//! throws away everything that only matters to a human — comments, blank
//! lines, key order, quote style, indentation.
//!
//! Writing needs the opposite. `brink.toml` is hand-maintained, and several
//! features now edit it programmatically: `drafts` globs, the spellcheck
//! dictionary, project-wide lint suppression, the configurable indent size,
//! and eventually a whole visual editor over the file. A writer that
//! reformats someone's config on every toggle — dropping their comments,
//! reordering their keys — is worse than no writer at all, because the damage
//! is invisible until they open the file.
//!
//! So edits go through `toml_edit`, which keeps the document's formatting and
//! changes only what it is asked to. Everything here is deliberately narrow:
//! a caller names a table, a key, and a value. There is no "serialize a
//! `ProjectConfig` back out", because that would reintroduce exactly the
//! whole-file rewrite this module exists to avoid.

use toml_edit::{Array, DocumentMut, Item, Value};

/// A parsed `brink.toml` that remembers how it was written.
///
/// Construct from the file's text, apply edits, then [`Self::to_string`] the
/// result back to disk. Anything not touched by an edit comes out byte-identical.
#[derive(Debug, Clone)]
pub struct ConfigDocument {
    doc: DocumentMut,
}

/// Why a document could not be parsed for editing.
#[derive(Debug, thiserror::Error)]
pub enum EditError {
    /// The text is not valid TOML.
    #[error("brink.toml is not valid TOML: {0}")]
    Parse(#[from] toml_edit::TomlError),
    /// A path the edit needs is occupied by a value of the wrong shape —
    /// `[project]` present but not a table, say. Reported rather than
    /// overwritten: clobbering a value the author wrote is the one thing a
    /// config writer must never do silently.
    #[error("brink.toml has `{path}` as {found}, expected {expected}")]
    Shape {
        /// Dotted path to the offending item, e.g. `project.drafts`.
        path: String,
        /// What is actually there.
        found: &'static str,
        /// What the edit needed.
        expected: &'static str,
    },
}

impl ConfigDocument {
    /// Parse `text` for editing.
    ///
    /// # Errors
    /// [`EditError::Parse`] when the text is not valid TOML.
    pub fn parse(text: &str) -> Result<Self, EditError> {
        Ok(Self {
            doc: text.parse::<DocumentMut>()?,
        })
    }

    /// An empty document — for creating a `brink.toml` that does not exist yet.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            doc: DocumentMut::new(),
        }
    }

    /// The document's current text, formatting preserved.
    #[must_use]
    pub fn to_toml_string(&self) -> String {
        self.doc.to_string()
    }

    /// Set `table.key` to an integer, creating the table if absent.
    ///
    /// # Errors
    /// [`EditError::Shape`] when `table` exists as something other than a table.
    pub fn set_integer(&mut self, table: &str, key: &str, value: i64) -> Result<(), EditError> {
        let entry = self.table_mut(table)?;
        Self::assign_keeping_decor(entry, key, toml_edit::value(value));
        Ok(())
    }

    /// Set `table.key` to a string, creating the table if absent.
    ///
    /// # Errors
    /// [`EditError::Shape`] when `table` exists as something other than a table.
    pub fn set_string(&mut self, table: &str, key: &str, value: &str) -> Result<(), EditError> {
        let entry = self.table_mut(table)?;
        Self::assign_keeping_decor(entry, key, toml_edit::value(value));
        Ok(())
    }

    /// Read `table.key` as a string array, or an empty vec when absent.
    ///
    /// # Errors
    /// [`EditError::Shape`] when the key exists but is not an array of strings.
    pub fn string_array(&self, table: &str, key: &str) -> Result<Vec<String>, EditError> {
        let Some(item) = self.doc.get(table).and_then(|t| t.get(key)) else {
            return Ok(Vec::new());
        };
        if item.is_none() {
            return Ok(Vec::new());
        }
        let Some(array) = item.as_array() else {
            return Err(EditError::Shape {
                path: format!("{table}.{key}"),
                found: "a non-array",
                expected: "an array of strings",
            });
        };
        array
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| EditError::Shape {
                        path: format!("{table}.{key}"),
                        found: "an array with a non-string element",
                        expected: "an array of strings",
                    })
            })
            .collect()
    }

    /// Add `value` to the string array at `table.key`, creating both if
    /// absent. A value already present is left alone — and reported, so a
    /// caller can tell "added" from "was already there" without re-reading.
    ///
    /// Returns whether the document changed.
    ///
    /// # Errors
    /// [`EditError::Shape`] when the key exists but is not an array of strings.
    pub fn add_to_string_array(
        &mut self,
        table: &str,
        key: &str,
        value: &str,
    ) -> Result<bool, EditError> {
        if self.string_array(table, key)?.iter().any(|v| v == value) {
            return Ok(false);
        }
        let entry = self.table_mut(table)?;
        if entry.get(key).is_none_or(Item::is_none) {
            entry[key] = Item::Value(Value::Array(Array::new()));
        }
        let item = &mut entry[key];
        let Some(array) = item.as_array_mut() else {
            return Err(EditError::Shape {
                path: format!("{table}.{key}"),
                found: "a non-array",
                expected: "an array of strings",
            });
        };
        array.push(value);
        Ok(true)
    }

    /// Remove `value` from the string array at `table.key`. Returns whether
    /// the document changed.
    ///
    /// # Errors
    /// [`EditError::Shape`] when the key exists but is not an array of strings.
    pub fn remove_from_string_array(
        &mut self,
        table: &str,
        key: &str,
        value: &str,
    ) -> Result<bool, EditError> {
        // A missing key reads back as `Item::None` rather than `None` — the
        // `is_none()` check is what makes "remove from a key that was never
        // there" a no-op instead of a shape error.
        let Some(item) = self.doc.get_mut(table).and_then(|t| t.get_mut(key)) else {
            return Ok(false);
        };
        if item.is_none() {
            return Ok(false);
        }
        let Some(array) = item.as_array_mut() else {
            return Err(EditError::Shape {
                path: format!("{table}.{key}"),
                found: "a non-array",
                expected: "an array of strings",
            });
        };
        let before = array.len();
        array.retain(|v| v.as_str() != Some(value));
        Ok(array.len() != before)
    }

    /// Assign `item` to `key`, keeping the existing value's decoration.
    ///
    /// `toml_edit` attaches a trailing comment to the VALUE, so a plain
    /// `entry[key] = ...` silently drops the comment explaining the setting
    /// you just changed — the exact damage this module exists to prevent.
    fn assign_keeping_decor(entry: &mut Item, key: &str, item: Item) {
        let decor = entry
            .get(key)
            .and_then(Item::as_value)
            .map(|v| v.decor().clone());
        entry[key] = item;
        if let (Some(decor), Some(value)) = (decor, entry[key].as_value_mut()) {
            *value.decor_mut() = decor;
        }
    }

    /// The named table, created (as an implicit-free ordinary table) if absent.
    fn table_mut(&mut self, table: &str) -> Result<&mut Item, EditError> {
        if let Some(existing) = self.doc.get(table) {
            if !existing.is_table_like() {
                return Err(EditError::Shape {
                    path: table.to_owned(),
                    found: "a non-table",
                    expected: "a table",
                });
            }
        } else {
            self.doc[table] = Item::Table(toml_edit::Table::new());
        }
        Ok(&mut self.doc[table])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hand-written config, with everything a formatter would happily
    /// destroy: comments, a blank line, alignment, single quotes, and a
    /// deliberate key order that is not alphabetical.
    const HAND_WRITTEN: &str = "\
# The story's entry point.
[project]
entry = 'main.ink'   # kept deliberately in single quotes
dialect = \"screenplay\"

# Loud about unreachable content.
[lints]
E063 = \"warn\"
";

    #[test]
    fn an_edit_leaves_every_untouched_byte_alone() {
        // The whole reason this module exists. `toml` would return a
        // reformatted document with the comments gone.
        let mut doc = ConfigDocument::parse(HAND_WRITTEN).expect("valid toml");
        doc.add_to_string_array("project", "drafts", "scratch/*.ink")
            .expect("array edit");
        let out = doc.to_toml_string();

        assert!(
            out.contains("# The story's entry point."),
            "leading comment survives"
        );
        assert!(
            out.contains("# kept deliberately in single quotes"),
            "trailing comment survives"
        );
        assert!(out.contains("entry = 'main.ink'"), "quote style survives");
        assert!(
            out.contains("# Loud about unreachable content."),
            "section comment survives"
        );
        assert!(out.contains("E063 = \"warn\""), "unrelated table survives");
        // And the edit landed.
        assert!(out.contains("scratch/*.ink"));
    }

    #[test]
    fn adding_a_value_already_present_changes_nothing() {
        let mut doc = ConfigDocument::parse("[project]\ndrafts = [\"a.ink\"]\n").expect("valid");
        assert!(
            !doc.add_to_string_array("project", "drafts", "a.ink")
                .expect("edit")
        );
        assert_eq!(doc.to_toml_string(), "[project]\ndrafts = [\"a.ink\"]\n");
    }

    #[test]
    fn arrays_and_tables_are_created_when_absent() {
        let mut doc = ConfigDocument::empty();
        assert!(
            doc.add_to_string_array("project", "drafts", "scratch/*.ink")
                .expect("edit")
        );
        assert_eq!(
            doc.string_array("project", "drafts").expect("read"),
            vec!["scratch/*.ink"]
        );
    }

    #[test]
    fn removing_reports_whether_anything_changed() {
        let mut doc =
            ConfigDocument::parse("[project]\ndrafts = [\"a.ink\", \"b.ink\"]\n").expect("valid");
        assert!(
            doc.remove_from_string_array("project", "drafts", "a.ink")
                .expect("edit")
        );
        assert!(
            !doc.remove_from_string_array("project", "drafts", "a.ink")
                .expect("edit")
        );
        assert_eq!(
            doc.string_array("project", "drafts").expect("read"),
            vec!["b.ink"]
        );
    }

    #[test]
    fn removing_from_an_absent_key_is_a_no_op_not_an_error() {
        let mut doc = ConfigDocument::parse("[project]\n").expect("valid");
        assert!(
            !doc.remove_from_string_array("project", "drafts", "a.ink")
                .expect("edit")
        );
    }

    #[test]
    fn set_integer_and_string_round_trip() {
        let mut doc = ConfigDocument::parse(HAND_WRITTEN).expect("valid");
        doc.set_integer("format", "indent", 2).expect("edit");
        doc.set_string("project", "entry", "other.ink")
            .expect("edit");
        let out = doc.to_toml_string();
        assert!(out.contains("indent = 2"));
        assert!(out.contains("other.ink"));
        // Rewriting one value must not cost the comment attached to it.
        assert!(out.contains("# kept deliberately in single quotes"));
    }

    #[test]
    fn a_wrong_shaped_key_is_reported_not_clobbered() {
        // Overwriting what the author wrote is the one thing this must never
        // do quietly, so a scalar where an array belongs is an error.
        let mut doc = ConfigDocument::parse("[project]\ndrafts = \"oops\"\n").expect("valid");
        let err = doc
            .add_to_string_array("project", "drafts", "a.ink")
            .unwrap_err();
        assert!(matches!(err, EditError::Shape { .. }), "got {err:?}");
        assert_eq!(
            doc.to_toml_string(),
            "[project]\ndrafts = \"oops\"\n",
            "document untouched"
        );
    }

    #[test]
    fn a_non_table_where_a_table_belongs_is_reported() {
        let mut doc = ConfigDocument::parse("project = 3\n").expect("valid");
        let err = doc.set_integer("project", "x", 1).unwrap_err();
        assert!(matches!(err, EditError::Shape { .. }), "got {err:?}");
    }

    #[test]
    fn invalid_toml_fails_to_parse_rather_than_being_repaired() {
        assert!(matches!(
            ConfigDocument::parse("[project"),
            Err(EditError::Parse(_))
        ));
    }
}
