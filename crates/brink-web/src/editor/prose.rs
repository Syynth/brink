//! The project's proper nouns, for the prose checker's dictionary (#3210).
//!
//! Measured with the curated dictionary alone, `"Kaelen nodded at the
//! warden."` reports `Kaelen` as a misspelling and suggests *Karen*. Every
//! invented name in a manuscript, underlined. A checker that does that to
//! fiction is switched off in the first session and never switched back on,
//! so this is not a refinement of the feature — it is the feature.
//!
//! **The manuscript already says who its characters are.** A cue line is
//! structural, not prose: `KAELEN` above a line of dialogue is a claimed
//! convention the compiler resolves, and the dialect classification captures
//! the speaker as a named attribute. So writing the story teaches the
//! dictionary, with no author action, no settings page, and no word list to
//! maintain. That is the part a general-purpose spell checker cannot do, and
//! it is why this query lives next to the analysis rather than in the editor.
//!
//! Two sources, both project-wide:
//!
//! 1. **Declared names** — knots, stitches, externals, structs, variables,
//!    lists. Whatever the author named, they meant.
//! 2. **Dialect captures** — `speaker` attributes and character-cue content,
//!    which is where the cast lives.
//!
//! Mounted `std/` files are excluded: they are not the author's project, and
//! their identifiers would put library vocabulary into a manuscript's
//! dictionary.

use wasm_bindgen::prelude::*;

use super::EditorSession;

/// The shortest run of letters worth adding.
///
/// A one-letter token is never a proper noun and is already in any
/// dictionary worth the name; admitting them would let `a` and `I` in from
/// every identifier that happens to contain one.
const MIN_WORD_LEN: usize = 2;

#[wasm_bindgen]
impl EditorSession {
    /// The project's proper nouns, as a JSON array of words, sorted and
    /// deduplicated.
    ///
    /// Sorted for determinism — this feeds a cache key on the editor side,
    /// and `HashSet` iteration order would make an identical project produce
    /// a different request every time.
    #[must_use]
    pub fn prose_dictionary(&self) -> String {
        serde_json::to_string(&self.prose_dictionary_list()).unwrap_or_else(|_| "[]".to_owned())
    }

    pub(crate) fn prose_dictionary_list(&self) -> Vec<String> {
        let mut words: Vec<String> = Vec::new();

        let db = self.session.db();
        for file_id in db.file_ids() {
            // A mounted stdlib file is not the author's project.
            if self.mounted_std_ids.contains(&file_id) {
                continue;
            }

            if let (Some(hir), Some(manifest), Some(source)) = (
                self.session.hir(file_id),
                self.session.manifest(file_id),
                self.session.source(file_id),
            ) {
                for symbol in brink_ide::document::document_symbols(hir, manifest, source) {
                    collect_symbol_words(&symbol, &mut words);
                }
            }

            // The cast. `speaker` is the conventional capture name for a
            // character cue; a dialect may name it otherwise, so the
            // character-kind content is taken as well rather than trusting
            // one spelling.
            let Some(contexts) = self.session.line_contexts(file_id) else {
                continue;
            };
            let source = self.session.source(file_id).unwrap_or("");
            let lines: Vec<&str> = source.lines().collect();
            for (idx, context) in contexts.iter().enumerate() {
                let Some(dialect) = context.dialect.as_ref() else {
                    continue;
                };
                for (name, value) in &dialect.attrs {
                    if name == "speaker" {
                        push_words(value, &mut words);
                    }
                }
                if dialect.kind == "character"
                    && let Some(line) = lines.get(idx)
                {
                    push_words(line, &mut words);
                }
            }
        }

        words.sort_unstable();
        words.dedup();
        words
    }
}

/// Walk a document symbol and its children, harvesting name words.
fn collect_symbol_words(symbol: &brink_ide::document::DocumentSymbol, out: &mut Vec<String>) {
    push_words(&symbol.name, out);
    for child in &symbol.children {
        collect_symbol_words(child, out);
    }
}

/// Split `raw` into alphabetic runs and push each.
///
/// Splitting is required, not cosmetic: Harper's dictionary is keyed by word,
/// so a cue of `MARKET SQUARE` added whole would match nothing, and an
/// identifier like `warden_golem` has to become `warden` and `golem` to help
/// the prose that spells them apart. Case is preserved — the dictionary
/// case-folds for lookup, and the metadata this feeds marks them proper
/// nouns.
fn push_words(raw: &str, out: &mut Vec<String>) {
    let mut current = String::new();
    for ch in raw.chars() {
        if ch.is_alphabetic() || ch == '\'' {
            current.push(ch);
        } else if !current.is_empty() {
            take_word(&mut current, out);
        }
    }
    take_word(&mut current, out);
}

fn take_word(current: &mut String, out: &mut Vec<String>) {
    // A trailing apostrophe is punctuation, not part of the name.
    let trimmed = current.trim_matches('\'');
    if trimmed.chars().count() >= MIN_WORD_LEN {
        out.push(trimmed.to_owned());
    }
    current.clear();
}

#[cfg(test)]
mod tests {
    use super::push_words;
    use crate::editor::EditorSession;

    fn words(raw: &str) -> Vec<String> {
        let mut out = Vec::new();
        push_words(raw, &mut out);
        out
    }

    #[test]
    fn splits_names_into_the_words_a_dictionary_is_keyed_by() {
        assert_eq!(words("MARKET SQUARE"), vec!["MARKET", "SQUARE"]);
        assert_eq!(words("warden_golem"), vec!["warden", "golem"]);
        assert_eq!(words("Kaelen"), vec!["Kaelen"]);
    }

    #[test]
    fn keeps_internal_apostrophes_and_drops_edge_ones() {
        assert_eq!(words("Ka'len's"), vec!["Ka'len's"]);
        assert_eq!(words("'quoted'"), vec!["quoted"]);
    }

    #[test]
    fn drops_single_letters_and_digits() {
        // `a` and `1` are not proper nouns, and admitting them would let a
        // letter in from every identifier that contains one.
        assert_eq!(words("a1 b2 Cx"), vec!["Cx"]);
        assert!(words("42").is_empty());
    }

    #[test]
    fn harvests_declared_names_from_the_project() {
        let mut session = EditorSession::new();
        session.update_file("main.ink", "=== kaelen_intro ===\nHello.\n-> END\n");
        let _ = session.compile_project("main.ink");

        let dictionary = session.prose_dictionary_list();
        assert!(
            dictionary.iter().any(|w| w == "kaelen"),
            "the knot's name should reach the dictionary; got {dictionary:?}"
        );
    }

    #[test]
    fn is_sorted_and_deduplicated() {
        // Determinism matters: this feeds a request the editor caches on, and
        // an unstable order would make an unchanged project look changed.
        let mut session = EditorSession::new();
        session.update_file(
            "main.ink",
            "=== zeta ===\n-> alpha\n=== alpha ===\n-> END\n",
        );
        let _ = session.compile_project("main.ink");

        let dictionary = session.prose_dictionary_list();
        let mut sorted = dictionary.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(dictionary, sorted);
    }
}
