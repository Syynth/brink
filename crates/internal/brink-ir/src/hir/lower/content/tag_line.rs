use brink_syntax::ast::{self, AstNode};

use crate::{DiagnosticCode, Tag};

use super::super::context::{LowerScope, LowerSink, Lowered};
use super::super::directive::{self, TagLineClass};
use super::LowerBody;
use super::helpers::{lower_tag, lower_tags};

pub struct TagLineOutput {
    pub tags: Vec<Tag>,
}

impl LowerBody for ast::TagLine {
    type Output = TagLineOutput;

    fn lower_body(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<TagLineOutput> {
        match directive::scan_tag_line(self) {
            TagLineClass::Plain => Ok(TagLineOutput {
                tags: lower_tags(self.tags(), scope, sink),
            }),
            TagLineClass::Directives(_) => {
                // Erasure guarantee: a directive line never lowers to
                // content. Owners (VAR lookback, knot/stitch leading run)
                // read and validate it; anywhere else it's an error.
                if !directive::is_consumed_position(self) {
                    sink.diagnose(self.syntax().text_range(), DiagnosticCode::E045);
                }
                Ok(TagLineOutput { tags: Vec::new() })
            }
            TagLineClass::Mixed => {
                sink.diagnose(self.syntax().text_range(), DiagnosticCode::E047);
                // Keep the plain tags; the directive tags are dropped.
                let tags = self.tags().map_or_else(Vec::new, |t| {
                    t.tags()
                        .filter(|tag| directive::parse_directive_tag(tag).is_none())
                        .map(|tag| lower_tag(&tag, scope, sink))
                        .collect()
                });
                Ok(TagLineOutput { tags })
            }
        }
    }
}
