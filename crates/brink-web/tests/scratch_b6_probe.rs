//! THROWAWAY attribution probe — never committed (#3064 B6).
//! Run: cargo test -p brink-web --release --test scratch_b6_probe -- --nocapture --ignored

use std::fmt::Write as _;
use std::time::Instant;

use brink_web::EditorSession;

fn generate_large_file() -> String {
    let mut s = String::from("VAR large_0 = 0\n\n");
    for k in 0..900 {
        let _ = writeln!(s, "=== big_{k:03} ===");
        let _ = writeln!(s, "Some prose line here for knot {k}.");
        let _ = writeln!(s, "* [One] First option content.");
        let _ = writeln!(s, "* [Two] Second option content.");
        let _ = writeln!(s, "- Gather line.");
        let _ = writeln!(s, "-> DONE\n");
    }
    s
}

fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

#[test]
#[ignore = "throwaway measurement probe"]
#[expect(clippy::print_stdout, reason = "probe: printing is the product")]
fn attribute_wasm_surface_costs() {
    let text = generate_large_file();
    let mut s = EditorSession::new();
    s.update_file("large.ink", &text);
    assert!(s.set_active_file("large.ink"));

    for round in 1..=3u32 {
        let edited = text.replace(
            "Some prose line here for knot 450.",
            &format!("Some prose line here for knot 450, edit {round}."),
        );

        let start = Instant::now();
        s.update_file("large.ink", &edited);
        let t_update = ms(start);

        let start = Instant::now();
        let spans_json = s.hir_spans();
        let t_spans = ms(start);

        let start = Instant::now();
        let tokens_json = s.semantic_tokens();
        let t_tokens = ms(start);

        let start = Instant::now();
        let contexts_json = s.line_contexts();
        let t_contexts = ms(start);

        let start = Instant::now();
        let folds_json = s.folding_ranges();
        let t_folds = ms(start);

        println!(
            "round {round}: update={t_update:.1} spans={t_spans:.1} ({}KB) tokens={t_tokens:.1} ({}KB) contexts={t_contexts:.1} ({}KB) folds={t_folds:.1} ({}KB)",
            spans_json.len() / 1024,
            tokens_json.len() / 1024,
            contexts_json.len() / 1024,
            folds_json.len() / 1024,
        );
    }
}
