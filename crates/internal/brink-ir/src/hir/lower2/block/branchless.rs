//! `LowerBlock` impl for `ast::BranchlessCondBody`.

use brink_syntax::ast::{self, AstNode};

use crate::Block;

use super::super::backbone::BranchChild;
use super::super::backbone::classify_branch_child;
use super::super::content::{ContentAccumulator, DirectBackend};
use super::super::context::{LowerScope, LowerSink};
use super::LowerBlock;

// ─── BranchlessCondBody ─────────────────────────────────────────────

impl LowerBlock for ast::BranchlessCondBody {
    fn lower_block(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Block {
        let mut acc = ContentAccumulator::new(DirectBackend::new());
        let mut is_multiline = false;

        for child in self.syntax().children_with_tokens() {
            match classify_branch_child(&child) {
                BranchChild::ContentLine(cl) => {
                    acc.handle(&cl, scope, sink);
                }
                BranchChild::LogicLine(ll) => {
                    acc.handle(&ll, scope, sink);
                }
                BranchChild::DivertNode(dn) => {
                    acc.handle(&dn, scope, sink);
                }
                BranchChild::InlineLogic(il) => {
                    acc.handle(&il, scope, sink);
                }
                BranchChild::Text(t) => acc.push_text(t),
                BranchChild::Glue => acc.push_glue(),
                BranchChild::Escape(t) => acc.push_escape(&t),
                BranchChild::Choice(_) | BranchChild::Whitespace(_) | BranchChild::Trivia => {}
                BranchChild::Stop => break,

                BranchChild::Newline => {
                    let was_multiline = is_multiline;
                    is_multiline = true;
                    if acc.has_buffered_parts() {
                        let ends_glue = acc.ends_with_glue();
                        acc.flush();
                        if !ends_glue {
                            acc.push_eol();
                        }
                    } else if acc.last_was_content() || !was_multiline {
                        acc.push_eol();
                    }
                }
            }
        }

        acc.flush();
        if is_multiline && acc.last_was_content() {
            acc.push_eol();
        }
        acc.finish()
    }
}
