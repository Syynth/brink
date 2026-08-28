//! The project's proper nouns, for the prose checker's dictionary (#3210).
//!
//! Measured with the curated dictionary alone, `"Kaelen nodded at the
//! warden."` reports `Kaelen` as a misspelling and suggests *Karen*. Every
//! invented name in a manuscript, underlined. A checker that does that to
//! fiction is switched off in the first session and never switched back on,
//! so this is not a refinement of the feature — it is the feature.
//!
//! Two sources, both project-wide:
//!
//! 1. **Declared names** — knots, stitches, externals, structs, variables,
//!    lists. Whatever the author named, they meant.
//! 2. **Dialect captures** — `speaker` attributes and character-cue content.
//!
//! ⚠ **Source 2 does not fire on the native surface, and that is a known
//! defect, not a design choice.** It reads `LineContext.dialect`, which is
//! populated only by a host-registered `DialogueDialect` (#368) — the
//! studio's default being the ink `@Name:<>` at-cue preset. A native
//! project's cues are claimed by `@[convention(claims = "…")]` handlers
//! instead, and that mechanism populates `LineContext.dialect` on no line
//! at all. Measured: a project with a `cue` convention and a `GRISWOLD`
//! cue line harvests `["Cue", "cue", "main"]` — the struct, the handler and
//! the flow, but not the character.
//!
//! So for a `.brink` project every character name is currently underlined,
//! and the author's own list in `[prose] dictionary` is the only remedy.
//! Fixing it needs a record of which convention claimed a line, which
//! nothing carries today. Do not restore the claim that the manuscript
//! teaches the dictionary until that record exists.
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
    fn the_configured_dictionary_comes_from_the_prose_table() {
        // The author's own word list — everything the symbol table cannot
        // know. It lives in `brink.toml` rather than a sidecar so it is
        // shared by collaborators and survives a fresh clone (decision log,
        // "Prose dictionary lives in `brink.toml`").
        let mut session = EditorSession::new();
        session
            .apply_project_config("[prose]\ndictionary = [\"Griswold\", \"Ashfen\"]\n")
            .expect("valid config");
        let words: Vec<String> =
            serde_json::from_str(&session.configured_prose_dictionary()).expect("json");
        assert_eq!(words, vec!["Griswold", "Ashfen"]);
    }

    #[test]
    fn the_configured_dictionary_is_in_file_order_not_sorted() {
        // It is the author's list, shown back to them in the settings panel.
        // Sorting here would disagree with their file whenever they grouped
        // it by hand.
        let mut session = EditorSession::new();
        session
            .apply_project_config("[prose]\ndictionary = [\"Zeb\", \"Ada\"]\n")
            .expect("valid config");
        let words: Vec<String> =
            serde_json::from_str(&session.configured_prose_dictionary()).expect("json");
        assert_eq!(words, vec!["Zeb", "Ada"]);
    }

    #[test]
    fn a_config_without_a_dictionary_clears_the_previous_one() {
        // Wholesale-replace, like every other configured_* field: a word
        // removed from the file must stop being a known word, or "remove
        // from dictionary" appears to do nothing until a reload.
        let mut session = EditorSession::new();
        session
            .apply_project_config("[prose]\ndictionary = [\"Griswold\"]\n")
            .expect("valid config");
        session
            .apply_project_config("[prose]\ndialect = \"british\"\n")
            .expect("valid config");
        assert_eq!(session.configured_prose_dictionary(), "[]");
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
