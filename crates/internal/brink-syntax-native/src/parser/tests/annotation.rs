//! Annotations `@[…]`. Family for #1198.

use super::*;

#[test]
fn annotation_line_parses() {
    let src = "fn heal(hp) {\n  @[effects(pure, silent, reads(gold, hp))]\n  var x = hp\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}
