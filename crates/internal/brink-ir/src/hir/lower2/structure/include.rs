//! Include lowering: `lower_include`.

use brink_syntax::ast::{self, AstNode, AstPtr};

use crate::{DiagnosticCode, IncludeSite};

use super::super::context::LowerSink;

pub(super) fn lower_include(
    inc: &ast::IncludeStmt,
    sink: &mut impl LowerSink,
) -> Option<IncludeSite> {
    let file_path = inc.file_path().or_else(|| {
        sink.diagnose(inc.syntax().text_range(), DiagnosticCode::E011);
        None
    })?;
    let raw = file_path.text();
    let cleaned = raw
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(&raw);
    Some(IncludeSite {
        file_path: cleaned.to_owned(),
        ptr: AstPtr::new(inc),
    })
}
