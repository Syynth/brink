use super::check_lossless;

/// Regression test for fuzzer-discovered panic in tag parsing.
/// A block comment before a `#` tag caused `bump_assert(HASH)` to fail
/// because `skip_ws` didn't skip comments while `current()` did.
#[test]
fn fuzz_tag_block_comment_before_hash() {
    let src = "#tag /*comment*/ #tag2\n";
    check_lossless(src);
}
