#![cfg(feature = "inkt")]
#![allow(clippy::unwrap_used)]

use std::path::Path;

fn i001_data() -> brink_format::StoryData {
    // Compile from an in-memory string with a fixed entry name so the
    // embedded source path (and thus snapshots) stay machine-independent.
    let src = include_str!("../../../../tests/tier1/basics/I001-minimal-story/story.ink");
    brink_compiler::compile("story.ink", |_p| Ok(src.to_owned()))
        .unwrap()
        .data
}

#[test]
fn snapshot_i001_minimal_story() {
    let data = i001_data();

    let mut buf = String::new();
    brink_format::write_inkt(&data, &mut buf).unwrap();

    insta::assert_snapshot!(buf);
}

#[test]
fn roundtrip_i001_minimal_story() {
    let data = i001_data();

    let mut buf = String::new();
    brink_format::write_inkt(&data, &mut buf).unwrap();

    let recovered = brink_format::read_inkt(&buf).unwrap();
    assert_eq!(data, recovered);
}

/// The T1b literal pool round-trips through `.inkt` text, including nested
/// array/map entries — this also exercises the `array_value`/`map_value`
/// grammar rules added alongside it, which fix a pre-existing gap: `Value`
/// (used for `GlobalVarDef` defaults since #525) could already be an
/// `Array`/`Map` and `write_value` already rendered it as `(array …)`/
/// `(map …)`, but `read_inkt`'s `value` grammar rule had no matching
/// alternative — a written array/map value could not be parsed back.
#[test]
fn literal_pool_roundtrip_with_collections() {
    use brink_format::{MapKey, OrderedMap, Value};

    let mut data = i001_data();
    let mut inner_map = OrderedMap::new();
    inner_map.insert(MapKey::from("hp"), Value::Int(10));
    inner_map.insert(MapKey::from(true), Value::String("flag".into()));
    inner_map.insert(MapKey::from(7), Value::Float(1.5));
    data.literal_pool = vec![
        Value::array(vec![Value::Int(1), Value::Int(2), Value::Int(3)]),
        Value::map(inner_map),
        Value::array(vec![Value::array(vec![]), Value::Null, Value::Bool(true)]),
    ];

    let mut buf = String::new();
    brink_format::write_inkt(&data, &mut buf).unwrap();
    assert!(buf.contains("literal_pool"), "{buf}");

    let recovered = brink_format::read_inkt(&buf).unwrap();
    assert_eq!(recovered.literal_pool, data.literal_pool);
}

/// A global variable with an `Array`/`Map` default also round-trips through
/// `.inkt` — the general case the literal-pool test above's grammar fix
/// unblocks (see that test's doc).
#[test]
fn global_with_collection_default_roundtrips() {
    use brink_format::{
        DefinitionId, DefinitionTag, GlobalVarDef, MapKey, NameId, OrderedMap, Value, ValueType,
    };

    let mut data = i001_data();
    let mut m = OrderedMap::new();
    m.insert(MapKey::from("a"), Value::array(vec![Value::Int(1)]));
    let next_id = data.variables.len() as u64;
    data.variables.push(GlobalVarDef {
        id: DefinitionId::new(DefinitionTag::GlobalVar, next_id),
        name: NameId(0),
        value_type: ValueType::Map,
        default_value: Value::map(m),
        mutable: true,
        local: false,
    });

    let mut buf = String::new();
    brink_format::write_inkt(&data, &mut buf).unwrap();
    let recovered = brink_format::read_inkt(&buf).unwrap();
    assert_eq!(recovered.variables, data.variables);
}

/// T1d (`docs/t1d-spec.md` §2): a `Handle`-valued global round-trips through
/// `.inkt` — both the declared `:handle` type keyword and the `(handle …)`
/// value atom need matching writer/reader grammar (the #742 asymmetry class
/// this PR must not repeat for its own new atom). This test would have
/// caught the grammar gap this PR's implementation initially hit: the pest
/// `type_name` rule not listing `"handle"` as a valid declared-type keyword.
#[test]
fn global_with_handle_default_roundtrips() {
    use brink_format::{DefinitionId, DefinitionTag, GlobalVarDef, NameId, Value, ValueType};

    let mut data = i001_data();
    let next_id = data.variables.len() as u64;
    data.variables.push(GlobalVarDef {
        id: DefinitionId::new(DefinitionTag::GlobalVar, next_id),
        name: NameId(0),
        value_type: ValueType::Handle,
        default_value: Value::handle(NameId(3), 42),
        mutable: true,
        local: false,
    });

    let mut buf = String::new();
    brink_format::write_inkt(&data, &mut buf).unwrap();
    assert!(buf.contains("(handle 3 42)"), "dump:\n{buf}");
    let recovered = brink_format::read_inkt(&buf).unwrap();
    assert_eq!(recovered.variables, data.variables);
}

/// #673: a `VAR`/`CONST` declaration default that's a collection literal
/// (`#[…]`/`#{…}`) used to constant-fold to `Value::Null` with no
/// diagnostic (`eval_const_expr` had no `ArrayLiteral`/`MapLiteral` arm).
/// Compiles real brink-dialect source through the actual compiler entry
/// point (not a hand-built `StoryData`, unlike `global_with_collection_
/// default_roundtrips` above, which only proves the format layer can
/// *represent* a collection default — this proves the compiler actually
/// *produces* one from a literal-only default) and inspects the `.inkt`
/// text dump: the fixed default must render as the real collection
/// (`(array 1 2 3)`/`(map ("a" 1))`), never `null`.
#[test]
fn collection_literal_declaration_default_compiles_to_a_real_value_not_null() {
    let src = "VAR arr = #[1, 2, 3]\nVAR m = #{\"a\": 1}\nHello.\n-> END\n";
    let options = brink_compiler::AnalysisOptions {
        dialect: brink_compiler::Dialect::Brink,
        ..brink_compiler::AnalysisOptions::default()
    };
    let output = brink_compiler::compile_with_options("main.ink", |_p| Ok(src.to_owned()), options)
        .expect("brink-dialect collection literal defaults must compile cleanly");

    let mut buf = String::new();
    brink_format::write_inkt(&output.data, &mut buf).unwrap();

    assert!(
        buf.contains("(array 1 2 3)"),
        "expected the real array default in .inkt output, got:\n{buf}"
    );
    assert!(
        buf.contains("(map (\"a\" 1))"),
        "expected the real map default in .inkt output, got:\n{buf}"
    );
    assert!(
        !buf.contains("null"),
        ".inkt output must not contain a silently-dropped `null` default:\n{buf}"
    );
}

fn collect_story_ink_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    if dir.is_dir() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                files.extend(collect_story_ink_files(&path));
            } else if path.file_name().is_some_and(|n| n == "story.ink") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

#[test]
fn write_inkt_corpus_smoke() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tests_dir = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests");

    let files = collect_story_ink_files(&tests_dir);
    assert!(
        !files.is_empty(),
        "no story.ink files found in {tests_dir:?}"
    );

    let mut failures = Vec::new();

    for path in &files {
        // Some corpus stories intentionally do not compile — skip those.
        let Ok(output) = brink_compiler::compile_path(path) else {
            continue;
        };
        let data = output.data;

        let mut buf = String::new();
        if let Err(e) = brink_format::write_inkt(&data, &mut buf) {
            failures.push(format!("WRITE_INKT {}: {e}", path.display()));
        }
    }

    assert!(
        failures.is_empty(),
        "{}/{} files failed write_inkt:\n{}",
        failures.len(),
        files.len(),
        failures.join("\n")
    );
}

#[test]
fn inkt_roundtrip_corpus_smoke() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tests_dir = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests");

    let files = collect_story_ink_files(&tests_dir);
    assert!(
        !files.is_empty(),
        "no story.ink files found in {tests_dir:?}"
    );

    let mut failures = Vec::new();

    for path in &files {
        // Some corpus stories intentionally do not compile — skip those.
        let Ok(output) = brink_compiler::compile_path(path) else {
            continue;
        };
        let data = output.data;

        let mut buf = String::new();
        if brink_format::write_inkt(&data, &mut buf).is_err() {
            continue;
        }

        match brink_format::read_inkt(&buf) {
            Ok(recovered) => {
                if data != recovered {
                    failures.push(format!("MISMATCH {}", path.display()));
                }
            }
            Err(e) => {
                failures.push(format!("PARSE {}: {e}", path.display()));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{}/{} files failed inkt roundtrip:\n{}",
        failures.len(),
        files.len(),
        failures.join("\n")
    );
}
