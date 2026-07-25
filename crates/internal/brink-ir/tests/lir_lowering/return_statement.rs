use crate::support::*;
use brink_ir::lir;
use brink_ir::{FileId, HirFile};

// ─── Return statement ───────────────────────────────────────────────

#[test]
fn return_from_function() {
    let p = lower_ink(
        "\
== function double(x) ==
~ return x * 2
",
    );
    let knot = find_child(&p.root, "double");
    let has_return = knot
        .body
        .iter()
        .any(|s| matches!(s, lir::Stmt::Return { value: Some(_), .. }));
    assert!(has_return, "function should have a Return statement");
}

/// B0.2 (contract D5 / F-I#6): tunnel-vs-explicit classification reads the
/// explicit `ReturnKind`, never `ptr` presence. A tunnel return that
/// *carries* provenance — no ink surface syntax produces this shape, but a
/// uniformly provenance-stamping frontend (native) will — must still emit
/// no `E032` and still lower to LIR `is_tunnel: true`.
#[test]
fn provenance_carrying_tunnel_return_still_lowers_as_tunnel() {
    let source = "\
== main ==
-> tun ->
-> END

== tun ==
Hello.
->->
";
    let parsed = brink_syntax::parse(source);
    let tree = parsed.tree();
    let file_id = FileId(0);
    let (mut hir, manifest, _diags) = brink_ir::hir::lower(file_id, &tree);

    // Stamp provenance onto the tunnel return, simulating a frontend that
    // attaches provenance uniformly. The kind must keep it a tunnel.
    let tun = hir
        .knots
        .iter_mut()
        .find(|k| k.name.text == "tun")
        .expect("knot `tun` exists");
    let mut stamped = 0;
    for stmt in &mut tun.body.stmts {
        if let brink_ir::Stmt::Return(ret) = stmt {
            assert_eq!(ret.kind, brink_ir::ReturnKind::TunnelRedirect);
            assert!(
                ret.ptr.is_none(),
                "ink lowering attaches no provenance to tunnel returns today"
            );
            ret.ptr = Some(brink_ir::Provenance::synthetic(
                brink_ir::NodeClass::Return,
                rowan::TextRange::default(),
            ));
            stamped += 1;
        }
    }
    assert_eq!(stamped, 1, "expected exactly one tunnel Return in `tun`");

    brink_ir::hir::normalize_file(&mut hir);

    let files_for_analysis: Vec<(FileId, &HirFile, &brink_ir::SymbolManifest)> =
        vec![(file_id, &hir, &manifest)];
    let result = brink_analyzer::analyze(&files_for_analysis);
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.code == brink_ir::DiagnosticCode::E032),
        "a provenance-carrying tunnel return must not trip E032: {:?}",
        result.diagnostics
    );

    let files_for_lir: Vec<(FileId, &HirFile)> = vec![(file_id, &hir)];
    let (program, _warnings) = lir::lower_to_program(
        &files_for_lir,
        &result.index,
        &result.resolutions,
        &std::collections::HashMap::new(),
    );
    let program = program.unwrap();
    let tun = find_by_path(&program, "tun");
    let has_tunnel_return = tun.body.iter().any(|s| {
        matches!(
            s,
            lir::Stmt::Return {
                is_tunnel: true,
                ..
            }
        )
    });
    assert!(
        has_tunnel_return,
        "provenance-carrying tunnel return must still classify as is_tunnel"
    );
}
