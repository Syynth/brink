#![expect(clippy::unwrap_used, clippy::expect_used)]

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

/// Send a JSON-RPC message with the Content-Length header over a writer.
fn send(w: &mut impl Write, msg: &Value) {
    let body = serde_json::to_string(msg).unwrap();
    write!(w, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
    w.flush().unwrap();
}

/// Read one JSON-RPC message from the LSP stdout stream.
fn recv(reader: &mut BufReader<impl std::io::Read>) -> Value {
    // Read headers until blank line
    let mut content_length: Option<usize> = None;
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).unwrap();
        let header = header.trim();
        if header.is_empty() {
            break;
        }
        if let Some(len) = header.strip_prefix("Content-Length: ") {
            content_length = Some(len.parse().unwrap());
        }
    }

    let len = content_length.expect("missing Content-Length header");
    let mut body = vec![0u8; len];
    std::io::Read::read_exact(reader, &mut body).unwrap();
    serde_json::from_slice(&body).unwrap()
}

/// Read responses until we find one matching the given request id.
/// Notifications and other responses are collected and returned alongside.
fn recv_response(reader: &mut BufReader<impl std::io::Read>, id: u64) -> (Value, Vec<Value>) {
    let mut others = Vec::new();
    loop {
        let msg = recv(reader);
        if msg.get("id").and_then(Value::as_u64) == Some(id) {
            return (msg, others);
        }
        others.push(msg);
    }
}

#[test]
#[expect(clippy::too_many_lines)]
fn document_symbols_for_ink_file() {
    let bin = env!("CARGO_BIN_EXE_brink-lsp");

    let mut child = Command::new(bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to start brink-lsp");

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    // --- initialize (id=1) ---
    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "capabilities": {},
                "rootUri": null,
            }
        }),
    );

    let (init_resp, _) = recv_response(&mut stdout, 1);
    let caps = &init_resp["result"]["capabilities"];
    assert!(
        caps["textDocumentSync"].is_object(),
        "expected sync capabilities"
    );
    assert_eq!(init_resp["result"]["serverInfo"]["name"], "brink-lsp",);

    // --- initialized (notification, no id) ---
    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {}
        }),
    );

    // --- didOpen with an ink file containing knots and stitches ---
    let ink_source = "\
VAR knotCount = 0
-> knot_count_test ->
-> DONE
== knot_count_test ==
~ knotCount++
{knotCount}
{knotCount<3:->knot_count_test}
->->
== another_knot ==
= my_stitch
Some text.
->->
";

    let file_uri = "file:///tmp/test_story.ink";

    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": file_uri,
                    "languageId": "ink",
                    "version": 1,
                    "text": ink_source,
                }
            }
        }),
    );

    // --- documentSymbol (id=2) ---
    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "textDocument/documentSymbol",
            "params": {
                "textDocument": { "uri": file_uri }
            }
        }),
    );

    let (symbols_resp, _notifications) = recv_response(&mut stdout, 2);
    let symbols = symbols_resp["result"]
        .as_array()
        .expect("expected array of document symbols");

    // We should have at least the two knots and the VAR declaration
    let names: Vec<&str> = symbols.iter().filter_map(|s| s["name"].as_str()).collect();

    assert!(
        names.contains(&"knot_count_test"),
        "expected knot_count_test in symbols, got: {names:?}",
    );
    assert!(
        names.contains(&"another_knot"),
        "expected another_knot in symbols, got: {names:?}",
    );

    // another_knot should have my_stitch as a child
    let another = symbols
        .iter()
        .find(|s| s["name"].as_str() == Some("another_knot"))
        .expect("another_knot not found");
    let children = another["children"]
        .as_array()
        .expect("expected children on another_knot");
    let child_names: Vec<&str> = children.iter().filter_map(|c| c["name"].as_str()).collect();
    assert!(
        child_names.contains(&"my_stitch"),
        "expected my_stitch as child of another_knot, got: {child_names:?}",
    );

    // Drop stdin to signal the server to shut down.
    drop(stdin);
    drop(stdout);
    let _ = child.wait();
}

#[test]
fn diagnostics_for_scene1_ink() {
    let bin = env!("CARGO_BIN_EXE_brink-lsp");

    let mut child = std::process::Command::new(bin)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("failed to start brink-lsp");

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    // --- initialize ---
    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "capabilities": {},
                "rootUri": null,
            }
        }),
    );
    let (_init_resp, _) = recv_response(&mut stdout, 1);

    // --- initialized ---
    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {}
        }),
    );

    // --- didOpen with the scene1.ink file ---
    let ink_source =
        include_str!("../../../tests/tests_patched/wildwinter__Ink-Explorer/tests/dink/scene1.ink");
    let file_uri = "file:///tmp/scene1.ink";

    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": file_uri,
                    "languageId": "ink",
                    "version": 1,
                    "text": ink_source,
                }
            }
        }),
    );

    // Send a dummy request so we can collect notifications that arrived
    // between didOpen and this response.
    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "textDocument/documentSymbol",
            "params": {
                "textDocument": { "uri": file_uri }
            }
        }),
    );

    let (_symbols_resp, notifications) = recv_response(&mut stdout, 2);

    // Find publishDiagnostics notifications
    let diag_notifications: Vec<&Value> = notifications
        .iter()
        .filter(|n| n["method"] == "textDocument/publishDiagnostics")
        .collect();

    // Print diagnostics for inspection
    for note in &diag_notifications {
        let diags = note["params"]["diagnostics"]
            .as_array()
            .expect("diagnostics should be array");
        eprintln!(
            "=== publishDiagnostics for {} ({} diagnostics) ===",
            note["params"]["uri"],
            diags.len()
        );
        for d in diags {
            let range = &d["range"];
            let start = &range["start"];
            let end = &range["end"];
            eprintln!(
                "  [{severity}] {line}:{col}-{eline}:{ecol}: {msg}",
                severity = d["severity"],
                line = start["line"],
                col = start["character"],
                eline = end["line"],
                ecol = end["character"],
                msg = d["message"],
            );
        }
    }

    // Assert we got at least one publishDiagnostics notification
    assert!(
        !diag_notifications.is_empty(),
        "expected at least one publishDiagnostics notification"
    );

    // For now, just report what we got. We can tighten assertions later.
    let all_diags: Vec<&Value> = diag_notifications
        .iter()
        .flat_map(|n| n["params"]["diagnostics"].as_array().unwrap().iter())
        .collect();
    eprintln!("\nTotal diagnostics: {}", all_diags.len());

    drop(stdin);
    drop(stdout);
    let _ = child.wait();
}

#[test]
#[expect(clippy::too_many_lines)]
fn folding_ranges_for_dice_rolling_functions() {
    let bin = env!("CARGO_BIN_EXE_brink-lsp");

    let mut child = Command::new(bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to start brink-lsp");

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    // --- initialize ---
    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "capabilities": {},
                "rootUri": null,
            }
        }),
    );
    let (_init_resp, _) = recv_response(&mut stdout, 1);

    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {}
        }),
    );

    // --- didOpen with dice_rolling.ink ---
    let ink_source =
        include_str!("../../../tests/tests_patched/alobacheva__Tsiolkov-Sky/dice_rolling.ink");
    let file_uri = "file:///tmp/dice_rolling.ink";

    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": file_uri,
                    "languageId": "ink",
                    "version": 1,
                    "text": ink_source,
                }
            }
        }),
    );

    // --- foldingRange (id=2) ---
    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "textDocument/foldingRange",
            "params": {
                "textDocument": { "uri": file_uri }
            }
        }),
    );

    let (fold_resp, _) = recv_response(&mut stdout, 2);
    let ranges = fold_resp["result"]
        .as_array()
        .expect("expected array of folding ranges");

    // dice_rolling.ink has 6 function knots:
    //   _start_rolling, _keep_rolling, player_roll,
    //   ccplayer_roll, opposite_roll, ccopposite_roll
    // Collect (start_line, end_line, collapsed_text) tuples for easy assertion
    let folds: Vec<(u64, u64, Option<&str>)> = ranges
        .iter()
        .map(|r| {
            (
                r["startLine"].as_u64().unwrap(),
                r["endLine"].as_u64().unwrap(),
                r["collapsedText"].as_str(),
            )
        })
        .collect();

    eprintln!("folding ranges ({}):", folds.len());
    for (s, e, t) in &folds {
        eprintln!("  lines {s}-{e}: {t:?}");
    }

    // Helper: check that a fold with the given collapsed text exists covering the expected lines
    let has_fold = |start: u64, end: u64, text: &str| -> bool {
        folds
            .iter()
            .any(|(s, e, t)| *s == start && *e == end && *t == Some(text))
    };

    // Knot folds (0-indexed lines)
    assert!(
        has_fold(8, 22, "== _start_rolling =="),
        "missing _start_rolling knot fold"
    );
    assert!(
        has_fold(22, 49, "== _keep_rolling =="),
        "missing _keep_rolling knot fold"
    );
    assert!(
        has_fold(49, 58, "== player_roll =="),
        "missing player_roll knot fold"
    );
    assert!(
        has_fold(69, 93, "== opposite_roll =="),
        "missing opposite_roll knot fold"
    );

    // Conditionals inside _start_rolling (lines 10-12, 13-15, 16-18)
    assert!(
        has_fold(10, 12, "{...}"),
        "missing conditional fold at lines 10-12"
    );
    assert!(
        has_fold(13, 15, "{...}"),
        "missing conditional fold at lines 13-15"
    );
    assert!(
        has_fold(16, 18, "{...}"),
        "missing conditional fold at lines 16-18"
    );

    // _keep_rolling: outer conditional (lines 23-39)
    assert!(
        has_fold(23, 39, "{...}"),
        "missing outer conditional in _keep_rolling"
    );
    // TODO: nested conditional at lines 26-35 is not emitted because the outer
    assert!(
        has_fold(26, 35, "{...}"),
        "missing nested conditional in _keep_rolling"
    );

    // player_roll: conditional (lines 52-56)
    assert!(
        has_fold(52, 56, "{...}"),
        "missing conditional in player_roll"
    );

    // opposite_roll: conditionals (lines 82-85, 87-91)
    assert!(
        has_fold(82, 85, "{...}"),
        "missing conditional at lines 82-85"
    );
    assert!(
        has_fold(87, 91, "{...}"),
        "missing conditional at lines 87-91"
    );

    // Every range should span multiple lines
    for (s, e, _) in &folds {
        assert!(e > s, "folding range should span multiple lines: {s}-{e}");
    }

    drop(stdin);
    drop(stdout);
    let _ = child.wait();
}
