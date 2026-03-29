use brink_syntax::ast;

use crate::Tag;

use super::super::context::{LowerScope, LowerSink, Lowered};
use super::LowerBody;
use super::helpers::lower_tags;

pub struct TagLineOutput {
    pub tags: Vec<Tag>,
}

impl LowerBody for ast::TagLine {
    type Output = TagLineOutput;

    fn lower_body(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<TagLineOutput> {
        Ok(TagLineOutput {
            tags: lower_tags(self.tags(), scope, sink),
        })
    }
}
