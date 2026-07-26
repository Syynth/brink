use rowan::TextSize;
use wasm_bindgen::prelude::*;

use super::{EditorSession, ViewContext};
use crate::editor_dto::HoverInfoJs;

#[wasm_bindgen]
impl EditorSession {
    /// Compute hover info for a document handle at the given offset. Returns JSON or "null".
    pub fn hover_doc(&self, doc: u32, offset: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "null".to_owned();
        };
        self.hover_impl(&d.path, d.view.as_ref(), offset)
    }

    /// Compute hover info at the given byte offset. Returns JSON or "null".
    pub fn hover(&self, offset: u32) -> String {
        self.hover_impl(&self.active_path, self.view.as_ref(), offset)
    }
}

impl EditorSession {
    fn hover_impl(&self, path: &str, view: Option<&ViewContext>, offset: u32) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "null".to_owned();
        };
        let (Some(analysis), Some(source)) =
            (self.session.analysis(), self.session.source(file_id))
        else {
            return "null".to_owned();
        };

        // Every file in the session, not just the hovered one (#1553):
        // `brink_ide::hover` looks the *definition's* file up in here to
        // render "Defined in `path`", and `ufcs_hover` does the same for a
        // method's declaring file. A single-entry set can only ever match a
        // same-file definition, so cross-file hover text could never render
        // in the web editor. `file_metadata` is the same `(FileId, path,
        // source)` shape the LSP's `NavigationSnapshot` builds, sorted by
        // `FileId`; the wasm session holds one project, so no per-project
        // scoping applies here.
        let project_files = self.session.db().file_metadata();

        let abs_offset = self.to_absolute(path, view, offset);
        match brink_ide::hover::hover(
            analysis,
            self.session.db(),
            file_id,
            source,
            TextSize::new(abs_offset),
            &project_files,
        ) {
            Some(info) => {
                let js = HoverInfoJs {
                    content: info.content,
                    start: info
                        .range
                        .and_then(|r| self.to_relative(path, view, r.start().into())),
                    end: info
                        .range
                        .and_then(|r| self.to_relative(path, view, r.end().into())),
                };
                serde_json::to_string(&js).unwrap_or_default()
            }
            None => "null".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::EditorSession;

    /// `market/barter.brink` — the definition side of the file boundary.
    const BARTER: &str = "\
var gold = 10

/// Trade at the market stall.
flow haggle() {
  You haggle over the price.
}
";

    /// `main.brink` — the reference side: a divert into the other file.
    const MAIN: &str = "\
use story::market::barter::haggle;

flow start() {
  The market is busy.
  -> haggle
}
";

    /// Cross-file "Defined in `path`" hover in the web editor (#1553).
    /// `hover_impl` used to hand `brink_ide::hover` only the hovered file, so
    /// the definition's file was never in the lookup set and the note could
    /// not render — a defect invisible to any same-file hover test.
    #[test]
    fn hover_names_the_defining_file_across_the_project() {
        let mut session = EditorSession::new();
        session.update_file("market/barter.brink", BARTER);
        session.update_file("main.brink", MAIN);
        assert!(session.set_active_file("main.brink"));

        let offset = u32::try_from(MAIN.find("haggle\n}").expect("divert target")).expect("offset");
        let json = session.hover(offset);
        let v: serde_json::Value = serde_json::from_str(&json).expect("hover JSON");
        let content = v["content"].as_str().unwrap_or_default();

        assert!(
            content.contains("*Defined in `market/barter.brink`*"),
            "hover must name the defining file: {json}"
        );
    }
}
