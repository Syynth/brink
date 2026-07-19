#![expect(clippy::unwrap_used, clippy::expect_used)]

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

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
    // Backstop cap on the wait loop below — see that loop's comment.
    const MAX_MESSAGES: u64 = 2000;

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

    // Wait for a background pass covering the file to complete. The settling
    // condition is the `$/brink/backgroundAnalysisComplete` signal (#695) —
    // NOT "at least one publishDiagnostics arrived". scene1.ink is a *clean*
    // file, and the server suppresses an empty first publish for a
    // never-published file (`DiagnosticsPublisher`, #615, matching the
    // background loop's long-standing behavior), so a correct run legitimately
    // emits ZERO diagnostics for it. Requiring a publish is exactly what hung
    // this test once empty publishes were suppressed. Collect any diagnostics
    // publishes that do arrive, for reporting. The message cap is only a
    // backstop against a genuine hang/regression.
    let mut diag_notifications: Vec<Value> = Vec::new();
    let mut analysis_done = false;
    for _ in 0..MAX_MESSAGES {
        let msg = recv(&mut stdout);
        if msg["method"] == "textDocument/publishDiagnostics" && msg["params"]["uri"] == file_uri {
            diag_notifications.push(msg);
        } else if msg["method"] == "$/brink/backgroundAnalysisComplete"
            && msg["params"]["file_count"].as_u64().unwrap_or(0) >= 1
        {
            analysis_done = true;
            break;
        }
    }
    assert!(
        analysis_done,
        "background analysis never signaled completion for {file_uri} \
         within {MAX_MESSAGES} messages"
    );

    // Report whatever diagnostics were published (may be none — see above).
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
    diagnostics_after_background_analysis_full(None, dialect, types, source)
}

/// Same as [`diagnostics_after_background_analysis_with_types`], but also
/// lets the caller set the session's workspace root (`rootUri`) to
/// `root_dir` — so a `brink.toml` placed there (#1005) is discovered and
/// reconciled with `initializationOptions.dialect`/`.types` per the #1030
/// precedence rule (see `resolve_language_options` in `backend.rs`: the
/// file supplies the default, an explicit client option always wins).
/// `None` keeps `rootUri: null` (no workspace, no discovery), matching
/// every other test in this file.
fn diagnostics_after_background_analysis_full(
    root_dir: Option<&std::path::Path>,
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

    let root_uri = root_dir.map(|d| format!("file://{}", d.display()));
    let mut init_params = json!({
        "capabilities": {},
        "rootUri": root_uri,
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

/// #660 premise, rewritten for NS-A9 (#1127): the default `types` is now
/// **dialect-keyed** — a brink-dialect project with no
/// `initializationOptions.types` resolves strict (E065 fires on the escaping
/// param), while the strict-ink default stays gradual forever (no E065). The
/// handler still threads only the client's actual choice; what changed is
/// what "no choice" means per dialect.
#[test]
fn background_analysis_default_types_is_dialect_keyed() {
    let source = "=== noop(x) ===\nHello.\n-> DONE\n";
    let diags = diagnostics_after_background_analysis_with_types(Some("brink"), None, source);
    let e065: Vec<&Value> = diags
        .iter()
        .filter(|d| d["code"].as_str() == Some("E065"))
        .collect();
    assert!(
        !e065.is_empty(),
        "brink dialect + unset types now defaults strict: expected E065, got: {diags:?}"
    );

    let diags = diagnostics_after_background_analysis_with_types(None, None, source);
    let e065: Vec<&Value> = diags
        .iter()
        .filter(|d| d["code"].as_str() == Some("E065"))
        .collect();
    assert!(
        e065.is_empty(),
        "strict-ink dialect + unset types stays gradual: must not flag E065: {diags:?}"
    );
}

/// NS-A9 companion: an explicit `types: "gradual"` opt-out on a brink-dialect
/// project suppresses the strict default's E065.
#[test]
fn background_analysis_explicit_gradual_opts_out_of_strict_default() {
    let source = "=== noop(x) ===\nHello.\n-> DONE\n";
    let diags =
        diagnostics_after_background_analysis_with_types(Some("brink"), Some("gradual"), source);
    let e065: Vec<&Value> = diags
        .iter()
        .filter(|d| d["code"].as_str() == Some("E065"))
        .collect();
    assert!(
        e065.is_empty(),
        "explicit types=gradual must opt out of the strict default: {diags:?}"
    );
}

// ── #1030: reconcile initializationOptions.dialect/.types with brink.toml ──
//
// The four scenarios below mirror `resolve_language_options`'s precedence
// contract (`AnalysisOptions::default()` < discovered `brink.toml` (#1005)
// < an explicit `initializationOptions` value): file-only, option-only,
// both (option must win), neither (defaults). `apply_to_options` itself is
// already exhaustively unit-tested in `brink-project-config`; these
// exercise the LSP-specific wiring — workspace-root discovery,
// `initializationOptions` reading, and writing the resolved value into
// `LanguageOptions` — using the same `E051` postfix-indexing signal
// (`background_analysis_uses_declared_*_dialect_*` above) to observe
// `Dialect::Brink` vs. `Dialect::StrictInk`.

/// A unique per-test scratch directory under the OS temp dir, cleaned up by
/// the caller. Mirrors `brink-project-config`'s own test helper of the same
/// name/shape (`crates/internal/brink-project-config/src/lib.rs`) — each
/// test gets an isolated directory so parallel test runs never collide.
fn unique_tmp_dir(tag: &str) -> std::path::PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "brink-lsp-test-{tag}-{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    dir
}

/// Postfix array indexing (`a[0]`) — a `brink`-dialect extension syntax
/// construct that `strict-ink` flags as `E051`. Shared by the four
/// scenarios below, same source `background_analysis_uses_declared_brink_dialect_no_e051`
/// (above) already uses to observe the dialect the session actually
/// resolved.
const DIALECT_PROBE_SOURCE: &str = "\
VAR a = 0
VAR x = 0
== start ==
~ x = a[0]
Hello.
-> DONE
";

fn has_e051(diags: &[Value]) -> bool {
    diags.iter().any(|d| d["code"].as_str() == Some("E051"))
}

/// **File-only**: `brink.toml` sets `dialect = "brink"`, no
/// `initializationOptions.dialect` at all. The file must supply the
/// default — no `E051`.
#[test]
fn brink_toml_dialect_file_only_no_option() {
    let root = unique_tmp_dir("dialect-file-only");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("brink.toml"), "[project]\ndialect = \"brink\"\n").unwrap();

    let diags =
        diagnostics_after_background_analysis_full(Some(&root), None, None, DIALECT_PROBE_SOURCE);

    std::fs::remove_dir_all(&root).unwrap();

    assert!(
        !has_e051(&diags),
        "brink.toml dialect=brink (file-only) should not flag E051, got: {diags:?}"
    );
}

/// **Option-only**: a workspace root is configured (so discovery runs) but
/// has no `brink.toml`; `initializationOptions.dialect = "brink"` is the
/// only source. The option must still apply exactly as it did before
/// #1030 — no `E051`.
#[test]
fn brink_toml_dialect_option_only_no_file() {
    let root = unique_tmp_dir("dialect-option-only");
    std::fs::create_dir_all(&root).unwrap();

    let diags = diagnostics_after_background_analysis_full(
        Some(&root),
        Some("brink"),
        None,
        DIALECT_PROBE_SOURCE,
    );

    std::fs::remove_dir_all(&root).unwrap();

    assert!(
        !has_e051(&diags),
        "initializationOptions.dialect=brink (option-only, no brink.toml) \
         should not flag E051, got: {diags:?}"
    );
}

/// **Both, option wins**: `brink.toml` sets `dialect = "strict-ink"`
/// (which alone would flag `E051`), but `initializationOptions.dialect =
/// "brink"` is also set. The explicit client option must override the
/// file — no `E051`.
#[test]
fn brink_toml_dialect_both_option_wins() {
    let root = unique_tmp_dir("dialect-both");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("brink.toml"),
        "[project]\ndialect = \"strict-ink\"\n",
    )
    .unwrap();

    let diags = diagnostics_after_background_analysis_full(
        Some(&root),
        Some("brink"),
        None,
        DIALECT_PROBE_SOURCE,
    );

    std::fs::remove_dir_all(&root).unwrap();

    assert!(
        !has_e051(&diags),
        "initializationOptions.dialect=brink must win over brink.toml's \
         dialect=strict-ink, got: {diags:?}"
    );
}

/// **Neither**: a workspace root is configured (discovery runs) but has no
/// `brink.toml`, and no `initializationOptions.dialect` is set either. Must
/// fall back to `AnalysisOptions::default()` (`StrictInk`) — `E051` still
/// fires, exactly as pre-#1030 (and pre-#1005) behavior.
#[test]
fn brink_toml_dialect_neither_file_nor_option_defaults() {
    let root = unique_tmp_dir("dialect-neither");
    std::fs::create_dir_all(&root).unwrap();

    let diags =
        diagnostics_after_background_analysis_full(Some(&root), None, None, DIALECT_PROBE_SOURCE);

    std::fs::remove_dir_all(&root).unwrap();

    assert!(
        has_e051(&diags),
        "no brink.toml and no initializationOptions.dialect should default \
         to StrictInk and still flag E051, got: {diags:?}"
    );
}

// ── #1055: malformed brink.toml diagnostic + live reload ───────────────
//
// Two related gaps closed together (see `backend.rs`): (1) a malformed
// `brink.toml` must surface a client-visible `textDocument/publishDiagnostics`
// on the file itself, not just a server-side `tracing::warn!`
// (`config_error_diagnostic`, `resolve_language_options`); (2) the config is
// re-read on `workspace/didChangeConfiguration` and on a file-watched change
// to `brink.toml` itself, not only once at `initialize`
// (`Backend::reload_brink_toml`), so edits apply without a client restart.

/// #1055 gap 1: a `brink.toml` with malformed TOML syntax surfaces a
/// `textDocument/publishDiagnostics` on the file itself during startup — a
/// client-visible signal that the session silently fell back to defaults,
/// where before only a server-side `tracing::warn!` fired (invisible to any
/// real client).
#[test]
fn brink_toml_malformed_syntax_surfaces_diagnostic() {
    // Backstop cap on the wait loop below — see that loop's comment.
    const MAX_MESSAGES: u64 = 2000;

    let root = unique_tmp_dir("malformed-toml");
    std::fs::create_dir_all(&root).unwrap();
    let toml_path = root.join("brink.toml");
    std::fs::write(&toml_path, "this is not [ valid toml").unwrap();

    let bin = env!("CARGO_BIN_EXE_brink-lsp");
    let mut child = Command::new(bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to start brink-lsp");

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "capabilities": {},
                "rootUri": format!("file://{}", root.display()),
            },
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

    // Condition-driven wait (#695 convention, see the helpers above):
    // `$/brink/backgroundAnalysisComplete` is sent unconditionally at the
    // end of the startup analysis pass, strictly after `initialized()`'s
    // brink.toml diagnostic publish — seeing it means that publish, if any,
    // has already gone out.
    let toml_uri = format!("file://{}", toml_path.display());
    let mut toml_diags: Option<Vec<Value>> = None;
    for _ in 0..MAX_MESSAGES {
        let msg = recv(&mut stdout);
        if msg["method"] == "textDocument/publishDiagnostics" && msg["params"]["uri"] == toml_uri {
            toml_diags = msg["params"]["diagnostics"].as_array().cloned();
        } else if msg["method"] == "$/brink/backgroundAnalysisComplete" {
            break;
        }
    }

    drop(stdin);
    drop(stdout);
    let _ = child.wait();
    std::fs::remove_dir_all(&root).unwrap();

    let diags = toml_diags.expect(
        "expected a textDocument/publishDiagnostics notification for the malformed brink.toml",
    );
    assert!(
        !diags.is_empty(),
        "malformed brink.toml should surface at least one diagnostic, got an empty set"
    );
    assert_eq!(diags[0]["source"].as_str(), Some("brink.toml"));
    assert_eq!(
        diags[0]["severity"].as_u64(),
        Some(1),
        "expected ERROR severity, got: {:?}",
        diags[0]
    );
}

/// Spawn a session rooted at `root`, initialize + open a `.ink` file with
/// [`DIALECT_PROBE_SOURCE`], and wait for the first background-analysis
/// pass to settle. Returns the still-connected child/stdin/stdout — the
/// caller drives further notifications on the same session — the opened
/// file's URI, and the diagnostics observed for it after that first pass.
fn start_dialect_probe_session(
    root: &std::path::Path,
) -> (
    Child,
    ChildStdin,
    BufReader<ChildStdout>,
    String,
    Vec<Value>,
) {
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

    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "capabilities": {},
                "rootUri": format!("file://{}", root.display()),
            },
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

    let ink_uri = format!("file://{}", root.join("story.ink").display());
    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": ink_uri,
                    "languageId": "ink",
                    "version": 1,
                    "text": DIALECT_PROBE_SOURCE,
                }
            }
        }),
    );

    let diags = wait_for_next_analysis_pass(&mut stdout, &ink_uri, MAX_MESSAGES);
    (child, stdin, stdout, ink_uri, diags)
}

/// Read messages until a `$/brink/backgroundAnalysisComplete` reporting
/// `file_count >= 1` arrives, returning the last version-less
/// `textDocument/publishDiagnostics` set observed for `uri` along the way —
/// the background-analysis set, per the #695 convention every helper above
/// already relies on. Each call consumes only the messages up to and
/// including its own terminal signal, so sequential calls on the same
/// session observe successive passes without racing each other.
fn wait_for_next_analysis_pass(
    stdout: &mut BufReader<ChildStdout>,
    uri: &str,
    max_messages: u64,
) -> Vec<Value> {
    let mut last_diags: Vec<Value> = Vec::new();
    let mut settled = false;
    for _ in 0..max_messages {
        let msg = recv(stdout);
        if msg["method"] == "textDocument/publishDiagnostics"
            && msg["params"]["uri"] == uri
            && msg["params"]["version"].is_null()
        {
            last_diags = msg["params"]["diagnostics"]
                .as_array()
                .cloned()
                .unwrap_or_default();
        } else if msg["method"] == "$/brink/backgroundAnalysisComplete" {
            let file_count = msg["params"]["file_count"].as_u64().unwrap_or(0);
            if file_count >= 1 {
                settled = true;
                break;
            }
        }
    }
    assert!(
        settled,
        "background analysis never signaled completion for {uri} within {max_messages} messages"
    );
    last_diags
}

/// #1055 gap 2 (file-watch path): editing `brink.toml` on disk and sending
/// the `workspace/didChangeWatchedFiles` notification the server's own
/// `initialized()`-time watcher registration asks for (`**/brink.toml`, in
/// addition to `**/*.ink`) re-resolves the dialect and re-analyzes on the
/// very next pass — no client restart, no re-`initialize`.
#[test]
fn brink_toml_file_watch_reload_applies_without_restart() {
    let root = unique_tmp_dir("file-watch-reload");
    std::fs::create_dir_all(&root).unwrap();
    let toml_path = root.join("brink.toml");
    std::fs::write(&toml_path, "[project]\ndialect = \"strict-ink\"\n").unwrap();

    let (mut child, mut stdin, mut stdout, ink_uri, diags_before) =
        start_dialect_probe_session(&root);

    assert!(
        has_e051(&diags_before),
        "strict-ink (from brink.toml) should flag E051 before the reload, got: {diags_before:?}"
    );

    // Flip the file to `brink` dialect and send the file-watch notification
    // a real client sends after the server's registered watcher fires —
    // `type: 2` is `FileChangeType::Changed` per the LSP spec.
    std::fs::write(&toml_path, "[project]\ndialect = \"brink\"\n").unwrap();
    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "workspace/didChangeWatchedFiles",
            "params": {
                "changes": [{
                    "uri": format!("file://{}", toml_path.display()),
                    "type": 2,
                }]
            }
        }),
    );

    let diags_after = wait_for_next_analysis_pass(&mut stdout, &ink_uri, 2000);

    drop(stdin);
    drop(stdout);
    let _ = child.wait();
    std::fs::remove_dir_all(&root).unwrap();

    assert!(
        !has_e051(&diags_after),
        "brink dialect after a file-watched brink.toml reload should not flag E051, got: {diags_after:?}"
    );
}

/// #1055 gap 2 (`workspace/didChangeConfiguration` path): some clients send
/// this notification, without a separate file-watch event, when workspace
/// settings change — `brink.toml` must still be re-read and re-applied.
#[test]
fn brink_toml_did_change_configuration_reload_applies_without_restart() {
    let root = unique_tmp_dir("did-change-configuration-reload");
    std::fs::create_dir_all(&root).unwrap();
    let toml_path = root.join("brink.toml");
    std::fs::write(&toml_path, "[project]\ndialect = \"strict-ink\"\n").unwrap();

    let (mut child, mut stdin, mut stdout, ink_uri, diags_before) =
        start_dialect_probe_session(&root);

    assert!(
        has_e051(&diags_before),
        "strict-ink (from brink.toml) should flag E051 before the reload, got: {diags_before:?}"
    );

    std::fs::write(&toml_path, "[project]\ndialect = \"brink\"\n").unwrap();
    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "workspace/didChangeConfiguration",
            "params": { "settings": {} }
        }),
    );

    let diags_after = wait_for_next_analysis_pass(&mut stdout, &ink_uri, 2000);

    drop(stdin);
    drop(stdout);
    let _ = child.wait();
    std::fs::remove_dir_all(&root).unwrap();

    assert!(
        !has_e051(&diags_after),
        "brink dialect after a didChangeConfiguration reload should not flag E051, got: {diags_after:?}"
    );
}
