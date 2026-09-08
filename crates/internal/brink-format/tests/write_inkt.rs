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

#[test]
fn visibility_roundtrips_through_inkt() {
    use brink_format::{DefinitionId, DefinitionTag};

    let mut data = i001_data();
    let mut private_defs = vec![
        DefinitionId::new(DefinitionTag::GlobalVar, 3),
        DefinitionId::new(DefinitionTag::Address, 7),
    ];
    private_defs.sort_by_key(|id| id.to_raw());
    data.private_defs = private_defs.clone();

    let mut buf = String::new();
    brink_format::write_inkt(&data, &mut buf).unwrap();
    assert!(buf.contains("(visibility"), "inkt text: {buf}");

    let recovered = brink_format::read_inkt(&buf).unwrap();
    assert_eq!(recovered.private_defs, private_defs);
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

/// The `char_at(s, i)` stdlib-slice-1-completion opcode (issue #857)
/// round-trips through `.inkt` text. The corpus smoke tests below compile
/// under the default `Dialect::StrictInk` and silently skip any file that
/// doesn't compile under it — `char_at` is brink-dialect-gated, so a
/// dedicated `Dialect::Brink` compile is needed to actually exercise it.
#[test]
fn char_at_opcode_roundtrips_through_inkt() {
    use brink_compiler::{AnalysisOptions, Dialect};
    use brink_format::Opcode;

    let src = "~ temp c = char_at(\"hello\", 1)\n{c}\n-> END\n";
    let data = brink_compiler::compile_with_options(
        "main.ink",
        |_p| Ok(src.to_owned()),
        AnalysisOptions {
            dialect: Dialect::Brink,
            ..AnalysisOptions::default()
        },
    )
    .unwrap()
    .data;

    let has_char_at = data.containers.iter().any(|c| {
        let mut offset = 0;
        let mut found = false;
        while offset < c.bytecode.len() {
            let Ok(op) = Opcode::decode(&c.bytecode, &mut offset) else {
                break;
            };
            if matches!(op, Opcode::CharAt) {
                found = true;
            }
        }
        found
    });
    assert!(
        has_char_at,
        "expected a CharAt opcode in the lowered bytecode"
    );

    let mut buf = String::new();
    brink_format::write_inkt(&data, &mut buf).unwrap();
    assert!(buf.contains("char_at"), "inkt text: {buf}");

    let recovered = brink_format::read_inkt(&buf).unwrap();
    assert_eq!(data, recovered);
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

/// T1e (`docs/t1e-spec.md` §3): a `Projection`-valued global round-trips
/// through `.inkt` — both the declared `:projection` type keyword and the
/// `(projection <cell> (segments …))` value atom need matching
/// writer/reader grammar (the #742 asymmetry class, same lesson
/// `global_with_handle_default_roundtrips` proved for T1d).
#[test]
fn global_with_projection_default_roundtrips() {
    use brink_format::{
        DefinitionId, DefinitionTag, GlobalVarDef, NameId, ProjSegment, Value, ValueType,
    };

    let mut data = i001_data();
    let next_id = data.variables.len() as u64;
    let cell = DefinitionId::new(DefinitionTag::GlobalVar, 7);
    data.variables.push(GlobalVarDef {
        id: DefinitionId::new(DefinitionTag::GlobalVar, next_id),
        name: NameId(0),
        value_type: ValueType::Projection,
        default_value: Value::projection(
            cell,
            vec![
                ProjSegment::Key(Value::String("hp".into())),
                ProjSegment::Index(3),
            ],
        ),
        mutable: true,
        local: false,
    });

    let mut buf = String::new();
    brink_format::write_inkt(&data, &mut buf).unwrap();
    assert!(
        buf.contains("(segments (key \"hp\") (index 3))"),
        "dump:\n{buf}"
    );
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

/// #742: a `Record` default round-trips through `.inkt` text — the
/// `(record <shape> <field>…)` atom that `write_value` already emitted but
/// `read_inkt`'s `value` grammar rule had no matching alternative for (a
/// write/read asymmetry for the T1c value tags, `docs/format-v4-rfc.md`
/// §4). Also exercises the `:record` `type_name` on the enclosing
/// `global_entry`, which had the same gap.
#[test]
fn global_with_record_default_roundtrips() {
    use brink_format::{
        DefinitionId, DefinitionTag, GlobalVarDef, NameId, ShapeId, Value, ValueType,
    };

    let mut data = i001_data();
    let next_id = data.variables.len() as u64;
    data.variables.push(GlobalVarDef {
        id: DefinitionId::new(DefinitionTag::GlobalVar, next_id),
        name: NameId(0),
        value_type: ValueType::Record,
        default_value: Value::record(ShapeId(3), vec![Value::Int(10), Value::Bool(true)]),
        mutable: false,
        local: false,
    });

    let mut buf = String::new();
    brink_format::write_inkt(&data, &mut buf).unwrap();
    assert!(buf.contains("(record 3 10 true)"), "{buf}");

    let recovered = brink_format::read_inkt(&buf).unwrap();
    assert_eq!(recovered.variables, data.variables);
}

/// #742: a `FnRef` default (`(fn_ref <def_id>)`) round-trips. Also exercises
/// `:fn_ref` as a `global_entry` `type_name`.
#[test]
fn global_with_fn_ref_default_roundtrips() {
    use brink_format::{DefinitionId, DefinitionTag, GlobalVarDef, NameId, Value, ValueType};

    let mut data = i001_data();
    let target = DefinitionId::new(DefinitionTag::Address, 0x00AB_CDEF);
    let next_id = data.variables.len() as u64;
    data.variables.push(GlobalVarDef {
        id: DefinitionId::new(DefinitionTag::GlobalVar, next_id),
        name: NameId(0),
        value_type: ValueType::FnRef,
        default_value: Value::FnRef(target),
        mutable: false,
        local: false,
    });

    let mut buf = String::new();
    brink_format::write_inkt(&data, &mut buf).unwrap();
    assert!(buf.contains("(fn_ref $01_00000000abcdef)"), "{buf}");

    let recovered = brink_format::read_inkt(&buf).unwrap();
    assert_eq!(recovered.variables, data.variables);
}

/// M-3 (`docs/modules-spec.md` §5): the `AliasTable` section
/// (`(alias_table (alias <old> -> <new>)…)`) round-trips through `.inkt`
/// text.
#[test]
fn alias_table_roundtrips() {
    use brink_format::{AliasEntry, DefinitionId, DefinitionTag};

    let mut data = i001_data();
    data.alias_table = vec![
        AliasEntry {
            old: DefinitionId::new(DefinitionTag::Address, 0x01),
            new: DefinitionId::new(DefinitionTag::Address, 0x02),
        },
        AliasEntry {
            old: DefinitionId::new(DefinitionTag::GlobalVar, 0x03),
            new: DefinitionId::new(DefinitionTag::GlobalVar, 0x04),
        },
    ];

    let mut buf = String::new();
    brink_format::write_inkt(&data, &mut buf).unwrap();
    assert!(
        buf.contains("(alias $01_00000000000001 -> $01_00000000000002)"),
        "{buf}"
    );
    assert!(
        buf.contains("(alias $02_00000000000003 -> $02_00000000000004)"),
        "{buf}"
    );

    let recovered = brink_format::read_inkt(&buf).unwrap();
    assert_eq!(recovered.alias_table, data.alias_table);
}

/// T2-3 (`docs/effects-spec.md` §11): the `EffectRows` section round-trips
/// through `.inkt` text — direct part (reads/writes/call atoms/opaque) plus a
/// narrowable per-dispatch entry with a static-fallback row. Unlike
/// `struct_shapes`, this section is fully round-tripped (writer + reader land
/// together, the #742 lesson).
#[test]
fn effect_rows_roundtrips() {
    use brink_format::{
        CallAtom, CapabilityParam, DefinitionId, DefinitionTag, DirectEffects, DispatchEntry,
        EffectRowEntry, NameId,
    };

    let cell = |n| DefinitionId::new(DefinitionTag::GlobalVar, n);
    let mut data = i001_data();
    data.effect_rows = vec![EffectRowEntry {
        def: DefinitionId::new(DefinitionTag::Address, 0x01),
        is_entry: true,
        direct: DirectEffects {
            reads: vec![cell(1), cell(2)],
            writes: vec![cell(3)],
            calls: vec![CallAtom {
                name: NameId(5),
                capability: CapabilityParam::Any,
                handle_param: None,
            }],
            opaque: false,
            emits: false,
            tags: false,
            faults: false,
        },
        dispatches: vec![DispatchEntry {
            cell: cell(9),
            narrowable: true,
            fallback: DirectEffects {
                reads: vec![cell(4)],
                writes: vec![],
                calls: vec![],
                opaque: true,
                emits: false,
                tags: false,
                faults: false,
            },
        }],
    }];

    let mut buf = String::new();
    brink_format::write_inkt(&data, &mut buf).unwrap();
    assert!(buf.contains("(effect_rows"), "{buf}");
    assert!(buf.contains("(call 5 any)"), "{buf}");
    assert!(
        buf.contains("(dispatch $02_00000000000009 narrowable"),
        "{buf}"
    );
    // An entry row (`is_entry: true`, the default) never prints the `internal`
    // freeze-bit token (#882).
    assert!(!buf.contains("internal"), "{buf}");

    let recovered = brink_format::read_inkt(&buf).unwrap();
    assert_eq!(recovered.effect_rows, data.effect_rows);
}

/// #882: the freeze bit round-trips through `.inkt` — a `#@private` def's row
/// prints (and re-parses) `is_entry: false` via the `internal` token, while a
/// sibling public row's `is_entry: true` stays the default (no token). Both
/// rows remain present in the recovered table — `docs/effects-spec.md` §10 and
/// `docs/modules-spec.md` §4's first boundary rule both hold that private
/// hides the *name*, not the *cell* — so the private row's own effects are
/// still resolvable by `def`, proving the table (not just the entry set)
/// survives the round trip.
#[test]
fn effect_rows_internal_flag_roundtrips() {
    use brink_format::{
        CallAtom, CapabilityParam, DefinitionId, DefinitionTag, DirectEffects, EffectRowEntry,
        NameId,
    };

    let private_def = DefinitionId::new(DefinitionTag::Address, 0x01);
    let public_def = DefinitionId::new(DefinitionTag::Address, 0x02);
    let mut data = i001_data();
    data.effect_rows = vec![
        EffectRowEntry {
            def: private_def,
            is_entry: false,
            direct: DirectEffects {
                reads: vec![],
                writes: vec![],
                calls: vec![CallAtom {
                    name: NameId(5),
                    capability: CapabilityParam::Any,
                    handle_param: None,
                }],
                opaque: false,
                emits: false,
                tags: false,
                faults: false,
            },
            dispatches: vec![],
        },
        EffectRowEntry {
            def: public_def,
            is_entry: true,
            direct: DirectEffects {
                reads: vec![],
                writes: vec![],
                calls: vec![],
                opaque: false,
                emits: false,
                tags: false,
                faults: false,
            },
            dispatches: vec![],
        },
    ];

    let mut buf = String::new();
    brink_format::write_inkt(&data, &mut buf).unwrap();
    assert!(buf.contains("internal"), "{buf}");

    let recovered = brink_format::read_inkt(&buf).unwrap();
    assert_eq!(recovered.effect_rows, data.effect_rows);

    // The private row's own call atom is still resolvable via the table by
    // `def` — freezing it out of the entry set never drops the row.
    let private_row = recovered
        .effect_rows
        .iter()
        .find(|r| r.def == private_def)
        .expect("private def's row still resolvable via the table");
    assert!(!private_row.is_entry);
    assert_eq!(private_row.direct.calls.len(), 1);

    let public_row = recovered
        .effect_rows
        .iter()
        .find(|r| r.def == public_def)
        .expect("public def's row unaffected");
    assert!(public_row.is_entry);
}

/// #742: a `Closure` default (`(closure <def_id> (val|ref <name> <value>)…)`)
/// round-trips, including both `val` and `ref` env entries and a nested
/// collection payload. Also exercises `:closure` as a `global_entry`
/// `type_name`.
#[test]
fn global_with_closure_default_roundtrips() {
    use brink_format::{
        ClosureEnvEntry, DefinitionId, DefinitionTag, GlobalVarDef, NameId, Value, ValueType,
    };

    let mut data = i001_data();
    let target = DefinitionId::new(DefinitionTag::Address, 0x123);
    let ref_target = DefinitionId::new(DefinitionTag::GlobalVar, 0x456);
    let closure = Value::closure(
        target,
        vec![
            ClosureEnvEntry {
                name: NameId(1),
                is_ref: false,
                payload: Value::array(vec![Value::Int(1), Value::Int(2)]),
            },
            ClosureEnvEntry {
                name: NameId(2),
                is_ref: true,
                payload: Value::VariablePointer(ref_target),
            },
        ],
    );
    let next_id = data.variables.len() as u64;
    data.variables.push(GlobalVarDef {
        id: DefinitionId::new(DefinitionTag::GlobalVar, next_id),
        name: NameId(0),
        value_type: ValueType::Closure,
        default_value: closure,
        mutable: false,
        local: false,
    });

    let mut buf = String::new();
    brink_format::write_inkt(&data, &mut buf).unwrap();
    assert!(buf.contains("(closure $01_00000000000123"), "{buf}");
    assert!(buf.contains("(val 1 (array 1 2))"), "{buf}");
    assert!(
        buf.contains("(ref 2 (var_pointer $02_00000000000456))"),
        "{buf}"
    );

    let recovered = brink_format::read_inkt(&buf).unwrap();
    assert_eq!(recovered.variables, data.variables);
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

// Regression for #745: the fuzz matrix's very first CI run found that
// `read_inkt` accepted a `(params N …)` clause whose declared count `N`
// disagreed with the number of `(mode id)` metadata entries that followed —
// producing a `ContainerDef` violating its own documented invariant
// (`params.len()` always equals `param_count`, see `definition.rs`). That
// silently-inconsistent value then made `write_inkt`'s `(params {c.param_count}
// ...)` clause (gated on `param_count != 0`) drop the params metadata
// entirely on the next write when count said 0 but params was non-empty.
// The reader must reject the mismatch outright rather than construct
// invariant-violating data for downstream code to mishandle.
#[test]
fn params_count_mismatch_is_a_parse_error() {
    let inkt = r#"(story

  (name_table
    0 ""
  )

  (addresses
    (address $01_406ea523c53def -> $01_406ea523c53def +0)
  )

  (address_paths
    (path 0 -> $01_406ea523c53def)
  )

  (container $01_406ea523c53def
    (params 0 (val 0 0))
    (code
      done
    )
  )
)
"#;

    let err = brink_format::read_inkt(inkt).expect_err("count/metadata mismatch must not parse");
    assert!(
        err.message.contains("params metadata count"),
        "unexpected error: {err}"
    );
}

// Issue #985 (follow-up to #909): `OrderedMap`'s `Eq` is content-based and
// assumes each key appears at most once. A legitimate `write_inkt` never
// emits a duplicate `(map ...)` key — `OrderedMap::insert` de-duplicates on
// the write side — so a `(map (0 1) (0 2))` literal only ever arises from a
// hand-crafted or corrupted `.inkt` file. `read_inkt` must reject it with a
// parse error rather than silently keeping the last occurrence and handing
// callers an `OrderedMap` that violates the invariant its `Eq` relies on.
#[test]
fn duplicate_map_key_is_a_parse_error() {
    let inkt = r#"(story

  (name_table
    0 ""
  )

  (addresses
    (address $01_406ea523c53def -> $01_406ea523c53def +0)
  )

  (address_paths
    (path 0 -> $01_406ea523c53def)
  )

  (literal_pool
    (map (0 1) (0 2))
  )

  (container $01_406ea523c53def
    (code
      done
    )
  )
)
"#;

    let err = brink_format::read_inkt(inkt).expect_err("duplicate map key must not parse");
    assert!(
        err.message.contains("duplicate key in map value"),
        "unexpected error: {err}"
    );
}

// Fuzz-found #1102 (fuzz lane #672-C): a `.inkt` document declaring the same
// container address twice is malformed input. `read_inkt` used to admit it,
// and the poison surfaced downstream: `write_inkt` collapses line tables
// through a `scope_id`-keyed `HashMap`, so the later duplicate's lines
// silently replaced the earlier one's on the next write and the
// `inkt_write_read_roundtrip` fuzz harness aborted on the roundtrip
// mismatch. Same admission-check posture as the duplicate map key (#985):
// reject at read time with a graceful parse error, never a panic.
//
// The input below is the fuzz-minimized crasher from the issue, verbatim.
#[test]
fn regression_1102_duplicate_container_address() {
    let inkt = r#"(story checksum=0x8bd73265

  (name_table
    0 ""
  )

  (addresses
    (address $01_406ea523c53def -> $01_406ea523c53def +0)
  )

  (address_paths
    (path 0 -> $01_406ea523c53def)
  )

  (container $01_406ea523c53def )

  (container $01_406ea523c53def
    (name 0)
    (lines
      0 "Hello, world!" @626e7681b4e2e7bc (source "tests/tier1/basics/I001-minimal-story/story.ink" 0..14)
    )
    (code
      emit_line 0 0
      emit_newline
      done
    )
  )
)
"#;

    let err =
        brink_format::read_inkt(inkt).expect_err("duplicate container address must not parse");
    assert!(
        err.message.contains("duplicate container address"),
        "unexpected error: {err}"
    );
    assert!(
        err.message.contains("$01_406ea523c53def"),
        "error should name the duplicated address: {err}"
    );
    // The error points at the second `(container …)` declaration.
    assert_eq!(err.line, 17, "error should point at the duplicate: {err}");
}

// The duplicate-container check covers the whole class, not just the
// fuzz-found scope-owner shape: a child container (`(scope …)` field,
// `scope_id != id`) re-declaring an existing address is rejected too.
#[test]
fn regression_1102_duplicate_child_container_address() {
    let inkt = r"(story

  (addresses
    (address $01_406ea523c53def -> $01_406ea523c53def +0)
  )

  (container $01_406ea523c53def
    (code
      done
    )
  )

  (container $01_406ea523c53def
    (scope $01_0000000000beef)
    (code
      done
    )
  )
)
";

    let err =
        brink_format::read_inkt(inkt).expect_err("duplicate container address must not parse");
    assert!(
        err.message.contains("duplicate container address"),
        "unexpected error: {err}"
    );
}

// Sibling audit for #1102: duplicate `(address …)` and `(path …)` entries do
// NOT have the same abort shape — both sections are written back verbatim
// from their `Vec`s (no keyed-map collapse like the container line tables),
// so the roundtrip is lossless and the fuzz harness cannot trip on them.
// This test documents that audit result; it is not an endorsement of such
// documents as well-formed.
#[test]
fn duplicate_address_and_path_entries_roundtrip_losslessly() {
    let inkt = r"(story

  (addresses
    (address $01_406ea523c53def -> $01_406ea523c53def +0)
    (address $01_406ea523c53def -> $01_406ea523c53def +4)
  )

  (address_paths
    (path 0 -> $01_406ea523c53def)
    (path 0 -> $01_406ea523c53def)
  )

  (container $01_406ea523c53def
    (code
      done
    )
  )
)
";

    let story = brink_format::read_inkt(inkt).expect("duplicate address entries parse");
    assert_eq!(story.addresses.len(), 2);
    assert_eq!(story.address_paths.len(), 2);

    let mut buf = String::new();
    brink_format::write_inkt(&story, &mut buf).unwrap();
    let recovered = brink_format::read_inkt(&buf).expect("re-encoded .inkt parses");
    assert_eq!(story, recovered, "roundtrip must be lossless");
}

#[test]
fn distinct_map_keys_parse_cleanly() {
    let inkt = r#"(story

  (name_table
    0 ""
  )

  (addresses
    (address $01_406ea523c53def -> $01_406ea523c53def +0)
  )

  (address_paths
    (path 0 -> $01_406ea523c53def)
  )

  (literal_pool
    (map (0 1) (5 2))
  )

  (container $01_406ea523c53def
    (code
      done
    )
  )
)
"#;

    let data = brink_format::read_inkt(inkt).expect("distinct keys must parse");
    assert_eq!(data.literal_pool.len(), 1);
}

// ── FS-3 FrameShapes + invisible container flag through .inkt ────────────────

#[test]
fn frame_shapes_roundtrip_through_inkt() {
    use brink_format::{DefinitionId, DefinitionTag, FrameShapeDef, NameId};

    let mut data = i001_data();
    data.frame_shapes = vec![
        FrameShapeDef {
            site: DefinitionId::new(DefinitionTag::Address, 4),
            slots: vec![NameId(1), NameId(2)],
        },
        FrameShapeDef {
            site: DefinitionId::new(DefinitionTag::Address, 9),
            slots: vec![],
        },
    ];

    let mut buf = String::new();
    brink_format::write_inkt(&data, &mut buf).unwrap();
    assert!(
        buf.contains("(frame_shapes"),
        "dump carries the section:\n{buf}"
    );

    let recovered = brink_format::read_inkt(&buf).unwrap();
    assert_eq!(data.frame_shapes, recovered.frame_shapes);
    assert_eq!(data, recovered);
}

#[test]
fn invisible_container_flag_roundtrips_through_inkt() {
    use brink_format::CountingFlags;

    let mut data = i001_data();
    assert!(!data.containers.is_empty());
    data.containers[0].counting_flags |= CountingFlags::INVISIBLE;

    let mut buf = String::new();
    brink_format::write_inkt(&data, &mut buf).unwrap();
    assert!(buf.contains("invisible"), "dump names the flag:\n{buf}");

    let recovered = brink_format::read_inkt(&buf).unwrap();
    assert!(
        recovered.containers[0]
            .counting_flags
            .contains(CountingFlags::INVISIBLE)
    );
    assert_eq!(data, recovered);
}
