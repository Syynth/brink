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

        let project_files = [(file_id, path.to_owned(), source.to_owned())];

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
