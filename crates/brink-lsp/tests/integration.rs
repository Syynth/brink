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
#[ignore = "flaky — intermittent failures in CI and local runs"]
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

    // Knot folds (0-indexed lines, trimmed to exclude trailing whitespace).
    // collapsed_text is None — the editor already shows the header line.
    let has_knot_fold = |start: u64, end: u64| -> bool {
        folds
            .iter()
            .any(|(s, e, t)| *s == start && *e == end && t.is_none())
    };

    assert!(has_knot_fold(8, 20), "missing _start_rolling knot fold");
    assert!(has_knot_fold(22, 48), "missing _keep_rolling knot fold");
    assert!(has_knot_fold(49, 56), "missing player_roll knot fold");
    assert!(has_knot_fold(69, 91), "missing opposite_roll knot fold");

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

#[test]
fn code_actions_sort_knots() {
    let bin = env!("CARGO_BIN_EXE_brink-lsp");

    let mut child = Command::new(bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
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

    // --- didOpen with unsorted knots ---
    let ink_source = "\
=== charlie ===
Charlie content.

=== alpha ===
Alpha content.

=== bravo ===
Bravo content.
";

    let file_uri = "file:///tmp/sort_test.ink";

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

    // --- codeAction (id=2) ---
    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "textDocument/codeAction",
            "params": {
                "textDocument": { "uri": file_uri },
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 0, "character": 0 }
                },
                "context": {
                    "diagnostics": []
                }
            }
        }),
    );

    let (resp, _) = recv_response(&mut stdout, 2);
    eprintln!(
        "code_action response: {}",
        serde_json::to_string_pretty(&resp).unwrap()
    );

    let actions = resp["result"]
        .as_array()
        .expect("expected array of code actions");

    assert!(!actions.is_empty(), "expected at least one code action");

    assert_eq!(
        actions[0]["title"].as_str(),
        Some("Sort knots alphabetically"),
    );

    drop(stdin);
    drop(stdout);
    let _ = child.wait();
}

/// Run an LSP session with the given `initializationOptions.dialect` (or
/// `None` to omit it, exercising the `StrictInk` default), open `source`,
/// and return the diagnostics from the *last background-analysis*
/// `publishDiagnostics` notification observed for that file's URI.
///
/// Background analysis (`analysis_loop`, #599) runs on a separate tokio task
/// woken by a `Notify`, so its diagnostics can arrive after a same-id
/// round-trip response — unlike per-file parse diagnostics, which publish
/// synchronously inside `did_open`. Rather than polling for a fixed number
/// of rounds (#695 — that flaked under load: a slow/contended machine could
/// blow through the deadline before the background pass ever completed),
/// this waits for the server's own `$/brink/backgroundAnalysisComplete`
/// completion signal, fired unconditionally at the end of every pass — the
/// wait condition is "the background analysis actually finished a pass that
/// includes our file", not "some wall-clock budget elapsed". A generous
/// message-count hard cap remains as a backstop against a genuine hang/
/// regression (never firing, or never including the file), matching the
/// project's "guard against unbounded growth" rule for polling loops.
///
/// Only *version-less* publishes count toward the returned diagnostics
/// (#615). The `did_open` handler's own per-file publish (parse + lowering
/// only — no analyzer diagnostics) runs on the notification-handler task,
/// concurrently with the background loop. When the `initialized`-triggered
/// pass snapshots the db after `didOpen`'s content update, it reports
/// `file_count == 1` and publishes the full analysis set — but under CI
/// load, the delayed per-file publish could land on the wire *between*
/// that pass's publish and its completion signal, so "last publish before
/// the completion" observed the parse-only set and analyzer-diagnostic
/// assertions flaked. The server tags per-file publishes with the client
/// document version (`PublishDiagnosticsParams.version`); background
/// publishes carry none (they analyze a db snapshot, not a specific
/// client document version), so filtering to version-less publishes is
/// order-insensitive: whichever way the tasks interleave, the last
/// background publish before the first file-covering completion is that
/// pass's analysis set (or absent, meaning the set is empty — the
/// background loop suppresses never-published empty sets).
fn diagnostics_after_background_analysis(dialect: Option<&str>, source: &str) -> Vec<Value> {
    diagnostics_after_background_analysis_with_types(dialect, None, source)
}

/// Same as [`diagnostics_after_background_analysis`], but also sets
/// `initializationOptions.types` (`"strict"`/`"gradual"`, or `None` to omit
/// it, exercising the `Gradual` default) — #660's LSP-side counterpart of
/// the `dialect` option.
fn diagnostics_after_background_analysis_with_types(
    dialect: Option<&str>,
    types: Option<&str>,
    source: &str,
) -> Vec<Value> {
    // Condition-driven wait (#695): the settling condition is the server's
    // own `$/brink/backgroundAnalysisComplete` signal reporting a pass whose
    // db snapshot already includes our (single, just-opened) file — not a
    // fixed number of rounds or a sleep. `MAX_MESSAGES` is a generous hard
    // cap purely as a backstop against a genuine hang/regression (the signal
    // never arriving), matching the project's "guard against unbounded
    // growth" rule; it is not the intended exit path.
    const MAX_MESSAGES: u64 = 2000;

    let bin = env!("CARGO_BIN_EXE_brink-lsp");

    let mut child = Command::new(bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to start brink-lsp");

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    let mut init_params = json!({
        "capabilities": {},
        "rootUri": null,
    });
    if dialect.is_some() || types.is_some() {
        let mut opts = serde_json::Map::new();
        if let Some(d) = dialect {
            opts.insert("dialect".to_string(), json!(d));
        }
        if let Some(t) = types {
            opts.insert("types".to_string(), json!(t));
        }
        init_params["initializationOptions"] = Value::Object(opts);
    }

    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": init_params,
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

    let file_uri = "file:///tmp/dialect_test_story.ink";
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
                    "text": source,
                }
            }
        }),
    );

    let mut last_diags_for_uri: Vec<Value> = Vec::new();
    let mut settled = false;
    for _ in 0..MAX_MESSAGES {
        let msg = recv(&mut stdout);
        if msg["method"] == "textDocument/publishDiagnostics"
            && msg["params"]["uri"] == file_uri
            // Versioned publishes are `did_open`'s per-file (parse/lowering
            // only) set, which can interleave anywhere relative to the
            // background pass — only version-less background-analysis
            // publishes are the subject here (#615, see helper docs).
            && msg["params"]["version"].is_null()
        {
            last_diags_for_uri = msg["params"]["diagnostics"]
                .as_array()
                .cloned()
                .unwrap_or_default();
        } else if msg["method"] == "$/brink/backgroundAnalysisComplete" {
            // A pass triggered by server startup (`initialized`) can race
            // ahead of our `didOpen` and report `file_count == 0` — keep
            // waiting until a pass actually reflects the file we opened.
            let file_count = msg["params"]["file_count"].as_u64().unwrap_or(0);
            if file_count >= 1 {
                settled = true;
                break;
            }
        }
    }

    assert!(
        settled,
        "background analysis never signaled completion for {file_uri} \
         within {MAX_MESSAGES} messages"
    );

    drop(stdin);
    drop(stdout);
    let _ = child.wait();

    last_diags_for_uri
}

/// #599: the background `analysis_loop` must analyze under the
/// client-declared dialect, not always default to `StrictInk`. A
/// brink-dialect session must NOT see `E051` for valid brink-extension
/// syntax (postfix indexing) — before the fix, `analysis_loop` called bare
/// `brink_analyzer::analyze` with no `AnalysisOptions`, so this always
/// spuriously fired regardless of the declared dialect.
#[test]
fn background_analysis_uses_declared_brink_dialect_no_e051() {
    let source = "\
VAR a = 0
VAR x = 0
== start ==
~ x = a[0]
Hello.
-> DONE
";
    let diags = diagnostics_after_background_analysis(Some("brink"), source);
    let e051: Vec<&Value> = diags
        .iter()
        .filter(|d| d["code"].as_str() == Some("E051"))
        .collect();
    assert!(
        e051.is_empty(),
        "brink-dialect session should not see E051 for extension syntax, got: {diags:?}"
    );
}

/// #599 counterpart: a strict-ink session (explicit `"strict-ink"`, and
/// separately the default with no `dialect` declared at all) must still
/// flag the same construct as `E051` — the fix must not blanket-disable the
/// gate, only thread through the client's actual choice.
#[test]
fn background_analysis_uses_declared_strict_ink_dialect_still_flags_e051() {
    let source = "\
VAR a = 0
VAR x = 0
== start ==
~ x = a[0]
Hello.
-> DONE
";
    for dialect in [Some("strict-ink"), None] {
        let diags = diagnostics_after_background_analysis(dialect, source);
        let e051: Vec<&Value> = diags
            .iter()
            .filter(|d| d["code"].as_str() == Some("E051"))
            .collect();
        assert!(
            !e051.is_empty(),
            "strict-ink session (dialect={dialect:?}) should still flag E051 \
             for extension syntax, got: {diags:?}"
        );
    }
}

/// #660: the background `analysis_loop` must analyze under the
/// client-declared TM-3 typed-mode policy, not always default to `Gradual`.
/// `types = strict` under the default `strict-ink` dialect is a
/// project-level config error (`E064`) — the LSP surface must reach this the
/// same way the compiler CLI's `--types strict` does. Before #660,
/// `initialize` had no `types` handler at all, so this option was silently
/// ignored.
#[test]
fn background_analysis_uses_declared_strict_types_flags_e064_without_brink_dialect() {
    let source = "-> END\n";
    let diags = diagnostics_after_background_analysis_with_types(None, Some("strict"), source);
    let e064: Vec<&Value> = diags
        .iter()
        .filter(|d| d["code"].as_str() == Some("E064"))
        .collect();
    assert!(
        !e064.is_empty(),
        "types=strict + dialect=strict-ink (default): expected E064, got: {diags:?}"
    );
}

/// #660 counterpart: `types = strict` + `dialect = brink` turns on the
/// Unknown-escape check (`E065`) — proving the LSP plumbing reaches the real
/// strict-mode checks, not just the config-error path.
#[test]
fn background_analysis_uses_declared_strict_types_with_brink_dialect_flags_e065() {
    let source = "=== noop(x) ===\nHello.\n-> DONE\n";
    let diags =
        diagnostics_after_background_analysis_with_types(Some("brink"), Some("strict"), source);
    let e065: Vec<&Value> = diags
        .iter()
        .filter(|d| d["code"].as_str() == Some("E065"))
        .collect();
    assert!(
        !e065.is_empty(),
        "types=strict + dialect=brink: expected E065 on unused param `x`, got: {diags:?}"
    );
}

/// #660: the default `types` (no `initializationOptions.types` at all, i.e.
/// `Gradual`) must NOT flag the same unused-param construct as `E065` — the
/// handler must not blanket-enable strict checks, only thread through the
/// client's actual choice.
#[test]
fn background_analysis_default_types_does_not_flag_e065() {
    let source = "=== noop(x) ===\nHello.\n-> DONE\n";
    let diags = diagnostics_after_background_analysis_with_types(Some("brink"), None, source);
    let e065: Vec<&Value> = diags
        .iter()
        .filter(|d| d["code"].as_str() == Some("E065"))
        .collect();
    assert!(
        e065.is_empty(),
        "default types (gradual) must not flag E065: {diags:?}"
    );
}
