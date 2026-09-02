//! `passage_lines` (#3408): the content lines of a knot/stitch, for the
//! Conventions editor's teach-by-example marking list. See
//! `brink_ide::passage` for what counts as a line.

use serde::Serialize;
use wasm_bindgen::prelude::*;

use super::EditorSession;

#[derive(Serialize)]
struct PassageLineJs<'a> {
    text: String,
    tags: Vec<String>,
    file: &'a str,
    /// One-based, for humans; `brink_ide::passage::PassageLine::line` is
    /// zero-based.
    line: u32,
    origin: brink_ide::passage::PassageOrigin,
}

#[wasm_bindgen]
impl EditorSession {
    /// The content lines of `path` (`knot` or `knot.stitch`), searched across
    /// every project file in id order. Returns JSON `PassageLine[]`, or
    /// `null` when no file declares that path. Tags ride separately from
    /// the text; knot/global tag blocks, logic, diverts and comments are not
    /// lines. Both source surfaces resolve through the db's line contexts.
    pub fn passage_lines(&self, path: &str) -> String {
        crate::perf::time("ide.passageLines", || self.passage_lines_inner(path))
    }
}

impl EditorSession {
    fn passage_lines_inner(&self, path: &str) -> String {
        let db = self.session.db();
        for file_id in db.file_ids() {
            let Some(hir) = self.session.hir(file_id) else {
                continue;
            };
            let source = self.session.source(file_id).unwrap_or("");
            let Some(contexts) = self.session.line_contexts(file_id) else {
                continue;
            };
            let Some(lines) = brink_ide::passage::passage_lines(hir, source, &contexts, path)
            else {
                continue;
            };
            let file = self.session.file_path(file_id).unwrap_or("");
            let items: Vec<PassageLineJs<'_>> = lines
                .into_iter()
                .map(|l| PassageLineJs {
                    text: l.text,
                    tags: l.tags,
                    file,
                    line: l.line + 1,
                    origin: l.origin,
                })
                .collect();
            return serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_owned());
        }
        "null".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::EditorSession;

    #[test]
    fn passage_lines_finds_the_declaring_file_and_reports_one_based_lines() {
        let mut s = EditorSession::new();
        s.update_file(
            "main.ink",
            "INCLUDE east.ink\n=== start ===\nHello. # wave\n-> east\n",
        );
        s.update_file(
            "east.ink",
            "=== east ===\n@MARA: <>\nGo.\n= gate\n* Lisa: Which way?\n",
        );
        let v: serde_json::Value = serde_json::from_str(&s.passage_lines("east.gate")).unwrap();
        assert_eq!(v[0]["text"], "Lisa: Which way?");
        assert_eq!(v[0]["file"], "east.ink");
        assert_eq!(v[0]["line"], 5);
        assert_eq!(v[0]["origin"], "choice");
        assert_eq!(v[0]["tags"].as_array().unwrap().len(), 0);

        let v: serde_json::Value = serde_json::from_str(&s.passage_lines("start")).unwrap();
        assert_eq!(v[0]["text"], "Hello.");
        assert_eq!(v[0]["tags"][0], "wave");
        assert_eq!(v.as_array().unwrap().len(), 1, "the divert is not a line");
    }

    #[test]
    fn passage_lines_is_null_for_an_unknown_path() {
        let mut s = EditorSession::new();
        s.update_file("main.ink", "=== start ===\nHello.\n");
        assert_eq!(s.passage_lines("nowhere"), "null");
        assert_eq!(s.passage_lines("start.nowhere"), "null");
    }
}
