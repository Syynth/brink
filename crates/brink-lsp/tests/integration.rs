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
