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

// ── #1367: `[lints]` re-leveling reaches the published diagnostic severity ──
//
// `resolve_language_options` already resolves a discovered `brink.toml`'s
// `[lints]` table via `AnalysisOptions::apply_project_config` (#1160); this
// proves that resolved policy actually reaches `LanguageOptions` and every
// diagnostic-publish site's `effective_severity` call, end to end through
// the real LSP process — not just the unit-level `convert::diagnostic_to_lsp`
// coverage in `src/convert.rs`.

/// A bare `~` logic line — no expression, no statement — lowers to `E014`
/// ("logic line has no effect"), `Warning` by default
/// (`brink-ir/src/hir/lower/tests.rs`'s `logic_line_emits_diagnostic_on_malformed`
/// exercises the same construct at the lowering-unit level).
const E014_PROBE_SOURCE: &str = "\
== start ==
~
Hello.
-> DONE
";

fn e014_severity(diags: &[Value]) -> Option<u64> {
    diags
        .iter()
        .find(|d| d["code"].as_str() == Some("E014"))
        .and_then(|d| d["severity"].as_u64())
}

/// Like [`diagnostics_after_background_analysis_full`], but `E014` is a pure
/// **lowering** diagnostic (`brink.toml`'s `[lints]` policy is already loaded
/// before `initialize()` returns, so `publish_perfile_diagnostics`'s very
/// first, *versioned* publish already carries the effective severity — the
/// background pass then computes an identical set and the anti-downgrade
/// rule correctly never re-sends it). Filtering to version-less publishes
/// (as the dialect/types helpers do, where the analysis-only signal they
/// probe genuinely only ever appears on the `Analysis`-tier publish) would
/// see nothing here. This captures the last publish for the file's URI
/// regardless of version — the client-visible end state — up to the same
/// settling signal.
fn diagnostics_for_uri_settled(root: &std::path::Path, source: &str) -> Vec<Value> {
    diagnostics_for_uri_settled_with_init_options(root, source, Value::Null)
}

/// Like [`diagnostics_for_uri_settled`], but also sets
/// `initializationOptions` to `init_options` (`Value::Null` to omit the key
/// entirely, byte-identical to `diagnostics_for_uri_settled`'s own
/// behavior) — issue #1417's LSP-side probe for the
/// `initializationOptions.lints`/`.denyWarnings` CLI/API lint-override
/// tier, alongside `diagnostics_for_uri_settled`'s existing `brink.toml`
/// `[lints]` coverage.
fn diagnostics_for_uri_settled_with_init_options(
    root: &std::path::Path,
    source: &str,
    init_options: Value,
) -> Vec<Value> {
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
        "rootUri": format!("file://{}", root.display()),
    });
    if !init_options.is_null() {
        init_params["initializationOptions"] = init_options;
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

    let file_uri = "file:///tmp/lints_test_story.ink";
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
        if msg["method"] == "textDocument/publishDiagnostics" && msg["params"]["uri"] == file_uri {
            last_diags_for_uri = msg["params"]["diagnostics"]
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
        "background analysis never signaled completion for {file_uri} \
         within {MAX_MESSAGES} messages"
    );

    drop(stdin);
    drop(stdout);
    let _ = child.wait();

    last_diags_for_uri
}

/// No `[lints]` table at all: `E014` publishes at its raw `Warning` default
/// (LSP severity `2`).
#[test]
fn brink_toml_lints_no_override_keeps_warning_default() {
    let root = unique_tmp_dir("lints-no-override");
    std::fs::create_dir_all(&root).unwrap();

    let diags = diagnostics_for_uri_settled(&root, E014_PROBE_SOURCE);

    std::fs::remove_dir_all(&root).unwrap();

    assert_eq!(
        e014_severity(&diags),
        Some(2),
        "no [lints] override: E014 should publish at its Warning default, got: {diags:?}"
    );
}

/// `[lints] E014 = "deny"` in `brink.toml`: the published diagnostic must be
/// `Error` (LSP severity `1`), not the raw `Warning` default — the exact
/// regression #1367 fixes (`diagnostic_to_lsp` previously called the raw
/// `diag.code.severity()`, never consulting `[lints]` at all).
#[test]
fn brink_toml_lints_override_promotes_published_severity_to_error() {
    let root = unique_tmp_dir("lints-deny-override");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("brink.toml"), "[lints]\nE014 = \"deny\"\n").unwrap();

    let diags = diagnostics_for_uri_settled(&root, E014_PROBE_SOURCE);

    std::fs::remove_dir_all(&root).unwrap();

    assert_eq!(
        e014_severity(&diags),
        Some(1),
        "[lints] E014 = deny should promote the published severity to Error, got: {diags:?}"
    );
}

// ── #1417: initializationOptions.lints/.denyWarnings CLI/API override tier ──
//
// Extends #1367's file-only `[lints]` resolution (above) with an explicit
// client-declared tier — the LSP's counterpart of `brink compile`'s
// `--deny`/`--warn`/`--allow`/`-D warnings` (#1373) and `BrinkPlugin::
// with_config` (#1394). `resolve_language_options` applies it last, so it
// always wins over a conflicting `brink.toml` entry for the same code.

/// `initializationOptions.lints.E014 = "deny"`, no `brink.toml` at all: the
/// published severity must promote to `Error` — the file-only counterpart
/// this closes the gap on is
/// `brink_toml_lints_override_promotes_published_severity_to_error` above.
#[test]
fn init_options_lints_deny_promotes_published_severity_to_error() {
    let root = unique_tmp_dir("init-lints-deny");
    std::fs::create_dir_all(&root).unwrap();

    let diags = diagnostics_for_uri_settled_with_init_options(
        &root,
        E014_PROBE_SOURCE,
        json!({ "lints": { "E014": "deny" } }),
    );

    std::fs::remove_dir_all(&root).unwrap();

    assert_eq!(
        e014_severity(&diags),
        Some(1),
        "initializationOptions.lints.E014 = deny should promote the published \
         severity to Error, got: {diags:?}"
    );
}

/// `initializationOptions.denyWarnings = true`: same promotion via the
/// blanket flag rather than a per-code override, mirroring `-D warnings`.
#[test]
fn init_options_deny_warnings_promotes_published_severity_to_error() {
    let root = unique_tmp_dir("init-deny-warnings");
    std::fs::create_dir_all(&root).unwrap();

    let diags = diagnostics_for_uri_settled_with_init_options(
        &root,
        E014_PROBE_SOURCE,
        json!({ "denyWarnings": true }),
    );

    std::fs::remove_dir_all(&root).unwrap();

    assert_eq!(
        e014_severity(&diags),
        Some(1),
        "initializationOptions.denyWarnings = true should promote every Warning \
         (including E014) to Error, got: {diags:?}"
    );
}

/// `initializationOptions.lints.E014 = "deny"` must win over a conflicting
/// `brink.toml [lints] E014 = "allow"` for the same code (#1005 `CLI/API >
/// file > default` precedence) — proves the override is applied *after* the
/// file's own resolution, not merely alongside it.
#[test]
fn init_options_lints_deny_wins_over_conflicting_brink_toml_allow() {
    let root = unique_tmp_dir("init-lints-deny-wins");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("brink.toml"), "[lints]\nE014 = \"allow\"\n").unwrap();

    let diags = diagnostics_for_uri_settled_with_init_options(
        &root,
        E014_PROBE_SOURCE,
        json!({ "lints": { "E014": "deny" } }),
    );

    std::fs::remove_dir_all(&root).unwrap();

    assert_eq!(
        e014_severity(&diags),
        Some(1),
        "initializationOptions.lints.E014 = deny should win over the file's \
         [lints] E014 = \"allow\", got: {diags:?}"
    );
}

/// #1162: `initializationOptions.lints.E014 = "hint"` must publish
/// `DiagnosticSeverity::HINT` (LSP wire value `4`) end to end — the same
/// real-process assertion `init_options_lints_deny_promotes_published_severity_to_error`
/// makes for `deny`/`Error`, now covering the new advisory tier's LSP entry
/// point (`explicit_initialization_lints`, not just `brink.toml`'s
/// `[lints]` table).
#[test]
fn init_options_lints_hint_publishes_hint_severity() {
    let root = unique_tmp_dir("init-lints-hint");
    std::fs::create_dir_all(&root).unwrap();

    let diags = diagnostics_for_uri_settled_with_init_options(
        &root,
        E014_PROBE_SOURCE,
        json!({ "lints": { "E014": "hint" } }),
    );

    std::fs::remove_dir_all(&root).unwrap();

    assert_eq!(
        e014_severity(&diags),
        Some(4),
        "initializationOptions.lints.E014 = hint should publish DiagnosticSeverity::HINT \
         (wire value 4), got: {diags:?}"
    );
}

// ── #1618: DiagnosticTag::UNNECESSARY on the LSP publish channel ───────
//
// `convert::diagnostic_to_lsp`'s `is_unnecessary` classification (unit-level
// coverage in `src/convert.rs`) has to actually reach the wire — this proves
// it through a real `brink-lsp` process and `textDocument/publishDiagnostics`,
// the same "through the LSP publish channel" bar #1367/#1162's own
// integration tests hold themselves to.

/// `-> DONE` immediately followed by more content in the same block lowers
/// to `E033` ("unreachable code after divert"), `Warning` by default — one
/// of the two codes `is_unnecessary` recognizes.
const E033_PROBE_SOURCE: &str = "\
== start ==
-> DONE
Unreachable.
";

fn diag_tags(diags: &[Value], code: &str) -> Option<Vec<u64>> {
    diags
        .iter()
        .find(|d| d["code"].as_str() == Some(code))
        .map(|d| {
            d["tags"]
                .as_array()
                .map(|arr| arr.iter().filter_map(Value::as_u64).collect())
                .unwrap_or_default()
        })
}

/// `E033` must publish with `tags: [1]` (`DiagnosticTag::UNNECESSARY`'s wire
/// value) so a client dims the unreachable statement instead of just
/// underlining it — the actual UX payoff #1162 asked for and #1615 deferred.
#[test]
fn e033_unreachable_code_publishes_unnecessary_tag() {
    let root = unique_tmp_dir("unnecessary-tag-e033");
    std::fs::create_dir_all(&root).unwrap();

    let diags = diagnostics_for_uri_settled(&root, E033_PROBE_SOURCE);

    std::fs::remove_dir_all(&root).unwrap();

    assert_eq!(
        diag_tags(&diags, "E033"),
        Some(vec![1]),
        "E033 should publish DiagnosticTag::UNNECESSARY (wire value 1), got: {diags:?}"
    );
}

/// `E014` (bare `~`, "logic line has no effect") is deliberately excluded
/// from the unnecessary-tag set (see `is_unnecessary`'s doc comment — some of
/// its emission sites are malformed logic that needs fixing, not deleting),
/// so it must publish with no `tags` field at all end to end, mirroring
/// `convert::diagnostic_to_lsp_does_not_tag_unrelated_codes` at the
/// unit level.
#[test]
fn e014_no_effect_does_not_publish_unnecessary_tag() {
    let root = unique_tmp_dir("unnecessary-tag-e014-excluded");
    std::fs::create_dir_all(&root).unwrap();

    let diags = diagnostics_for_uri_settled(&root, E014_PROBE_SOURCE);

    std::fs::remove_dir_all(&root).unwrap();

    let e014 = diags
        .iter()
        .find(|d| d["code"].as_str() == Some("E014"))
        .expect("expected an E014 diagnostic");
    assert!(
        e014.get("tags").is_none(),
        "E014 should publish with no tags field, got: {e014:?}"
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

/// Read messages until a `$/brink/backgroundAnalysisComplete` whose
/// `file_count` satisfies `is_target_pass` arrives, returning the last
/// version-less `textDocument/publishDiagnostics` set observed for `uri`
/// along the way — the background-analysis set, per the #695 convention
/// every helper above already relies on. Each call consumes only the
/// messages up to and including its own terminal signal, so sequential
/// calls on the same session observe successive passes without racing each
/// other, *provided* the predicate is specific enough to reject an earlier
/// pass's own signal.
///
/// `file_count` is the *total* number of files currently tracked in
/// `ProjectDb`, not a per-pass delta — a bare `file_count >= 1` (what
/// [`wait_for_next_analysis_pass`] uses) is satisfied by the very first
/// analysis pass of a session and stays true forever after. A caller that
/// needs to pin down a *specific later* batch (e.g. to prove a
/// `didChangeWatchedFiles` notification was actually processed, not just
/// that some earlier pass already happened to settle the same bound) must
/// supply a predicate only that batch's resulting `file_count` can satisfy
/// (#1415 review finding: `did_change_watched_files_skips_ignored_dirs`
/// used `>= 1` and could settle on the initial `didOpen` pass's own
/// completion signal instead of the batch under test).
fn wait_for_analysis_pass_where(
    stdout: &mut BufReader<ChildStdout>,
    uri: &str,
    max_messages: u64,
    is_target_pass: impl Fn(u64) -> bool,
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
            if is_target_pass(file_count) {
                settled = true;
                break;
            }
        }
    }
    assert!(
        settled,
        "background analysis never signaled a matching completion for {uri} within {max_messages} messages"
    );
    last_diags
}

/// [`wait_for_analysis_pass_where`] with the loosest possible predicate
/// (`file_count >= 1`) — good enough for callers that only need to know
/// *some* pass completed (e.g. right after opening the very first file of a
/// session), but see that function's doc comment before reusing this for a
/// later, more specific batch.
fn wait_for_next_analysis_pass(
    stdout: &mut BufReader<ChildStdout>,
    uri: &str,
    max_messages: u64,
) -> Vec<Value> {
    wait_for_analysis_pass_where(stdout, uri, max_messages, |file_count| file_count >= 1)
}

/// #1055 gap 2 (file-watch path): editing `brink.toml` on disk and sending
/// the `workspace/didChangeWatchedFiles` notification the server's own
/// `initialized()`-time watcher registration asks for (`**/brink.toml`, in
/// addition to `**/*.ink` and `**/*.brink`) re-resolves the dialect and
/// re-analyzes on the very next pass — no client restart, no
/// re-`initialize`.
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

/// #1415 regression: `did_change_watched_files` must prune ignored
/// directories the same way every other recursive walk in the codebase
/// does (#1370's config discovery, #1381's native compile walk, #1402's LSP
/// workspace-load walk) — a change event for a file under `target/` must
/// never be admitted into `ProjectDb`.
///
/// The batch pairs the ignored-dir CREATED event with a *legitimate*
/// CREATED event for a new, non-ignored `control.ink` (in addition to the
/// pre-existing `main.ink`), and synchronizes on `file_count >= 2` rather
/// than `>= 1`. `file_count` is `ProjectDb`'s *total* tracked-file count, so
/// `>= 1` is already satisfied by the very first (`didOpen`) pass and stays
/// true forever after — it can settle before this batch is even processed,
/// making the assertion below race the batch instead of observing its
/// result (#1415 review finding: vacuous integration test — a control run
/// with the production guard deleted still passed). `>= 2` can only be
/// satisfied once `control.ink` has actually been admitted (`hidden.ink`
/// must never count towards it), so it pins down this exact batch. The test
/// then asserts *both* that `control.ink`'s knot is found (proving the pass
/// under test is the one observed, and legitimate files aren't collaterally
/// dropped) and that the smuggled knot is not.
#[test]
#[expect(clippy::too_many_lines)]
fn did_change_watched_files_skips_ignored_dirs() {
    let root = unique_tmp_dir("watched-files-ignored-dir");
    std::fs::create_dir_all(&root).unwrap();
    let main_path = root.join("main.ink");
    std::fs::write(&main_path, "== start ==\nHello.\n-> DONE\n").unwrap();

    let bin = env!("CARGO_BIN_EXE_brink-lsp");
    let mut child = Command::new(bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to start brink-lsp");
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    let main_uri = format!("file://{}", main_path.display());

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
        &json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}),
    );

    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": main_uri,
                    "languageId": "ink",
                    "version": 1,
                    "text": "== start ==\nHello.\n-> DONE\n",
                }
            }
        }),
    );

    // file_count == 1 here (main.ink only) — the bound this test must not
    // be satisfied by.
    wait_for_analysis_pass_where(&mut stdout, &main_uri, 2000, |file_count| file_count >= 1);

    // Plant a uniquely-named knot under target/, touch main.ink, and add a
    // legitimate new control.ink so the batch has an unambiguous file_count
    // >= 2 to synchronize on.
    let target_dir = root.join("target/debug");
    std::fs::create_dir_all(&target_dir).unwrap();
    let hidden_path = target_dir.join("hidden.ink");
    std::fs::write(&hidden_path, "== smuggled_only_here ==\nSecret.\n-> DONE\n").unwrap();
    std::fs::write(&main_path, "== start ==\nHello again.\n-> DONE\n").unwrap();
    let control_path = root.join("control.ink");
    std::fs::write(
        &control_path,
        "== control_knot_visible ==\nControl.\n-> DONE\n",
    )
    .unwrap();

    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "workspace/didChangeWatchedFiles",
            "params": {
                "changes": [
                    {"uri": main_uri, "type": 2},
                    {"uri": format!("file://{}", hidden_path.display()), "type": 1},
                    {"uri": format!("file://{}", control_path.display()), "type": 1},
                ]
            }
        }),
    );

    wait_for_analysis_pass_where(&mut stdout, &main_uri, 2000, |file_count| file_count >= 2);

    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "workspace/symbol",
            "params": {"query": "smuggled_only_here"}
        }),
    );
    let (smuggled_resp, _) = recv_response(&mut stdout, 2);

    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "workspace/symbol",
            "params": {"query": "control_knot_visible"}
        }),
    );
    let (control_resp, _) = recv_response(&mut stdout, 3);

    drop(stdin);
    drop(stdout);
    let _ = child.wait();
    std::fs::remove_dir_all(&root).unwrap();

    let smuggled_results = smuggled_resp["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        smuggled_results.is_empty(),
        "a change event under target/ must never enter ProjectDb, but workspace/symbol found: {smuggled_results:?}"
    );

    let control_results = control_resp["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        !control_results.is_empty(),
        "control.ink is a legitimate, non-ignored file — it must be admitted and found by workspace/symbol"
    );
}

/// #1415 review finding (regression): the ignored-dir guard must gate
/// *admission* only, never a file already tracked in `ProjectDb`.
/// `load_file_from_disk`/`chase_includes` (and `did_open`) resolve
/// `INCLUDE` targets without pruning, so `INCLUDE
/// node_modules/shared/lib.ink` is loaded despite living under an ignored
/// dir — and once tracked, that file must keep syncing on every later
/// CHANGED/DELETED event exactly like any other file, not get silently
/// skipped because its path matches an ignored-dir component. Before the
/// fix, a DELETED event for such a file left a permanently stale
/// `ProjectDb` entry with diagnostics that were never cleared.
#[test]
#[expect(clippy::too_many_lines)]
fn did_change_watched_files_syncs_already_tracked_file_under_node_modules() {
    let root = unique_tmp_dir("watched-files-tracked-node-modules");
    std::fs::create_dir_all(&root).unwrap();
    let main_path = root.join("main.ink");
    std::fs::write(
        &main_path,
        "INCLUDE node_modules/shared/lib.ink\n== start ==\nHello.\n-> DONE\n",
    )
    .unwrap();
    let lib_dir = root.join("node_modules/shared");
    std::fs::create_dir_all(&lib_dir).unwrap();
    let lib_path = lib_dir.join("lib.ink");
    std::fs::write(&lib_path, "== helper_symbol_xyz ==\nHelp.\n-> DONE\n").unwrap();

    let bin = env!("CARGO_BIN_EXE_brink-lsp");
    let mut child = Command::new(bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to start brink-lsp");
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    let main_uri = format!("file://{}", main_path.display());
    let lib_uri = format!("file://{}", lib_path.display());

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
        &json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}),
    );

    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": main_uri,
                    "languageId": "ink",
                    "version": 1,
                    "text": "INCLUDE node_modules/shared/lib.ink\n== start ==\nHello.\n-> DONE\n",
                }
            }
        }),
    );

    // `chase_includes` loads lib.ink synchronously within did_open, before
    // the trigger — so the very first pass already covers both files.
    wait_for_analysis_pass_where(&mut stdout, &main_uri, 2000, |file_count| file_count >= 2);

    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "workspace/symbol",
            "params": {"query": "helper_symbol_xyz"}
        }),
    );
    let (before_resp, _) = recv_response(&mut stdout, 2);
    let before_results = before_resp["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        !before_results.is_empty(),
        "an INCLUDE resolving into node_modules/ must still be loaded (control assertion), got: {before_resp:?}"
    );

    // Regression: CHANGED on the already-tracked node_modules/ file must
    // keep syncing. Pair it with a brand-new, non-ignored companion file so
    // the resulting file_count (3) is unambiguous.
    std::fs::write(
        &lib_path,
        "== helper_symbol_xyz_v2 ==\nHelp again.\n-> DONE\n",
    )
    .unwrap();
    let extra_path = root.join("extra.ink");
    std::fs::write(&extra_path, "== extra_companion_knot ==\nExtra.\n-> DONE\n").unwrap();

    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "workspace/didChangeWatchedFiles",
            "params": {
                "changes": [
                    {"uri": lib_uri.clone(), "type": 2},
                    {"uri": format!("file://{}", extra_path.display()), "type": 1},
                ]
            }
        }),
    );

    wait_for_analysis_pass_where(&mut stdout, &main_uri, 2000, |file_count| file_count >= 3);

    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "workspace/symbol",
            "params": {"query": "helper_symbol_xyz_v2"}
        }),
    );
    let (changed_resp, _) = recv_response(&mut stdout, 3);
    let changed_results = changed_resp["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        !changed_results.is_empty(),
        "a CHANGED event on an already-tracked node_modules/ file must still sync, got: {changed_resp:?}"
    );

    // Regression (the reviewer's own reproduction): DELETED on the
    // already-tracked node_modules/ file must not be short-circuited by the
    // ignored-dir guard either.
    std::fs::remove_file(&lib_path).unwrap();

    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "workspace/didChangeWatchedFiles",
            "params": {
                "changes": [
                    {"uri": lib_uri, "type": 3},
                ]
            }
        }),
    );

    wait_for_analysis_pass_where(&mut stdout, &main_uri, 2000, |file_count| file_count == 2);

    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "workspace/symbol",
            "params": {"query": "helper_symbol_xyz_v2"}
        }),
    );
    let (after_resp, _) = recv_response(&mut stdout, 4);

    drop(stdin);
    drop(stdout);
    let _ = child.wait();
    std::fs::remove_dir_all(&root).unwrap();

    let after_results = after_resp["result"].as_array().cloned().unwrap_or_default();
    assert!(
        after_results.is_empty(),
        "a DELETED event on an already-tracked node_modules/ file must remove it from ProjectDb, but workspace/symbol found: {after_results:?}"
    );
}

/// Issue #1424: `did_open` is explicit path admission (the client is
/// telling us the user opened this exact file), not a directory walk, so it
/// must admit a file whose *own* path lives under `node_modules/` even when
/// no `INCLUDE` is involved — unlike the #1415 regression test above, which
/// only proves this for a file reached indirectly via `chase_includes`.
/// Pins the `is_ignored_dir` "Admission policy" doc's claim for the
/// `did_open` call site specifically.
#[test]
fn did_open_admits_file_directly_under_ignored_dir() {
    let root = unique_tmp_dir("did-open-node-modules");
    let vendor_dir = root.join("node_modules/vendor-pkg");
    std::fs::create_dir_all(&vendor_dir).unwrap();
    let vendor_path = vendor_dir.join("index.ink");
    std::fs::write(
        &vendor_path,
        "== vendored_knot_opened_directly ==\nVendored.\n-> DONE\n",
    )
    .unwrap();

    let bin = env!("CARGO_BIN_EXE_brink-lsp");
    let mut child = Command::new(bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to start brink-lsp");
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    let vendor_uri = format!("file://{}", vendor_path.display());

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
        &json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}),
    );

    // No workspace files exist outside node_modules/, so `initialized`'s
    // workspace scan admits nothing — the only way this file can reach
    // `ProjectDb` in this test is `did_open` admitting it directly.
    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": vendor_uri,
                    "languageId": "ink",
                    "version": 1,
                    "text": "== vendored_knot_opened_directly ==\nVendored.\n-> DONE\n",
                }
            }
        }),
    );

    wait_for_analysis_pass_where(&mut stdout, &vendor_uri, 2000, |file_count| file_count >= 1);

    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "workspace/symbol",
            "params": {"query": "vendored_knot_opened_directly"}
        }),
    );
    let (resp, _) = recv_response(&mut stdout, 2);

    drop(stdin);
    drop(stdout);
    let _ = child.wait();
    std::fs::remove_dir_all(&root).unwrap();

    let results = resp["result"].as_array().cloned().unwrap_or_default();
    assert!(
        !results.is_empty(),
        "did_open must admit a file directly under node_modules/ (explicit path admission, \
         not a directory walk), got: {resp:?}"
    );
}

// ── Native `.brink` workspaces (#1526 / #1553) ──────────────────────

/// `market/barter.brink` — the definition side of a two-file native project.
const NATIVE_BARTER: &str = "\
var gold = 10

/// Trade at the market stall.
flow haggle() {
  You haggle over the price.
}
";

/// `main.brink` — the reference side.
const NATIVE_MAIN: &str = "\
use story::market::barter::haggle;

flow start() {
  The market is busy.
  -> haggle
}
";

/// LSP-level regression test for #1526 (issue #1553): the background
/// analysis pass must qualify `DefinitionId`s with the db's `ModuleMap`.
///
/// A native file's module is its path (`market/barter.brink` →
/// `story::market::barter`) and is always *declared*, so it qualifies
/// identity — unlike the undeclared stem-modules the ink corpus uses, where
/// module-blind and module-aware hashing agree by construction. The visible
/// tell is the hover **effects** row: `brink_ide::hover` renders it from
/// `db.effects(info.id)`, so it appears only when the id the background pass
/// minted is the id the db's per-def queries are keyed by. Before #1526 the
/// pass ran module-blind and every native symbol's row silently vanished.
///
/// Two files, in different directories, precisely because that is where the
/// two identity schemes diverge most visibly: the qualifying module differs
/// per file, so one shared bare-name index cannot stand in for it.
#[test]
fn native_two_file_workspace_hover_keeps_the_db_backed_effect_row() {
    const MAX_MESSAGES: u64 = 2000;

    let root = unique_tmp_dir("native-two-file");
    std::fs::create_dir_all(root.join("market")).unwrap();
    std::fs::write(root.join("market/barter.brink"), NATIVE_BARTER).unwrap();
    std::fs::write(root.join("main.brink"), NATIVE_MAIN).unwrap();

    // No declared dialect: a native mount's *default*
    // (`AnalysisOptions::default()`), matching `brink-ide`'s own
    // `native_cross_file_hover_under_default_dialect`.
    let (mut child, mut stdin, mut stdout) = start_server_at(&root, None);

    // Both files are opened explicitly here — this test is about identity,
    // not admission. (Since #1562 the workspace scan enumerates `.brink`
    // too, so a native sibling reaches the db either way; the
    // `native_two_file_workspace_*_without_opening_the_sibling` test below
    // covers the scan path on its own.)
    let barter_uri = format!("file://{}", root.join("market/barter.brink").display());
    let main_uri = format!("file://{}", root.join("main.brink").display());
    did_open_native(&mut stdin, &barter_uri, NATIVE_BARTER);
    did_open_native(&mut stdin, &main_uri, NATIVE_MAIN);
    // `file_count >= 2` pins the pass that has *both* files — an earlier
    // single-file pass cannot satisfy it (see `wait_for_analysis_pass_where`).
    let _ = wait_for_analysis_pass_where(&mut stdout, &main_uri, MAX_MESSAGES, |c| c >= 2);

    // `flow start()` in `main.brink` (module `story::main`) …
    let start = hover_at(&mut stdin, &mut stdout, 2, &main_uri, 2, 7);
    // … and `flow haggle()` in `market/barter.brink` (module
    // `story::market::barter`) — a per-file divergence would show on one and
    // not the other.
    let haggle = hover_at(&mut stdin, &mut stdout, 3, &barter_uri, 3, 7);

    // The cross-file divert target `-> haggle` in `main.brink`. Until issue
    // #1562 this resolved to nothing — the LSP partitioned projects by the
    // INCLUDE graph and native `.brink` has no INCLUDEs, so each native file
    // was its own single-file project and the divert target was simply not
    // in scope. It now crosses the file boundary (the native module tree is
    // one project), so the hover names the *defining* file.
    let cross_file = hover_at(&mut stdin, &mut stdout, 4, &main_uri, 4, 6);

    drop(stdin);
    drop(stdout);
    let _ = child.wait();
    std::fs::remove_dir_all(&root).unwrap();

    let start_md = start["result"]["contents"]["value"].as_str().unwrap_or("");
    assert!(
        start_md.contains("**knot** `start`") && start_md.contains("**effects**"),
        "native hover lost the db-backed effect row for `start`: {start}"
    );
    assert!(
        start_md.contains("main.brink"),
        "hover must name the defining file: {start}"
    );

    let haggle_md = haggle["result"]["contents"]["value"].as_str().unwrap_or("");
    assert!(
        haggle_md.contains("**knot** `haggle`") && haggle_md.contains("**effects**"),
        "native hover lost the db-backed effect row for `haggle`: {haggle}"
    );
    assert!(
        haggle_md.contains("Trade at the market stall."),
        "hover must carry the doc comment: {haggle}"
    );

    let cross_file_md = cross_file["result"]["contents"]["value"]
        .as_str()
        .unwrap_or("");
    assert!(
        cross_file_md.contains("**knot** `haggle`"),
        "a cross-file native divert must hover its target (#1562): {cross_file}"
    );
    assert!(
        cross_file_md.contains("barter.brink"),
        "the cross-file hover must name the defining file: {cross_file}"
    );
    assert!(
        cross_file_md.contains("**effects**"),
        "the db-backed effect row must survive the identity join across the \
         file boundary: {cross_file}"
    );
}

/// Issue #1580 (RULED 2026-08-03): the LSP must discover *every* governing
/// `brink.toml` in the workspace and give each its own project, applying
/// `brink_driver::native_source_root` per project instead of once to
/// `roots.first()` — editor extent must equal compile extent.
///
/// The fixture plants **two independent copies** of the exact two-file
/// native project `native_two_file_workspace_hover_keeps_the_db_backed_
/// effect_row` above already proves works standalone — one under
/// `game/brink.toml`, one under `demo/brink.toml` — as *sibling*
/// subdirectories of one opened workspace root that itself has no
/// `brink.toml`. Neither sibling is an ancestor of the other, so
/// `native_source_root`'s walk-*up*-from-the-first-root alone can never
/// discover either one; before #1580 the LSP recognized no config at all
/// here and fell back to the bare workspace root as the *single* native
/// root, so `game/market/barter.brink` minted the module
/// `story::game::market::barter` — not `story::market::barter`, what a
/// real, standalone compile of `game/` (using *its own* `brink.toml`)
/// mints. `main.brink`'s `use story::market::barter::haggle;` names exactly
/// that real-compile-identical path, so before the fix it fails to resolve
/// (no db-backed **effects** row on the cross-file divert); after the fix,
/// `game/` and `demo/` are each their own project rooted at their own
/// directory, so the qualified name matches and the hover resolves —
/// independently, and correctly attributed, in *both* siblings at once
/// (proving no cross-project bleed: each hover carries its own sibling's
/// distinct doc comment, never the other's).
#[test]
fn two_sibling_brink_toml_projects_each_get_their_own_root_relative_identity() {
    const MAX_MESSAGES: u64 = 2000;

    let root = unique_tmp_dir("two-sibling-native-projects");
    for (sibling, doc) in [
        ("game", "Trade at the game stall."),
        ("demo", "Trade at the demo stall."),
    ] {
        let barter = format!(
            "var gold = 10\n\n/// {doc}\nflow haggle() {{\n  You haggle over the price.\n}}\n"
        );
        std::fs::create_dir_all(root.join(sibling).join("market")).unwrap();
        std::fs::write(root.join(sibling).join("brink.toml"), "[project]\n").unwrap();
        std::fs::write(root.join(sibling).join("market/barter.brink"), &barter).unwrap();
        std::fs::write(root.join(sibling).join("main.brink"), NATIVE_MAIN).unwrap();
    }

    // The workspace root itself has no `brink.toml` — walking up from it
    // finds nothing, so the legacy default project has no real config
    // either; `game/` and `demo/` are each discovered purely by the
    // downward sibling walk #1580 adds.
    let (mut child, mut stdin, mut stdout) = start_server_at(&root, None);

    let game_main_uri = format!("file://{}", root.join("game/main.brink").display());
    let game_barter_uri = format!("file://{}", root.join("game/market/barter.brink").display());
    let demo_main_uri = format!("file://{}", root.join("demo/main.brink").display());
    let demo_barter_uri = format!("file://{}", root.join("demo/market/barter.brink").display());

    let game_barter_src = std::fs::read_to_string(root.join("game/market/barter.brink")).unwrap();
    let demo_barter_src = std::fs::read_to_string(root.join("demo/market/barter.brink")).unwrap();

    did_open_native(&mut stdin, &game_main_uri, NATIVE_MAIN);
    did_open_native(&mut stdin, &game_barter_uri, &game_barter_src);
    did_open_native(&mut stdin, &demo_main_uri, NATIVE_MAIN);
    did_open_native(&mut stdin, &demo_barter_uri, &demo_barter_src);
    let _ = wait_for_analysis_pass_where(&mut stdout, &game_main_uri, MAX_MESSAGES, |c| c >= 4);

    // The cross-file divert target `-> haggle` in each sibling's own
    // `main.brink` (line 4, same shape as `NATIVE_MAIN` above).
    let game_cross = hover_at(&mut stdin, &mut stdout, 2, &game_main_uri, 4, 6);
    let demo_cross = hover_at(&mut stdin, &mut stdout, 3, &demo_main_uri, 4, 6);

    drop(stdin);
    drop(stdout);
    let _ = child.wait();
    std::fs::remove_dir_all(&root).unwrap();

    let game_md = game_cross["result"]["contents"]["value"]
        .as_str()
        .unwrap_or("");
    let demo_md = demo_cross["result"]["contents"]["value"]
        .as_str()
        .unwrap_or("");

    assert!(
        game_md.contains("**knot** `haggle`") && game_md.contains("**effects**"),
        "game/main.brink's `use story::market::barter::haggle;` must resolve \
         against game/'s *own* brink.toml root (story::market::barter), not \
         the workspace root's story::game::market::barter: {game_cross}"
    );
    assert!(
        game_md.contains("Trade at the game stall."),
        "game/'s cross-file hover must carry game/'s own doc comment, never demo's: {game_cross}"
    );
    assert!(
        demo_md.contains("**knot** `haggle`") && demo_md.contains("**effects**"),
        "demo/main.brink's `use story::market::barter::haggle;` must resolve \
         against demo/'s *own* brink.toml root, independently of game/'s: {demo_cross}"
    );
    assert!(
        demo_md.contains("Trade at the demo stall."),
        "demo/'s cross-file hover must carry demo/'s own doc comment, never game's \
         (cross-project bleed): {demo_cross}"
    );
}

/// Start a server rooted at `root`, `initialize` + `initialized` it, and
/// return its pipes. `dialect` is written verbatim into
/// `initializationOptions.dialect` when given.
fn start_server_at(
    root: &std::path::Path,
    dialect: Option<&str>,
) -> (Child, ChildStdin, BufReader<ChildStdout>) {
    let bin = env!("CARGO_BIN_EXE_brink-lsp");
    let mut child = Command::new(bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to start brink-lsp");
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    let mut params = json!({
        "capabilities": {},
        "rootUri": format!("file://{}", root.display()),
    });
    if let Some(dialect) = dialect {
        params["initializationOptions"] = json!({ "dialect": dialect });
    }
    send(
        &mut stdin,
        &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": params}),
    );
    let (_init_resp, _) = recv_response(&mut stdout, 1);
    send(
        &mut stdin,
        &json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}),
    );

    (child, stdin, stdout)
}

/// Send a `textDocument/didOpen` for a native document.
fn did_open_native(stdin: &mut ChildStdin, uri: &str, text: &str) {
    send(
        stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {"textDocument": {
                "uri": uri, "languageId": "brink", "version": 1, "text": text,
            }},
        }),
    );
}

/// Send the request `build(id)` produces, and keep re-sending it (with a
/// fresh id, briefly backing off) until `ready` accepts the response or the
/// attempts run out. Returns the last response either way.
///
/// The point is the *failure* mode: a request is always answered, so a
/// regression surfaces as a response the caller can assert on, whereas
/// pinning the same expectation to a `$/brink/backgroundAnalysisComplete`
/// predicate the regression makes unsatisfiable would block the reader
/// forever. Requests only need retrying at all because a server-side race
/// (the workspace scan versus the client's own `didOpen`) decides which
/// analysis pass lands first.
fn retry_request(
    stdin: &mut ChildStdin,
    stdout: &mut BufReader<ChildStdout>,
    first_id: u64,
    build: impl Fn(u64) -> Value,
    ready: impl Fn(&Value) -> bool,
) -> Value {
    const ATTEMPTS: u64 = 10;

    let mut last = Value::Null;
    for attempt in 0..ATTEMPTS {
        let id = first_id + attempt;
        send(stdin, &build(id));
        last = recv_response(stdout, id).0;
        if ready(&last) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    last
}

/// Request `textDocument/hover` at a position and return the raw response.
fn hover_at(
    stdin: &mut ChildStdin,
    stdout: &mut BufReader<ChildStdout>,
    id: u64,
    uri: &str,
    line: u32,
    character: u32,
) -> Value {
    send(
        stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/hover",
            "params": {
                "textDocument": {"uri": uri},
                "position": {"line": line, "character": character},
            },
        }),
    );
    recv_response(stdout, id).0
}

/// Issue #1562, the headline gap: **cross-file navigation in a native
/// workspace**, exercised the way a user actually meets it — one file open,
/// the sibling module merely present on disk.
///
/// Two things had to be wrong for this to fail, and both are fixed here:
/// the workspace scan enumerated `.ink` only, so `market/barter.brink` never
/// entered the db at all; and `compute_projects` grouped by INCLUDE
/// reachability, which native has none of, so even once both files *were* in
/// the db they were two disjoint single-file projects and `haggle` was not
/// in `main.brink`'s navigation scope.
#[test]
fn native_two_file_workspace_goes_to_definition_without_opening_the_sibling() {
    const MAX_MESSAGES: u64 = 2000;

    let root = unique_tmp_dir("native-cross-file-def");
    std::fs::create_dir_all(root.join("market")).unwrap();
    std::fs::write(root.join("market/barter.brink"), NATIVE_BARTER).unwrap();
    std::fs::write(root.join("main.brink"), NATIVE_MAIN).unwrap();

    let (mut child, mut stdin, mut stdout) = start_server_at(&root, Some("brink"));

    // Only `main.brink` is opened. `market/barter.brink` is on disk and must
    // reach the db through the `initialized` workspace scan.
    let main_uri = format!("file://{}", root.join("main.brink").display());
    let barter_uri = format!("file://{}", root.join("market/barter.brink").display());
    did_open_native(&mut stdin, &main_uri, NATIVE_MAIN);
    let _ = wait_for_next_analysis_pass(&mut stdout, &main_uri, MAX_MESSAGES);

    // Both requests are *retried* rather than pinned to a `file_count >= 2`
    // analysis pass: the scan and the `didOpen` race, so the first pass to
    // complete may be either one's. A request always gets a response, so a
    // regression fails these assertions instead of blocking on a completion
    // notification that will never arrive.
    let definition = retry_request(
        &mut stdin,
        &mut stdout,
        2,
        |id| {
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "textDocument/definition",
                "params": {
                    // The cross-file divert target `-> haggle` in `main.brink`.
                    "textDocument": {"uri": &main_uri},
                    "position": {"line": 4, "character": 6},
                },
            })
        },
        |resp| resp["result"]["uri"].is_string(),
    );

    // Find-references from the definition side is the same scope read the
    // other way: the reference lives in a file the defining module never
    // mentions.
    let references = retry_request(
        &mut stdin,
        &mut stdout,
        20,
        |id| {
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "textDocument/references",
                "params": {
                    "textDocument": {"uri": &barter_uri},
                    "position": {"line": 3, "character": 7},
                    "context": {"includeDeclaration": false},
                },
            })
        },
        |resp| {
            resp["result"]
                .as_array()
                .is_some_and(|locs| !locs.is_empty())
        },
    );

    drop(stdin);
    drop(stdout);
    let _ = child.wait();
    std::fs::remove_dir_all(&root).unwrap();

    let target = definition["result"]["uri"].as_str().unwrap_or("");
    assert!(
        target.ends_with("market/barter.brink"),
        "go-to-definition must cross into the sibling native module (#1562), \
         got: {definition}"
    );

    let ref_uris: Vec<&str> = references["result"]
        .as_array()
        .map(|locs| {
            locs.iter()
                .filter_map(|loc| loc["uri"].as_str())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    assert!(
        ref_uris.iter().any(|uri| uri.ends_with("main.brink")),
        "find-references must see the referring native module (#1562), \
         got: {references}"
    );
}

/// `market/vendor.brink` — declares the `@VENDOR` cue that
/// `market_vendor_cue_completes_across_files_without_opening_the_declaring_file`
/// proves completes in a sibling file that never opens this one.
const NATIVE_VENDOR: &str = "\
flow sell() {
  @VENDOR
  Something for the road?
}
";

/// `main.brink` for the same test — no `use` of `vendor.brink` at all: the
/// harvest index is not import-scoped (issue #2114/#2134's "harvest by
/// default" — `harvest_index_query` merges every project file unconditionally,
/// unlike symbol completion's reachability filter), so this file has zero
/// static relationship to `vendor.brink` beyond sharing a workspace.
const NATIVE_CUE_MAIN: &str = "\
flow start() {
  @
}
";

/// Request `textDocument/completion` at a position and return the raw
/// response.
fn completion_at(
    stdin: &mut ChildStdin,
    stdout: &mut BufReader<ChildStdout>,
    id: u64,
    uri: &str,
    line: u32,
    character: u32,
) -> Value {
    send(
        stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/completion",
            "params": {
                "textDocument": {"uri": uri},
                "position": {"line": line, "character": character},
            },
        }),
    );
    recv_response(stdout, id).0
}

/// Issue #2134's headline deliverable, proven the way a user actually meets
/// it: `@VENDOR` is declared only in `market/vendor.brink`, which this test
/// never opens — it merely sits on disk, reaching the db through the same
/// `initialized` workspace scan #1562's cross-file navigation test above
/// relies on. Completion right after `@` in the unrelated, never-imported
/// `main.brink` must still offer `VENDOR`: "every @NAME cue in the project
/// completes everywhere" (`docs/prose-dialect-spec.md` §5), not just within
/// the file that declared it.
#[test]
fn market_vendor_cue_completes_across_files_without_opening_the_declaring_file() {
    const MAX_MESSAGES: u64 = 2000;

    let root = unique_tmp_dir("native-cross-file-cue");
    std::fs::create_dir_all(root.join("market")).unwrap();
    std::fs::write(root.join("market/vendor.brink"), NATIVE_VENDOR).unwrap();
    std::fs::write(root.join("main.brink"), NATIVE_CUE_MAIN).unwrap();

    let (mut child, mut stdin, mut stdout) = start_server_at(&root, Some("brink"));

    // Only `main.brink` is opened. `market/vendor.brink` is on disk and must
    // reach the db through the workspace scan, exactly like #1562's
    // `market/barter.brink` above.
    let main_uri = format!("file://{}", root.join("main.brink").display());
    did_open_native(&mut stdin, &main_uri, NATIVE_CUE_MAIN);
    let _ = wait_for_next_analysis_pass(&mut stdout, &main_uri, MAX_MESSAGES);

    // Retried rather than pinned to a specific analysis pass — the workspace
    // scan and this file's own `didOpen` race, same as the definition test.
    let mut completion_resp = Value::Null;
    for attempt in 0..10u64 {
        completion_resp = completion_at(
            &mut stdin,
            &mut stdout,
            100 + attempt,
            &main_uri,
            1,
            3, // right after `@` on `  @` (line 1, the flow body).
        );
        let offers_vendor = completion_resp["result"]
            .as_array()
            .is_some_and(|items| items.iter().any(|it| it["label"] == "VENDOR"));
        if offers_vendor {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    drop(stdin);
    drop(stdout);
    let _ = child.wait();
    std::fs::remove_dir_all(&root).unwrap();

    let labels: Vec<&str> = completion_resp["result"]
        .as_array()
        .map(|items| items.iter().filter_map(|it| it["label"].as_str()).collect())
        .unwrap_or_default();
    assert!(
        labels.contains(&"VENDOR"),
        "a cue declared only in market/vendor.brink (never opened) must \
         complete in main.brink (issue #2134): {completion_resp}"
    );
}

/// A two-word cue name (`MARKET VENDOR`), declared once in a sibling file —
/// `cue_name()` allows internal whitespace (only `#`/`:`/newline end the
/// name), so this is a legal declaration, not a malformed one.
const NATIVE_MARKET_VENDOR_DECL: &str = "\
flow sell() {
  @MARKET VENDOR
  Something for the road?
}
";

/// `main.brink` for
/// `cue_completion_does_not_offer_a_multi_word_cue_mid_second_word` — the
/// cursor sits right after `@MARKET VEN`, mid-typing the cue's *second*
/// word.
const NATIVE_CUE_PARTIAL_SECOND_WORD: &str = "\
flow start() {
  @MARKET VEN
}
";

/// Review finding on #2134 (blocking): every completion client replaces only
/// the `\w`-contiguous word under the cursor (`CodeMirror`'s
/// `ctx.matchBefore(/[\w.]+/)`; `brink-lsp`'s own `CompletionItem` carries no
/// `text_edit`/`filter_text`). Offering the full harvested `MARKET VENDOR`
/// label while the user is still typing the cue's second word (`@MARKET
/// VEN`) would therefore replace only `VEN`, corrupting the line into
/// `@MARKET MARKET VENDOR` on accept. `detect_completion_context` must stop
/// recognizing `CueName` context at the name's first whitespace, so this
/// position falls back to the ordinary completion list instead — proven
/// here at the insertion/response level (not just `brink-ide`'s
/// context-detection unit tests), through the real
/// `textDocument/completion` request the corrupting scenario actually
/// hits.
#[test]
fn cue_completion_does_not_offer_a_multi_word_cue_mid_second_word() {
    const MAX_MESSAGES: u64 = 2000;

    let root = unique_tmp_dir("native-cue-multiword-no-corrupt");
    std::fs::create_dir_all(root.join("market")).unwrap();
    std::fs::write(root.join("market/vendor.brink"), NATIVE_MARKET_VENDOR_DECL).unwrap();
    std::fs::write(root.join("main.brink"), NATIVE_CUE_PARTIAL_SECOND_WORD).unwrap();

    let (mut child, mut stdin, mut stdout) = start_server_at(&root, Some("brink"));

    let main_uri = format!("file://{}", root.join("main.brink").display());
    did_open_native(&mut stdin, &main_uri, NATIVE_CUE_PARTIAL_SECOND_WORD);
    let _ = wait_for_next_analysis_pass(&mut stdout, &main_uri, MAX_MESSAGES);

    // Retried like the sibling cross-file test above — the workspace scan
    // that loads `market/vendor.brink` races this file's own `didOpen`.
    // "Settled" here means the harvested cue has definitely reached the db
    // (`VENDOR`-only completion would appear at the single-word position);
    // this test's own request is always at the multi-word position, so it
    // never itself flips true/false on the race — only whether the *other*
    // sibling test's harvest has propagated far enough to trust a negative
    // result from this one.
    let mut settled = false;
    let mut completion_resp = Value::Null;
    for attempt in 0..20u64 {
        completion_resp = completion_at(
            &mut stdin,
            &mut stdout,
            300 + attempt,
            &main_uri,
            1,
            13, // right after `@MARKET VEN` on `  @MARKET VEN`.
        );
        let vendor_probe = completion_at(
            &mut stdin,
            &mut stdout,
            400 + attempt,
            &main_uri,
            1,
            3, // right after `@` alone — the single-word cue position.
        );
        settled = vendor_probe["result"]
            .as_array()
            .is_some_and(|items| items.iter().any(|it| it["label"] == "MARKET VENDOR"));
        if settled {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    drop(stdin);
    drop(stdout);
    let _ = child.wait();
    std::fs::remove_dir_all(&root).unwrap();

    assert!(
        settled,
        "harness precondition: the harvested `MARKET VENDOR` cue never \
         reached the db within the retry budget, so this test cannot trust \
         a negative result: {completion_resp}"
    );

    let labels: Vec<&str> = completion_resp["result"]
        .as_array()
        .map(|items| items.iter().filter_map(|it| it["label"].as_str()).collect())
        .unwrap_or_default();
    assert!(
        !labels.contains(&"MARKET VENDOR"),
        "completion mid-second-word of a partially typed multi-word cue \
         must NOT offer the full cue label — accepting it would corrupt \
         the line into `@MARKET MARKET VENDOR` (review finding on #2134): \
         {completion_resp}"
    );
}

/// `alpha.brink` and `beta.brink` — two native modules that each declare a
/// flow named `greet`. Legal: a native file's module is its path and is
/// always *declared*, so the two are `story::alpha::greet` and
/// `story::beta::greet` and they coexist (M-2d, issue #790) — but only under
/// `Dialect::Brink`, which is exactly the input the LSP's own `ProjectDb`
/// never received.
const NATIVE_ALPHA: &str = "\
/// Greeting from alpha.
flow greet() {
  Alpha says hello.
}
";

const NATIVE_BETA: &str = "\
/// Greeting from beta.
flow greet() {
  Beta says hello.
}
";

/// The sibling bug folded into #1562: `Backend`'s `ProjectDb` never received
/// `set_analysis_options`, so every db-backed request handler — hover's
/// `db.effects`/`db.signature`/`db.infer_body`, inlay hints, code actions,
/// rename's UFCS resolution — ran under `AnalysisOptions::default()` while
/// the published diagnostics used the client's declared options. The #1553
/// bug class in a second db holder.
///
/// `Dialect::StrictInk` (the default) gates off M-2d cross-declared-module
/// coexistence in `symbol_index_query`, so under the stale default the db's
/// index kept only *one* of the two `greet`s. The visible tell is hover's
/// effects row, which `brink_ide::hover` renders from `db.effects(info.id)`:
/// the analysis (running under the declared `brink` dialect) minted an id
/// for both, but only one of them keyed anything in the db.
#[test]
fn native_homonym_flows_keep_the_db_backed_hover_under_the_declared_dialect() {
    const MAX_MESSAGES: u64 = 2000;

    let root = unique_tmp_dir("native-homonym-dialect");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("alpha.brink"), NATIVE_ALPHA).unwrap();
    std::fs::write(root.join("beta.brink"), NATIVE_BETA).unwrap();

    let (mut child, mut stdin, mut stdout) = start_server_at(&root, Some("brink"));

    let alpha_uri = format!("file://{}", root.join("alpha.brink").display());
    let beta_uri = format!("file://{}", root.join("beta.brink").display());
    did_open_native(&mut stdin, &alpha_uri, NATIVE_ALPHA);
    did_open_native(&mut stdin, &beta_uri, NATIVE_BETA);
    let _ = wait_for_analysis_pass_where(&mut stdout, &alpha_uri, MAX_MESSAGES, |c| c >= 2);

    // `flow greet()` is line 1 in both files (line 0 is the doc comment).
    let alpha = hover_at(&mut stdin, &mut stdout, 2, &alpha_uri, 1, 7);
    let beta = hover_at(&mut stdin, &mut stdout, 3, &beta_uri, 1, 7);

    drop(stdin);
    drop(stdout);
    let _ = child.wait();
    std::fs::remove_dir_all(&root).unwrap();

    for (label, resp) in [("alpha", &alpha), ("beta", &beta)] {
        let md = resp["result"]["contents"]["value"].as_str().unwrap_or("");
        assert!(
            md.contains("**knot** `greet`"),
            "hover over `{label}`'s own `greet` must resolve: {resp}"
        );
        assert!(
            md.contains("**effects**"),
            "`{label}`'s db-backed effect row is missing — the server's own \
             ProjectDb is analyzing under stale default options: {resp}"
        );
    }
}

/// Default-dialect counterpart of
/// `native_homonym_flows_keep_the_db_backed_hover_under_the_declared_dialect`
/// above (issue #1562 review finding): no `initializationOptions.dialect` at
/// all — the common case, since a native workspace has no ink dialect to
/// declare in the first place. `brink-db`'s `symbol_index_query` now widens
/// M-2d cross-declared-module coexistence with the same `project_is_native`
/// seam `whole_project_diagnostics_query` already uses for the ink-only
/// `E064` gate (`crates/internal/brink-db/src/queries/mod.rs`), so this must
/// hold under the *default* `Dialect::StrictInk` exactly as it holds under a
/// client-declared `dialect: "brink"`.
///
/// Pinned directly against `textDocument/publishDiagnostics` rather than
/// hover alone: before the fix, the stale default dialect made
/// `alpha`/`beta`'s second-registered `greet` an ordinary same-name
/// redefinition, which is a diagnosable `E022` — not just a missing hover
/// row.
#[test]
fn native_homonym_flows_coexist_under_the_default_dialect() {
    const MAX_MESSAGES: u64 = 2000;

    let root = unique_tmp_dir("native-homonym-default-dialect");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("alpha.brink"), NATIVE_ALPHA).unwrap();
    std::fs::write(root.join("beta.brink"), NATIVE_BETA).unwrap();

    // No declared dialect — the case the M-2d gate must not depend on.
    let (mut child, mut stdin, mut stdout) = start_server_at(&root, None);

    let alpha_uri = format!("file://{}", root.join("alpha.brink").display());
    let beta_uri = format!("file://{}", root.join("beta.brink").display());
    did_open_native(&mut stdin, &alpha_uri, NATIVE_ALPHA);
    did_open_native(&mut stdin, &beta_uri, NATIVE_BETA);

    // Collect the version-less background-analysis `publishDiagnostics` set
    // (the #695 convention every helper in this file relies on) for *both*
    // files along the way to the pass that has seen both —
    // `wait_for_analysis_pass_where` only tracks a single `uri`.
    let mut alpha_diags: Vec<Value> = Vec::new();
    let mut beta_diags: Vec<Value> = Vec::new();
    let mut settled = false;
    for _ in 0..MAX_MESSAGES {
        let msg = recv(&mut stdout);
        if msg["method"] == "textDocument/publishDiagnostics" && msg["params"]["version"].is_null()
        {
            let diags = msg["params"]["diagnostics"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            if msg["params"]["uri"] == alpha_uri {
                alpha_diags = diags;
            } else if msg["params"]["uri"] == beta_uri {
                beta_diags = diags;
            }
        } else if msg["method"] == "$/brink/backgroundAnalysisComplete" {
            let file_count = msg["params"]["file_count"].as_u64().unwrap_or(0);
            if file_count >= 2 {
                settled = true;
                break;
            }
        }
    }
    assert!(
        settled,
        "background analysis never signaled a matching completion within {MAX_MESSAGES} messages"
    );

    // `flow greet()` is line 1 in both files (line 0 is the doc comment).
    let alpha = hover_at(&mut stdin, &mut stdout, 2, &alpha_uri, 1, 7);
    let beta = hover_at(&mut stdin, &mut stdout, 3, &beta_uri, 1, 7);

    drop(stdin);
    drop(stdout);
    let _ = child.wait();
    std::fs::remove_dir_all(&root).unwrap();

    assert!(
        alpha_diags.is_empty(),
        "alpha.brink must not get a duplicate-definition diagnostic for its own \
         declared-module `greet` under the default dialect (#1562): {alpha_diags:?}"
    );
    assert!(
        beta_diags.is_empty(),
        "beta.brink must not get a duplicate-definition diagnostic for its own \
         declared-module `greet` under the default dialect (#1562): {beta_diags:?}"
    );

    for (label, resp) in [("alpha", &alpha), ("beta", &beta)] {
        let md = resp["result"]["contents"]["value"].as_str().unwrap_or("");
        assert!(
            md.contains("**knot** `greet`"),
            "hover over `{label}`'s own `greet` must resolve: {resp}"
        );
        assert!(
            md.contains("**effects**"),
            "`{label}`'s db-backed effect row is missing under the default dialect: {resp}"
        );
    }
}

/// Undeclared-rename detection (issue #1672 part 2, docs/modules-spec.md
/// §5): hand-renaming a knot — a plain text edit, never going through the
/// IDE's own F2 rename refactor (`brink_ide::rename`, which stamps `#@was`
/// itself and is covered by that crate's own tests) — surfaces a
/// `DiagnosticSeverity::HINT` "did you rename it?" diagnostic that *survives*
/// into the final published set for the file, not just a `publishDiagnostics`
/// that flashes and is immediately overwritten. End-to-end through a real
/// `brink-lsp` process: this is the part of #1672 with no other black-box
/// coverage (`brink-ide::rename_detection`'s own unit tests exercise the pure
/// diff, not the LSP wiring that surfaces it to an author).
///
/// Review finding on #1672 part 2 (blocking): the original version of this
/// test stopped at the *first* `publishDiagnostics` carrying the hint — the
/// fast per-file publish. It never proved the hint survived the background
/// `analysis_loop`'s own follow-up publish for the same file, which
/// (before the fix) recomputed a set that never carried the hint at all
/// (its own diff ran against a baseline the per-file publish had already
/// overwritten) and, being same-or-newer generation, won
/// `DiagnosticsPublisher`'s anti-downgrade exchange and silently replaced
/// the client's diagnostics with a set missing the hint. This version keeps
/// reading past the first hint-carrying publish, through a background
/// analysis pass (`$/brink/backgroundAnalysisComplete`), and asserts against
/// the *last* diagnostics set actually shown for the file's URI.
///
/// `initializationOptions.dialect: "brink"` is required post-fix: the
/// suspicion is gated on `Dialect::Brink` (`#@was` is a brink-only
/// directive), so under the default `StrictInk` nothing would ever surface
/// here at all — see `backend::tests::rename_suspicion_diags_is_gated_on_brink_dialect`
/// for the StrictInk-suppresses-it half of that gate.
#[test]
#[expect(clippy::too_many_lines)]
fn hand_renaming_a_knot_surfaces_an_undeclared_rename_hint_that_survives_background_analysis() {
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
                "rootUri": Value::Null,
                "initializationOptions": {"dialect": "brink"},
            },
        }),
    );
    let (_init_resp, _) = recv_response(&mut stdout, 1);
    send(
        &mut stdin,
        &json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}),
    );

    let file_uri = "file:///tmp/rename_suspicion_test_story.ink";
    let original = "=== hub ===\nHi.\n-> END\n";
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
                    "text": original,
                }
            }
        }),
    );

    // `did_open`'s own per-file publish records the baseline manifest
    // ("hub") for the next diff, but never hits the wire: `hub.ink` has no
    // diagnostics, and `DiagnosticsPublisher`'s anti-downgrade rule (#615)
    // never sends a *never-published* file's empty set (`publish_decision`:
    // "so a clean file never generates a spurious empty publish") — so
    // there's nothing to wait for here. Send the rename edit immediately.

    // Hand-rename `hub` -> `plaza`: a plain full-document text replacement,
    // not the IDE's rename refactor, so no `#@was` gets written here.
    let renamed = "=== plaza ===\nHi.\n-> END\n";
    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didChange",
            "params": {
                "textDocument": {"uri": file_uri, "version": 2},
                "contentChanges": [{"text": renamed}],
            }
        }),
    );

    let mut first_seen_suspicion = false;
    let mut last_diags_for_uri: Option<Vec<Value>> = None;
    for _ in 0..MAX_MESSAGES {
        let msg = recv(&mut stdout);
        if msg["method"] == "textDocument/publishDiagnostics" && msg["params"]["uri"] == file_uri {
            if let Some(diags) = msg["params"]["diagnostics"].as_array() {
                last_diags_for_uri = Some(diags.clone());
                if diags.iter().any(|d| d["code"] == "rename-suspicion") {
                    first_seen_suspicion = true;
                }
            }
        } else if first_seen_suspicion
            && msg["method"] == "$/brink/backgroundAnalysisComplete"
            && msg["params"]["file_count"].as_u64().is_some_and(|c| c >= 1)
            && last_diags_for_uri
                .as_ref()
                .is_some_and(|diags| diags.iter().any(|d| d["code"] == "rename-suspicion"))
        {
            // A background pass completing *after* the hint first appeared
            // is the stop signal — but only once the LAST publish for the
            // file still carries the hint. The first completion after the
            // hint is NOT guaranteed to be the pass that read the renamed
            // db: a pass already in flight when `did_change` landed (e.g.
            // the startup-triggered one, reading pre-rename content) can
            // complete after `publish_perfile_diagnostics` set
            // `first_seen_suspicion`, clobbering the per-file publish with
            // a stale empty set — the exact ordering a contended CI runner
            // produced (PR #3290's `Test` job). When that happens, the
            // buffered wakeup (`tokio::sync::Notify::notify_one` holds at
            // least one) still owes us a further pass that DID read the
            // renamed content, so keep reading; its publish re-carries the
            // hint and its completion satisfies this arm. Coalescing can
            // merge triggers into that one pass but never drops the
            // buffered wakeup, so this cannot hang — and `MAX_MESSAGES`
            // bounds the loop regardless.
            break;
        }
    }

    drop(stdin);
    drop(stdout);
    let _ = child.wait();

    assert!(
        first_seen_suspicion,
        "expected a rename-suspicion diagnostic after the hand rename"
    );
    let final_diags =
        last_diags_for_uri.expect("expected at least one publishDiagnostics for the renamed file");
    assert!(
        final_diags.iter().any(|d| d["code"] == "rename-suspicion"),
        "the rename-suspicion hint must survive into the final published set, not just flash on \
         the first (per-file) publish and be clobbered by the next background-analysis publish: \
         {final_diags:?}"
    );
    let suspicion = final_diags
        .iter()
        .find(|d| d["code"] == "rename-suspicion")
        .expect("checked above");
    assert_eq!(
        suspicion["severity"].as_u64(),
        Some(4),
        "rename-suspicion must publish at DiagnosticSeverity::HINT (4), got {suspicion}"
    );
    let message = suspicion["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("hub") && message.contains("plaza"),
        "message should name both the vanished and the new symbol: {message}"
    );
    assert!(
        message.contains("#@was(hub)"),
        "message should tell the author the exact directive to add: {message}"
    );
}

/// The dialect-gate half of the finding above: under the default
/// `Dialect::StrictInk` (no `initializationOptions.dialect` at all), the same
/// hand-rename must produce no `rename-suspicion` diagnostic anywhere —
/// `#@was` is a brink-only directive (`dialect_gate.rs`), so surfacing the
/// hint under strict ink would point the author at a directive that itself
/// produces a fresh `E051`. Mirrors
/// `brink_ide::rename::tests::renaming_under_strict_ink_dialect_does_not_stamp_was`,
/// but end-to-end through the LSP wiring rather than the pure `rename()` fn.
#[test]
fn hand_renaming_a_knot_under_strict_ink_dialect_surfaces_no_rename_suspicion() {
    const MAX_MESSAGES: u64 = 500;

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
            "params": {"capabilities": {}, "rootUri": Value::Null},
        }),
    );
    let (_init_resp, _) = recv_response(&mut stdout, 1);
    send(
        &mut stdin,
        &json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}),
    );

    let file_uri = "file:///tmp/rename_suspicion_strict_ink_test_story.ink";
    let original = "=== hub ===\nHi.\n-> END\n";
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
                    "text": original,
                }
            }
        }),
    );

    let renamed = "=== plaza ===\nHi.\n-> END\n";
    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didChange",
            "params": {
                "textDocument": {"uri": file_uri, "version": 2},
                "contentChanges": [{"text": renamed}],
            }
        }),
    );

    // Wait for a background-analysis completion covering the file — by the
    // time it fires, `did_change`'s `mutate_db` (committing the rename) has
    // already run, so this pass is guaranteed to have analyzed the renamed
    // content — then assert no rename-suspicion diagnostic was ever
    // published. (Waiting for more than one such completion risks a hang:
    // `analysis_loop` coalesces rapid triggers into a single pass, so a
    // second one is not guaranteed to ever arrive.)
    let mut saw_suspicion = false;
    for _ in 0..MAX_MESSAGES {
        let msg = recv(&mut stdout);
        if msg["method"] == "textDocument/publishDiagnostics"
            && msg["params"]["uri"] == file_uri
            && let Some(diags) = msg["params"]["diagnostics"].as_array()
            && diags.iter().any(|d| d["code"] == "rename-suspicion")
        {
            saw_suspicion = true;
            break;
        }
        if msg["method"] == "$/brink/backgroundAnalysisComplete"
            && msg["params"]["file_count"].as_u64().is_some_and(|c| c >= 1)
        {
            break;
        }
    }

    drop(stdin);
    drop(stdout);
    let _ = child.wait();

    assert!(
        !saw_suspicion,
        "no rename-suspicion diagnostic may surface under the default Dialect::StrictInk"
    );
}

// ── Native `.brink` routing (#1350 / #2360 / #2368) ─────────────────
//
// `brink-lsp` routed every `.brink` document through the always-ink
// `db.parse`/`brink_syntax::parse` regardless of extension: real semantic
// tokens, inlay hints, and code-transform requests over a native project
// (VS Code, Zed, any real LSP client) got ink-misclassified or ink-only
// output instead of the native analysis that already existed
// (`brink_ide::semantic_tokens_native`, `inlay_hints_native`, and the
// dialect-generic HIR-based `folding`/diagnostics paths). These tests drive
// the real LSP session over stdio, the way an editor does.

/// #1350: opening a `.brink` document and requesting
/// `textDocument/semanticTokens/full` (and `/range`) must classify tokens
/// from the *native* CST, not silently ink-misclassify or come back empty.
///
/// Before the fix, `semantic_tokens_full`/`_range` called `db.parse`
/// unconditionally (always the ink frontend) — `NATIVE_BARTER`'s content
/// (`var gold = 10`, a `///` doc comment, `flow haggle() { … }`) has no
/// meaning as ink source, so ink's `classify_token` recognized almost none
/// of it, yielding empty or near-empty token data.
#[test]
fn native_document_gets_non_empty_semantic_tokens() {
    const MAX_MESSAGES: u64 = 2000;

    let root = unique_tmp_dir("native-semantic-tokens");
    std::fs::create_dir_all(&root).unwrap();

    let (mut child, mut stdin, mut stdout) = start_server_at(&root, Some("brink"));

    let uri = format!("file://{}", root.join("main.brink").display());
    did_open_native(&mut stdin, &uri, NATIVE_BARTER);
    let _ = wait_for_next_analysis_pass(&mut stdout, &uri, MAX_MESSAGES);

    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "textDocument/semanticTokens/full",
            "params": {"textDocument": {"uri": uri}},
        }),
    );
    let (full_resp, _) = recv_response(&mut stdout, 2);

    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "textDocument/semanticTokens/range",
            "params": {
                "textDocument": {"uri": uri},
                "range": {
                    "start": {"line": 0, "character": 0},
                    "end": {"line": 5, "character": 0},
                },
            },
        }),
    );
    let (range_resp, _) = recv_response(&mut stdout, 3);

    drop(stdin);
    drop(stdout);
    let _ = child.wait();
    std::fs::remove_dir_all(&root).unwrap();

    // Delta-encoded groups of 5 ints per token. Measured directly against
    // this fixture: ink-misclassifying `NATIVE_BARTER` (the pre-fix bug)
    // yields exactly 1 token (its `///` doc-comment slashes read as an ink
    // comment; `var`/`flow`/the rest go unrecognized) — the properly routed
    // native classifier yields 7. `> 5` (more than one token) sits strictly
    // between the two, so this fails against the bug and passes against the
    // fix without pinning an exact count that would make the assertion
    // brittle to unrelated classifier changes.
    let full_data = full_resp["result"]["data"]
        .as_array()
        .expect("semanticTokens/full must return a token data array");
    assert!(
        full_data.len() > 5,
        "a .brink document with real native content (var/flow/doc comment) \
         must yield real, non-ink-misclassified semantic tokens (#1350) — \
         more than the single stray token an ink misparse of this content \
         produces, got {} raw ints: {full_resp:?}",
        full_data.len()
    );

    let range_data = range_resp["result"]["data"]
        .as_array()
        .expect("semanticTokens/range must return a token data array");
    assert!(
        range_data.len() > 5,
        "semanticTokens/range over the same native content must also yield \
         real, non-ink-misclassified tokens (#1350), got {} raw ints: {range_resp:?}",
        range_data.len()
    );
}

/// Bounded scan for the first `publishDiagnostics` naming `uri`, mirroring
/// the pattern every other message-scanning test in this file already uses
/// (e.g. the rename-suspicion scan) rather than an unbounded `loop { recv(..)
/// }` — CLAUDE.md's "guard against unbounded growth" applies to a test
/// harness loop just as much as production code.
fn diagnostics_for(
    stdout: &mut BufReader<ChildStdout>,
    uri: &str,
    max_messages: u64,
) -> Option<Vec<Value>> {
    for _ in 0..max_messages {
        let msg = recv(stdout);
        if msg["method"] == "textDocument/publishDiagnostics" && msg["params"]["uri"] == uri {
            return Some(
                msg["params"]["diagnostics"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default(),
            );
        }
    }
    None
}

/// #1350 item 3 (diagnostics): a `.brink` document with a real native parse
/// error must surface it through `publishDiagnostics` — the per-file
/// diagnostics path (`ProjectDb::file_diagnostics` → `lowered_query`)
/// already dispatches on the file's own dialect, so this pins the "already
/// works" half of #1350 as a regression rather than leaving it unverified.
///
/// A separate, single-file server session per case (rather than two
/// documents in one session): the diagnostics contract under test —
/// error-shows / clean-stays-empty — does not depend on any other document
/// sharing the session, and keeping each case isolated avoids coupling this
/// regression test to unrelated multi-file re-analysis timing.
#[test]
fn native_document_diagnostics_show_parse_error() {
    const MAX_MESSAGES: u64 = 2000;

    let root = unique_tmp_dir("native-diagnostics-broken");
    std::fs::create_dir_all(&root).unwrap();
    let (mut child, mut stdin, mut stdout) = start_server_at(&root, Some("brink"));

    // Unclosed `flow` body — a genuine native parse error.
    let broken_uri = format!("file://{}", root.join("broken.brink").display());
    let broken_src = "flow start() {\n  Hello\n";
    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {"textDocument": {
                "uri": broken_uri, "languageId": "brink", "version": 1, "text": broken_src,
            }},
        }),
    );
    let broken_diags = diagnostics_for(&mut stdout, &broken_uri, MAX_MESSAGES);

    drop(stdin);
    drop(stdout);
    let _ = child.wait();
    std::fs::remove_dir_all(&root).unwrap();

    assert!(
        broken_diags.is_some(),
        "no publishDiagnostics for {broken_uri} within {MAX_MESSAGES} messages"
    );
    let broken_diags = broken_diags.expect("just asserted above");
    assert!(
        !broken_diags.is_empty(),
        "an unclosed native flow body must publish a diagnostic: {broken_diags:?}"
    );
}

/// #1350 item 3 (diagnostics), the clean-file half — see
/// [`native_document_diagnostics_show_parse_error`]'s doc for why each case
/// gets its own isolated session.
///
/// Uses [`wait_for_next_analysis_pass`], not [`diagnostics_for`]: the
/// `DiagnosticsPublisher` anti-downgrade rule (`publish_decision`, `None =>
/// … send: nonempty`) deliberately never sends a `publishDiagnostics` at all
/// for a file's first, clean set — "so a clean file never generates a
/// spurious empty publish" — so waiting for an explicit empty-array publish
/// would wait forever. `wait_for_next_analysis_pass` instead waits for the
/// background pass to *complete* and reports whatever was last published for
/// the uri (defaulting to empty when nothing ever was), which is exactly
/// "no diagnostic surfaced."
#[test]
fn native_document_diagnostics_stay_clean_when_valid() {
    const MAX_MESSAGES: u64 = 2000;

    let root = unique_tmp_dir("native-diagnostics-clean");
    std::fs::create_dir_all(&root).unwrap();
    let (mut child, mut stdin, mut stdout) = start_server_at(&root, Some("brink"));

    let clean_uri = format!("file://{}", root.join("clean.brink").display());
    did_open_native(&mut stdin, &clean_uri, NATIVE_BARTER);
    let clean_diags = wait_for_next_analysis_pass(&mut stdout, &clean_uri, MAX_MESSAGES);

    drop(stdin);
    drop(stdout);
    let _ = child.wait();
    std::fs::remove_dir_all(&root).unwrap();

    assert!(
        clean_diags.is_empty(),
        "a clean native file must publish no diagnostics: {clean_diags:?}"
    );
}

/// #2360: `textDocument/inlayHint` over a `.brink` document must route
/// through `inlay_hints_native` off `db.parse_native`, not silently come
/// back empty because `db.parse` (ink) cast-fails on every native node.
///
/// `damage`'s doc-tagged `@param weapon {int}` gives the parameter hint a
/// type suffix, so the label is exact and unambiguous: `"weapon: int"`.
#[test]
fn native_document_inlay_hints_route_through_native_cst() {
    const MAX_MESSAGES: u64 = 2000;

    let root = unique_tmp_dir("native-inlay-hints");
    std::fs::create_dir_all(&root).unwrap();

    let src = "\
/// @param weapon {int}
fn damage(weapon) {
  return weapon;
}
flow main() {
  ~ let x = damage(3)
}
";
    let (mut child, mut stdin, mut stdout) = start_server_at(&root, Some("brink"));
    let uri = format!("file://{}", root.join("main.brink").display());
    did_open_native(&mut stdin, &uri, src);
    let _ = wait_for_next_analysis_pass(&mut stdout, &uri, MAX_MESSAGES);

    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "textDocument/inlayHint",
            "params": {
                "textDocument": {"uri": uri},
                "range": {
                    "start": {"line": 0, "character": 0},
                    "end": {"line": 7, "character": 0},
                },
            },
        }),
    );
    let (resp, _) = recv_response(&mut stdout, 2);

    drop(stdin);
    drop(stdout);
    let _ = child.wait();
    std::fs::remove_dir_all(&root).unwrap();

    let hints = resp["result"].as_array().cloned().unwrap_or_default();
    let labels: Vec<String> = hints
        .iter()
        .filter_map(|h| match &h["label"] {
            Value::String(s) => Some(s.clone()),
            Value::Array(parts) => Some(
                parts
                    .iter()
                    .filter_map(|p| p["value"].as_str())
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect();

    assert!(
        labels.iter().any(|l| l.contains("weapon: int")),
        "a .brink call site's inlay hints must route through \
         inlay_hints_native (#2360), got: {resp:?}"
    );
}

/// #2360: `textDocument/codeAction` over a `.brink` document must never
/// offer an ink-only structural action (`Sort knots alphabetically`, …) —
/// `brink_ide::code_actions::code_actions` unconditionally ink-parses the
/// source, and this exact content (two `=== name ===`-shaped narrative
/// lines, out of alphabetical order) is real ink `Knot` syntax when
/// misparsed that way, so before the `is_native` gate this fixture reliably
/// produced a bogus "Sort knots alphabetically" quick-fix on a native file
/// that has no knots at all.
#[test]
fn native_document_code_action_never_offers_ink_only_knot_actions() {
    const MAX_MESSAGES: u64 = 2000;

    let root = unique_tmp_dir("native-code-action");
    std::fs::create_dir_all(&root).unwrap();

    // Parses cleanly (zero errors) under the *native* frontend regardless of
    // extension — the LSP admits it purely by `.brink` extension either way.
    // But it happens to be real, out-of-order ink `Knot` header syntax if
    // (mis)parsed with the ink grammar instead (verified directly against
    // `brink_ide::code_actions::code_actions`, which reliably offers "Sort
    // knots alphabetically" for this exact text before the `is_native` gate).
    let src = "=== zeta ===\n=== alpha ===\n";
    let (mut child, mut stdin, mut stdout) = start_server_at(&root, Some("brink"));
    let uri = format!("file://{}", root.join("main.brink").display());
    did_open_native(&mut stdin, &uri, src);
    let _ = wait_for_next_analysis_pass(&mut stdout, &uri, MAX_MESSAGES);

    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "textDocument/codeAction",
            "params": {
                "textDocument": {"uri": uri},
                "range": {
                    "start": {"line": 0, "character": 0},
                    "end": {"line": 0, "character": 0},
                },
                "context": {"diagnostics": []},
            },
        }),
    );
    let (resp, _) = recv_response(&mut stdout, 2);

    drop(stdin);
    drop(stdout);
    let _ = child.wait();
    std::fs::remove_dir_all(&root).unwrap();

    let actions = resp["result"].as_array().cloned().unwrap_or_default();
    let titles: Vec<String> = actions
        .iter()
        .filter_map(|a| a["title"].as_str().map(str::to_owned))
        .collect();

    assert!(
        !titles.iter().any(|t| t.contains("Sort knots")),
        "a .brink file must never be offered an ink-only 'Sort knots' quick-fix \
         (#2360), got: {titles:?}"
    );
}

/// #2360 (formatting): `brink_fmt::format` is ink-only — it unconditionally
/// ink-parses its input — so `textDocument/formatting` on a `.brink` document
/// must decline (`null`) rather than return edits computed from a misparse.
/// The fixture's indentation is deliberately shaped so the ink formatter
/// WOULD rewrite it (verified red before the `is_native` gate in
/// `Backend::formatting`: the pre-gate server answered with edits).
#[test]
fn native_document_formatting_declines_instead_of_ink_formatting() {
    const MAX_MESSAGES: u64 = 2000;

    let root = unique_tmp_dir("native-formatting");
    std::fs::create_dir_all(&root).unwrap();

    let src = "\
flow main() {
      ~ let x = 1
          ~ let y = 2
}
";
    let (mut child, mut stdin, mut stdout) = start_server_at(&root, Some("brink"));
    let uri = format!("file://{}", root.join("main.brink").display());
    did_open_native(&mut stdin, &uri, src);
    let _ = wait_for_next_analysis_pass(&mut stdout, &uri, MAX_MESSAGES);

    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "textDocument/formatting",
            "params": {
                "textDocument": {"uri": uri},
                "options": {"tabSize": 2, "insertSpaces": true},
            },
        }),
    );
    let (resp, _) = recv_response(&mut stdout, 2);

    drop(stdin);
    drop(stdout);
    let _ = child.wait();
    std::fs::remove_dir_all(&root).unwrap();

    assert!(
        resp["result"].is_null(),
        "formatting a .brink document must decline (null) until a native \
         formatter path exists (#2360), got: {resp:?}"
    );
}
