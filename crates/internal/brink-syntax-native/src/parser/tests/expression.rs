//! Expressions & precedence. Family for #1193.

use super::*;

#[test]
fn lambda_pipe_tokenizes_and_parses() {
    let src = "var f = |x, y| x + y\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}
