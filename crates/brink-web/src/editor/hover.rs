use rowan::TextSize;
use wasm_bindgen::prelude::*;

use super::{EditorSession, ViewContext};
use crate::editor_dto::{HoverInfoJs, LocationJs};

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
                // Link targets are absolute positions in their OWN file, and
                // that file is usually not the one being viewed — so they are
                // deliberately not run through `to_relative`, which maps into
                // the current view's coordinate space. The host opens the
                // file and reveals the range.
                // `map`, never `filter_map`: the markdown refers to these by
                // INDEX, so dropping an unresolvable entry would shift every
                // later one and silently navigate to the wrong place. An
                // unresolvable file yields an empty `file`, which the host
                // renders as plain text instead of a link.
                let links = info
                    .links
                    .iter()
                    .map(|l| LocationJs {
                        file: project_files
                            .iter()
                            .find(|(fid, _, _)| *fid == l.file)
                            .map_or_else(String::new, |(_, p, _)| p.clone()),
                        start: l.range.start().into(),
                        end: l.range.end().into(),
                    })
                    .collect();
                let js = HoverInfoJs {
                    content: info.content,
                    start: info
                        .range
                        .and_then(|r| self.to_relative(path, view, r.start().into())),
                    end: info
                        .range
                        .and_then(|r| self.to_relative(path, view, r.end().into())),
                    links,
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
    /// `pub` (issue #1582, RULED 2026-08-03) is load-bearing here, not
    /// decorative: before that keyword existed, every native declaration
    /// lowered with `visibility: None`, which a declared module (native
    /// modules are always declared) treats as `Private` — `E087`
    /// unconditionally, `use` or not. Marking `haggle` `pub` is what makes
    /// `main.brink`'s cross-module reference legal at all.
    const BARTER: &str = "\
var gold = 10

/// Trade at the market stall.
pub flow haggle() {
  You haggle over the price.
}
";

    /// `docks/barter.brink` — a second, unrelated module that `pub`-exports
    /// a `haggle` *homonym*. Its presence is the disambiguation this test
    /// exists to force: a bare-name fallback (`lookup_by_name`) can only
    /// ever return one first-winner to every importer, so if hover still
    /// names `market/barter.brink` with this file also loaded, resolution
    /// is provably following `main.brink`'s qualified `use` path by
    /// equality, not guessing from the leaf name alone.
    const DOCKS_BARTER: &str = "\
/// Trade at the docks.
pub flow haggle() {
  You haggle over crates on the pier.
}
";

    /// `main.brink` — the reference side: a qualified `use` naming the
    /// market's `haggle` specifically, then a divert into it.
    const MAIN: &str = "\
use story::market::barter::haggle;

flow start() {
  The market is busy.
  -> haggle
}
";

    /// Cross-file "Defined in `path`" hover in the web editor (#1553),
    /// proven **E087-free and import-driven** end to end through the real
    /// `@brink-lang/web` surface (`EditorSession::hover`) — the acceptance
    /// criterion #1582's owner asked for (2026-07-26 comment): a
    /// same-leaf-name symbol in two modules that only a real `use` import
    /// can disambiguate. `hover_impl` used to hand `brink_ide::hover` only
    /// the hovered file, so the definition's file was never in the lookup
    /// set and the note could not render — a defect invisible to any
    /// same-file hover test.
    #[test]
    fn hover_names_the_defining_file_across_the_project() {
        let mut session = EditorSession::new();
        session.update_file("market/barter.brink", BARTER);
        session.update_file("docks/barter.brink", DOCKS_BARTER);
        session.update_file("main.brink", MAIN);
        assert!(session.set_active_file("main.brink"));

        let analysis = session.session.analysis().expect("analysis");
        assert!(
            !analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E087),
            "pub (#1582) must license this cross-module use import E087-free: {:?}",
            analysis.diagnostics
        );

        let offset = u32::try_from(MAIN.find("haggle\n}").expect("divert target")).expect("offset");
        let json = session.hover(offset);
        let v: serde_json::Value = serde_json::from_str(&json).expect("hover JSON");
        let content = v["content"].as_str().unwrap_or_default();

        assert!(
            // The path is a LINK now (#3255 decision 5): `*Defined in*
            // [`path`](#N)`.
            content.contains("*Defined in* [`market/barter.brink`](#"),
            "hover must resolve to the market's haggle, not the docks' homonym: {json}"
        );
    }
}
