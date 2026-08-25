//! THROWAWAY attribution probe — never committed (#3064 B6). Mimics the
//! wasm `hir_spans_doc` / `line_contexts_doc` conversion + JSON
//! serialization shape to split query cost from payload cost.
//! Run: cargo test -p brink-db --release --test scratch_b6_probe -- --nocapture --ignored

use std::fmt::Write as _;
use std::time::Instant;

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

#[derive(serde::Serialize)]
struct SpanJs {
    start_line: u32,
    start_char: u32,
    end_line: u32,
    end_char: u32,
    kind: u8,
    depth: u32,
    handle: Option<u32>,
}

#[test]
#[ignore = "throwaway measurement probe"]
#[expect(clippy::print_stdout, reason = "probe: printing is the product")]
fn attribute_payload_costs() {
    let text = generate_large_file();
    let mut db = brink_db::ProjectDb::new();
    let id = db.update_file("large.ink", text.clone());
    let _ = db.analysis();

    for round in 1..=3u32 {
        let edited = text.replace(
            "Some prose line here for knot 450.",
            &format!("Some prose line here for knot 450, edit {round}."),
        );
        db.update_file("large.ink", edited.clone());
        let _ = db.analysis();

        let start = Instant::now();
        let projection = db.projection(id).expect("projection");
        let t_proj = ms(start);

        let source = db.source(id).expect("source").to_owned();
        let start = Instant::now();
        let idx = brink_ir::LineIndex::new(&source);
        let spans: Vec<SpanJs> = projection
            .spans
            .iter()
            .map(|s| {
                let (sl, sc) = idx.line_col(s.range.start());
                let el = if s.kind.is_container() {
                    brink_ir::hir::projection::tight_container_end_line(&idx, &source, s.range)
                } else {
                    idx.line_col(s.range.end()).0
                };
                SpanJs {
                    start_line: sl,
                    start_char: sc,
                    end_line: el,
                    end_char: 0,
                    kind: 0,
                    depth: s.depth,
                    handle: s.handle,
                }
            })
            .collect();
        let t_convert = ms(start);

        let start = Instant::now();
        let spans_json = serde_json::to_string(&spans).unwrap_or_default();
        let lines_json = serde_json::to_string(
            &projection
                .lines
                .iter()
                .map(|l| l.containers.iter().map(|c| c.handle).collect::<Vec<_>>())
                .collect::<Vec<_>>(),
        )
        .unwrap_or_default();
        let t_json = ms(start);

        let start = Instant::now();
        let contexts = db.line_contexts(id).expect("contexts");
        let contexts_json = serde_json::to_string(&*contexts).unwrap_or_default();
        let t_ctx_json = ms(start);

        println!(
            "round {round}: projection={t_proj:.1} convert={t_convert:.1} spans_json={t_json:.1} ({}+{}KB) contexts_query+json={t_ctx_json:.1} ({}KB) spans={} lines={}",
            spans_json.len() / 1024,
            lines_json.len() / 1024,
            contexts_json.len() / 1024,
            spans.len(),
            projection.lines.len(),
        );
    }
}
