use std::path::Path;

use brink_syntax::parse;

fn collect_ink_files(dir: &Path, files: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.is_dir() {
            collect_ink_files(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "ink") {
            files.push(path);
        }
    }
}

#[test]
fn corpus_roundtrip() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let corpus_dir = manifest_dir.join("../../..").join("tests");
    let corpus_dir = corpus_dir
        .canonicalize()
        .expect("corpus directory not found");

    let mut files = Vec::new();
    collect_ink_files(&corpus_dir, &mut files);
    files.sort();

    assert!(
        !files.is_empty(),
        "no .ink files found in {}",
        corpus_dir.display()
    );

    let mut total = 0;
    let mut with_errors = 0;
    let mut total_errors = 0;
    let mut failures = Vec::new();

    for path in &files {
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("SKIP (read error): {}: {e}", path.display());
                continue;
            }
        };

        total += 1;

        let parsed = parse(&source);
        let roundtrip = parsed.syntax().text().to_string();

        if roundtrip != source {
            let rel = path.strip_prefix(&corpus_dir).unwrap_or(path);
            failures.push(format!(
                "  {}: source len={}, roundtrip len={}",
                rel.display(),
                source.len(),
                roundtrip.len(),
            ));
        }

        let errors = parsed.errors();
        if !errors.is_empty() {
            with_errors += 1;
            total_errors += errors.len();
        }
    }

    eprintln!();
    eprintln!("=== Corpus round-trip summary ===");
    eprintln!("  Files parsed:      {total}");
    eprintln!("  Files with errors: {with_errors}");
    eprintln!("  Total parse errors: {total_errors}");
    eprintln!("  Round-trip failures: {}", failures.len());
    eprintln!();

    assert!(
        failures.is_empty(),
        "lossless round-trip violated for {} file(s):\n{}",
        failures.len(),
        failures.join("\n"),
    );
}
