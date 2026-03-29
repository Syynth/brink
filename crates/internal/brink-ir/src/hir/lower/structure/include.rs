//! Include lowering: `lower_include`.

use brink_syntax::ast::{self, AstNode, AstPtr};

use crate::{DiagnosticCode, IncludeSite};

use super::super::context::{LowerSink, Lowered};

pub(super) fn lower_include(
    inc: &ast::IncludeStmt,
    sink: &mut impl LowerSink,
) -> Lowered<IncludeSite> {
    let file_path = inc
        .file_path()
        .ok_or_else(|| sink.diagnose(inc.syntax().text_range(), DiagnosticCode::E011))?;
    let raw = file_path.text();
    let cleaned = raw
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(&raw);
    Ok(IncludeSite {
        file_path: cleaned.to_owned(),
        ptr: AstPtr::new(inc),
    })
}
