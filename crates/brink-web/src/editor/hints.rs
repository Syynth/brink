use rowan::{TextRange, TextSize};
use wasm_bindgen::prelude::*;

use super::{EditorSession, ViewContext};
use crate::editor_dto::{
    CallWidgetSiteJs, ColorHintJs, DeclaredGroupJs, GroupStateJs, GroupWidgetSiteJs, InlayHintJs,
    ParamLabelJs, SignatureInfoJs, SlotStateJs, SlotWidgetJs, ValueItemJs, declared_group_js,
    inlay_hint_kind_str,
};

#[wasm_bindgen]
impl EditorSession {
    /// Compute inlay hints for a document handle. Returns JSON array.
    pub fn inlay_hints_doc(&self, doc: u32, start: u32, end: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "[]".to_owned();
        };
        self.inlay_hints_impl(&d.path, d.view.as_ref(), start, end)
    }

    /// Compute inlay hints. Returns JSON array.
    pub fn inlay_hints(&self, start: u32, end: u32) -> String {
        self.inlay_hints_impl(&self.active_path, self.view.as_ref(), start, end)
    }

    /// Color hints (`hex_color` argument literals) for a document handle, for
    /// the built-in color picker (#174-adjacent). Returns JSON array of
    /// `{ start, end, value }` (UTF-16 offsets).
    pub fn color_hints_doc(&self, doc: u32, start: u32, end: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "[]".to_owned();
        };
        self.color_hints_impl(&d.path, d.view.as_ref(), start, end)
    }

    /// Argument-widget sites for a document handle (argument-widget spec §4):
    /// every call's per-parameter slots + state (Filled / Empty / Expr), for
    /// inline editing and empty-slot filling. Returns a JSON array of
    /// `{ callee, slots: [{ param_name, widget?, type_name?, type_display?, state }] }`
    /// (UTF-16 offsets). `type_display` (#1027/#1053) is the honest render of
    /// `type_name` — a warning marker for an unregistered semantic type; the
    /// Form must render it instead of the raw `type_name`.
    pub fn argument_widgets_doc(&self, doc: u32, start: u32, end: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "[]".to_owned();
        };
        self.argument_widgets_impl(&d.path, d.view.as_ref(), start, end)
    }

    /// Compute signature help for a document handle. Returns JSON or "null".
    pub fn signature_help_doc(&self, doc: u32, offset: u32) -> String {
        let Some(d) = self.docs.get(&doc) else {
            return "null".to_owned();
        };
        self.signature_help_impl(&d.path, d.view.as_ref(), offset)
    }

    /// Compute signature help. Returns JSON or "null".
    pub fn signature_help(&self, offset: u32) -> String {
        self.signature_help_impl(&self.active_path, self.view.as_ref(), offset)
    }
}

impl EditorSession {
    fn inlay_hints_impl(
        &self,
        path: &str,
        view: Option<&ViewContext>,
        start: u32,
        end: u32,
    ) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let (Some(analysis), Some(root)) =
            (self.session.analysis(), self.session.syntax_root(file_id))
        else {
            return "[]".to_owned();
        };

        let abs_start = self.to_absolute(path, view, start);
        let abs_end = self.to_absolute(path, view, end);
        let range = TextRange::new(TextSize::new(abs_start), TextSize::new(abs_end));
        let hints = brink_ide::inlay_hints::inlay_hints(
            &root,
            analysis,
            self.session.db(),
            file_id,
            range,
            Some(self.session.host_values()),
        );

        let items: Vec<InlayHintJs> = hints
            .iter()
            .filter_map(|h| {
                let offset = self.to_relative(path, view, h.offset.into())?;
                Some(InlayHintJs {
                    offset,
                    label: h.label.clone(),
                    kind: inlay_hint_kind_str(&h.kind).to_owned(),
                    padding_right: h.padding_right,
                })
            })
            .collect();

        serde_json::to_string(&items).unwrap_or_default()
    }

    fn color_hints_impl(
        &self,
        path: &str,
        view: Option<&ViewContext>,
        start: u32,
        end: u32,
    ) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let (Some(analysis), Some(root)) =
            (self.session.analysis(), self.session.syntax_root(file_id))
        else {
            return "[]".to_owned();
        };

        let abs_start = self.to_absolute(path, view, start);
        let abs_end = self.to_absolute(path, view, end);
        let range = TextRange::new(TextSize::new(abs_start), TextSize::new(abs_end));
        let hints = brink_ide::color::color_hints(&root, analysis, range);

        let items: Vec<ColorHintJs> = hints
            .iter()
            .filter_map(|h| {
                let start = self.to_relative(path, view, h.start.into())?;
                let end = self.to_relative(path, view, h.end.into())?;
                Some(ColorHintJs {
                    start,
                    end,
                    value: h.value.clone(),
                })
            })
            .collect();

        serde_json::to_string(&items).unwrap_or_default()
    }

    fn argument_widgets_impl(
        &self,
        path: &str,
        view: Option<&ViewContext>,
        start: u32,
        end: u32,
    ) -> String {
        use brink_ide::argument_widgets::SlotState;
        let Some(file_id) = self.session.file_id(path) else {
            return "[]".to_owned();
        };
        let (Some(analysis), Some(root)) =
            (self.session.analysis(), self.session.syntax_root(file_id))
        else {
            return "[]".to_owned();
        };

        let abs_start = self.to_absolute(path, view, start);
        let abs_end = self.to_absolute(path, view, end);
        let range = TextRange::new(TextSize::new(abs_start), TextSize::new(abs_end));
        let sites = brink_ide::argument_widgets::argument_widgets(
            &root,
            analysis,
            range,
            Some(self.session.host_values()),
        );

        let out: Vec<CallWidgetSiteJs> = sites
            .iter()
            .map(|site| {
                let slots = site
                    .slots
                    .iter()
                    .map(|slot| {
                        // Map byte offsets to UTF-16; a slot whose offsets fall
                        // outside the view degrades to a non-actionable Expr.
                        let state = match &slot.state {
                            SlotState::Filled { start, end, value } => {
                                match (
                                    self.to_relative(path, view, (*start).into()),
                                    self.to_relative(path, view, (*end).into()),
                                ) {
                                    (Some(start), Some(end)) => SlotStateJs::Filled {
                                        start,
                                        end,
                                        value: value.clone(),
                                    },
                                    _ => SlotStateJs::Expr,
                                }
                            }
                            SlotState::Empty {
                                insert_at,
                                needs_leading_comma,
                            } => match self.to_relative(path, view, (*insert_at).into()) {
                                Some(insert_at) => SlotStateJs::Empty {
                                    insert_at,
                                    needs_leading_comma: *needs_leading_comma,
                                },
                                None => SlotStateJs::Expr,
                            },
                            SlotState::Expr => SlotStateJs::Expr,
                        };
                        SlotWidgetJs {
                            param_name: slot.param_name.clone(),
                            widget: slot.widget.clone(),
                            type_name: slot.type_name.clone(),
                            type_display: slot.type_display.clone(),
                            values: slot
                                .values
                                .iter()
                                .map(|v| ValueItemJs {
                                    value: v.value.clone(),
                                    label: v.label.clone(),
                                    detail: v.detail.clone(),
                                })
                                .collect(),
                            state,
                        }
                    })
                    .collect();
                // The call-name span (UTF-16) anchors the form glyph; default to
                // 0 if it falls outside the view (the studio guards end > start).
                let name_start = self
                    .to_relative(path, view, site.name_start.into())
                    .unwrap_or(0);
                let name_end = self
                    .to_relative(path, view, site.name_end.into())
                    .unwrap_or(0);

                // Arg-group widgets (UTF-16); a group with an out-of-view span is
                // dropped (it stays a per-slot affordance).
                let groups: Vec<GroupWidgetSiteJs> = site
                    .groups
                    .iter()
                    .filter_map(|g| self.group_widget_js(path, view, g))
                    .collect();

                // Declared groups carry no document spans, so they need no view
                // translation — the Form renders them and seeds from `slots`.
                let declared_groups: Vec<DeclaredGroupJs> =
                    site.declared_groups.iter().map(declared_group_js).collect();

                CallWidgetSiteJs {
                    callee: site.callee.clone(),
                    name_start,
                    name_end,
                    slots,
                    groups,
                    declared_groups,
                }
            })
            .collect();

        serde_json::to_string(&out).unwrap_or_default()
    }

    /// Map one arg-group widget to its JSON shape (UTF-16); `None` when a span
    /// falls outside the view (the group degrades to per-slot affordances).
    fn group_widget_js(
        &self,
        path: &str,
        view: Option<&ViewContext>,
        g: &brink_ide::argument_widgets::GroupWidgetSite,
    ) -> Option<GroupWidgetSiteJs> {
        use brink_ide::argument_widgets::GroupState;
        let state = match &g.state {
            GroupState::Filled { spans, values } => {
                let mut js_spans = Vec::with_capacity(spans.len());
                for (s, e) in spans {
                    js_spans.push((
                        self.to_relative(path, view, (*s).into())?,
                        self.to_relative(path, view, (*e).into())?,
                    ));
                }
                GroupStateJs::Filled {
                    spans: js_spans,
                    values: values.clone(),
                }
            }
            GroupState::Empty {
                insert_at,
                needs_leading_comma,
            } => GroupStateJs::Empty {
                insert_at: self.to_relative(path, view, (*insert_at).into())?,
                needs_leading_comma: *needs_leading_comma,
            },
        };
        Some(GroupWidgetSiteJs {
            ty: g.ty.clone(),
            surface: g.surface.clone(),
            param_indices: g.param_indices.clone(),
            param_names: g.param_names.clone(),
            state,
            context: g.context.iter().cloned().collect(),
            context_params: g.context_params.iter().cloned().collect(),
        })
    }

    fn signature_help_impl(&self, path: &str, view: Option<&ViewContext>, offset: u32) -> String {
        let Some(file_id) = self.session.file_id(path) else {
            return "null".to_owned();
        };
        let (Some(analysis), Some(source)) =
            (self.session.analysis(), self.session.source(file_id))
        else {
            return "null".to_owned();
        };

        let abs_offset = self.to_absolute(path, view, offset);
        match brink_ide::signature::signature_help_with_dialect(
            analysis,
            source,
            abs_offset as usize,
            self.dialect,
        ) {
            Some(info) => {
                let js = SignatureInfoJs {
                    label: info.label,
                    documentation: info.documentation,
                    parameters: info
                        .parameters
                        .iter()
                        .map(|p| ParamLabelJs {
                            label: p.label.clone(),
                        })
                        .collect(),
                    active_parameter: info.active_parameter,
                };
                serde_json::to_string(&js).unwrap_or_default()
            }
            None => "null".to_owned(),
        }
    }
}
