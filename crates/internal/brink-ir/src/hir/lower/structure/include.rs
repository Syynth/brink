//! Include lowering: `lower_include`.

use brink_syntax::ast::{self, AstNode};

use crate::IncludeSite;
use crate::provenance::NodeClass;

use super::super::context::{LowerScope, LowerSink, Lowered};

#[expect(clippy::unnecessary_wraps)]
pub(super) fn lower_include(
    scope: &LowerScope,
    inc: &ast::IncludeStmt,
    _sink: &mut impl LowerSink,
) -> Lowered<IncludeSite> {
    // The parser always materializes a FILE_PATH node inside INCLUDE_STMT
    // (possibly empty) and reports E037 if it's missing. So file_path() is
    // guaranteed to return Some (lane-A audit, #709: E011 is unreachable).
    let Some(file_path) = inc.file_path() else {
        // Unreachable: parser always creates FILE_PATH. E011 is retired.
        unreachable!("parser guarantees FILE_PATH node in INCLUDE_STMT")
    };
    let raw = file_path.text();
    let cleaned = raw
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(&raw);
    Ok(IncludeSite {
        file_path: cleaned.to_owned(),
        ptr: scope.prov(NodeClass::Include, inc.syntax()),
    })
}
