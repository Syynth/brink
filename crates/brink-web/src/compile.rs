use std::collections::{BTreeMap, HashMap};

use serde::Serialize;
use wasm_bindgen::prelude::*;

use brink_environment::{OptionOverrides, Project};
use brink_source_tree::InMemory;

use crate::editor_dto::diagnostic_to_js;

// ── Compilation ─────────────────────────────────────────────────────

/// Failure from [`compile_over_tree`]: either loading the [`Project`] (a bad
/// `SourceTree` read, a malformed `brink.toml`) or the compile itself. Kept
/// distinct from `brink_compiler::CompileError` — rather than laundering a
/// [`brink_environment::LoadError`] through `CompileError::Io`'s `"I/O
/// error: "` prefix (which doubled up on `LoadError::Discover`'s own
/// `"discovery error: "` / `"I/O error: "` prefixes and mislabelled a
/// `LoadError::Config`/`ConfigRead` as I/O) — so [`compile_result_json`] can
/// surface each variant's own `Display` message verbatim.
#[derive(Debug, thiserror::Error)]
enum LoadOrCompileError {
    /// [`Project::load`] failed before compilation ever started.
    #[error(transparent)]
    Load(#[from] brink_environment::LoadError),
    /// [`Project::load`] succeeded but [`brink_environment::compile`] failed.
    #[error(transparent)]
    Compile(#[from] brink_compiler::CompileError),
}

/// Build a [`CompileResult`] JSON string from an already-produced compile
/// result, resolving each diagnostic's byte offsets against `source_of` (the
/// text the diagnostic's `path` was actually parsed from — a single-file
/// compile always resolves the same source; a multi-file one, e.g.
/// [`compile_fragment`], looks the path up per diagnostic) and its rendered
/// severity against `options` — the [`brink_analyzer::AnalysisOptions`]
/// [`compile_over_tree`] actually resolved the compile under, so a
/// `[lints]`-re-leveled or `types = strict`-promoted code renders at the same
/// effective severity it was gated on, not the code's raw default (issue
/// #1367).
fn compile_result_json(
    options: &brink_analyzer::AnalysisOptions,
    result: Result<brink_compiler::CompileOutput, LoadOrCompileError>,
    source_of: impl Fn(&str) -> String,
) -> String {
    match result {
        Ok(output) => {
            let warnings: Vec<DiagnosticJs> = output
                .warnings
                .iter()
                .map(|d| {
                    diagnostic_to_js(
                        d,
                        &source_of(&d.path),
                        options.type_policy(),
                        &options.lints,
                    )
                })
                .collect();

            let mut bytes = Vec::new();
            brink_format::write_inkb(&output.data, &mut bytes);

            let resp = CompileResult {
                ok: true,
                story_bytes: Some(bytes),
                warnings,
                error: None,
            };
            serde_json::to_string(&resp).unwrap_or_default()
        }
        Err(e) => {
            let mut diagnostics = Vec::new();
            let mut error_msg = None;

            match e {
                LoadOrCompileError::Compile(brink_compiler::CompileError::Diagnostics(diags)) => {
                    diagnostics = diags
                        .iter()
                        .map(|d| {
                            diagnostic_to_js(
                                d,
                                &source_of(&d.path),
                                options.type_policy(),
                                &options.lints,
                            )
                        })
                        .collect();
                }
                other => {
                    error_msg = Some(format!("{other}"));
                }
            }

            let resp = CompileResult {
                ok: false,
                story_bytes: None,
                warnings: diagnostics,
                error: error_msg,
            };
            serde_json::to_string(&resp).unwrap_or_default()
        }
    }
}

/// Compile `entry` over `tree` through the #1306 producer:
/// `Project::load(&tree, entry, &overrides)` -> `compile(&env)`. Replaces the
/// throwaway `Driver` + `set_file`/`set_entry`/`set_analysis_options`
/// imperative path — `Project::load`'s own `brink.toml` discovery + override
/// precedence resolves the effective `AnalysisOptions`: the `overrides` here
/// are always the default (never a registered host manifest), but the
/// dialect/types/`[lints]` values themselves come from a `brink.toml`
/// discovered on `tree`, if one is served.
///
/// A [`brink_environment::LoadError`] (a bad `SourceTree` read, a malformed
/// `brink.toml`) is reported through the same `error` string field a
/// non-diagnostics [`brink_compiler::CompileError`] uses — the caller only
/// ever sees the one `CompileResult` JSON shape — but as its own `Display`
/// message, not laundered through `CompileError::Io`.
///
/// Also returns the resolved [`brink_analyzer::AnalysisOptions`] alongside
/// the compile result, so the caller can render diagnostics at the effective
/// severity the compile actually ran under (issue #1367) even on the
/// `CompileError::Diagnostics` failure path. Falls back to
/// `AnalysisOptions::default()` when `Project::load` itself fails, before any
/// options are resolved — that `Load` error variant never carries
/// diagnostics, so the fallback is never actually rendered through.
fn compile_over_tree(
    tree: &InMemory,
    entry: &str,
) -> (
    brink_analyzer::AnalysisOptions,
    Result<brink_compiler::CompileOutput, LoadOrCompileError>,
) {
    let overrides = OptionOverrides::default();
    match Project::load(tree, entry, &overrides) {
        Ok(env) => {
            let options = env.options.clone();
            let result = brink_environment::compile(&env).map_err(LoadOrCompileError::from);
            (options, result)
        }
        Err(e) => (
            brink_analyzer::AnalysisOptions::default(),
            Err(LoadOrCompileError::from(e)),
        ),
    }
}

/// Compile ink source and return JSON with diagnostics or story data.
#[wasm_bindgen]
pub fn compile(source: &str) -> String {
    let tree = InMemory::new(BTreeMap::from([(
        "main.ink".to_string(),
        source.to_owned(),
    )]));
    let (options, result) = compile_over_tree(&tree, "main.ink");
    compile_result_json(&options, result, |_path| source.to_owned())
}

/// The source-identity checksum of compiled `.inkb` bytes, formatted as
/// `0x{:08x}` — identical to `ProgramModel.checksum`. Lets the studio compare a
/// running program's identity against its latest compile *without* constructing
/// a `StoryRunner` (live-inspector degraded mode, #181 / spec §5): a mismatch
/// means the running program is not the studio's current source.
#[wasm_bindgen]
pub fn program_checksum(story_bytes: &[u8]) -> Result<String, JsError> {
    let data = brink_format::read_inkb(story_bytes)
        .map_err(|e| JsError::new(&format!("decode error: {e}")))?;
    Ok(format!("0x{:08x}", data.source_checksum))
}

/// Compile a project's sources plus one synthetic knot/function appended to
/// the entry file — Tier-1 speculative-eval fragment compilation (F5.1,
/// `docs/speculative-eval-spec.md`'s "mechanism B"): the web layer wraps an
/// arbitrary author-typed fragment as a synthetic symbol, then recompiles the
/// whole project with it via this entrypoint so the fragment resolves
/// against the live project's real symbols (globals, knots, lists, …), not
/// just the single string `compile()` serves.
///
/// `sources_json` is `{ "path": "content", ... }` for every file the running
/// program was last compiled from (an `INCLUDE`d path resolves against these
/// keys exactly as `compile_project` resolves against a live `EditorSession`
/// — the only difference is the file set is caller-supplied JSON instead of a
/// stateful session, since a `StoryRunner` keeps no reference to the project
/// that produced it). `entry` must be one of `sources_json`'s keys.
/// `synthetic_source` — already wrapped by the caller as
/// `=== function NAME() ===\n~ return (...)\n` or `=== NAME ===\n...\n` — is
/// appended to the entry file's served content before compiling; nothing
/// else about `entry`'s real content changes, and every other file is served
/// verbatim.
///
/// Returns the same JSON `CompileResult` shape as `compile()`/
/// `compile_project()`: `story_bytes` on success, `warnings`/`error` on
/// failure. A fragment that fails to compile (an unresolved name, a syntax
/// error) surfaces here as ordinary diagnostics, never a panic — the caller
/// tries the expression wrap first and falls back to the content wrap (or
/// vice versa) by calling this twice with different `synthetic_source`s.
///
/// Unlike `compile_project`, this always passes default `OptionOverrides` —
/// no registered host manifest — so a fragment compile never gets a
/// manifest-driven external type/arity/domain diagnostic. The dialect/types/
/// `[lints]` values themselves are still resolved from a `brink.toml`
/// discovered on `sources_json`, exactly as `compile_project` would resolve
/// them, so a served `[lints] deny-warnings = true` (or a deny-leveled code)
/// makes a fragment compile *stricter*, not more lenient — the only
/// leniency guarantee is the absent host manifest. This is benign: the
/// manifest is tooling/author-time-only and never affects codegen or binding
/// liveness, so the recompiled program runs identically; the fragment simply
/// skips an author-time check that has no bearing on a side-effect-proof
/// speculative run.
#[wasm_bindgen]
pub fn compile_fragment(entry: &str, sources_json: &str, synthetic_source: &str) -> String {
    let sources: HashMap<String, String> = match serde_json::from_str(sources_json) {
        Ok(s) => s,
        Err(e) => {
            let resp = CompileResult {
                ok: false,
                story_bytes: None,
                warnings: Vec::new(),
                error: Some(format!("sources decode error: {e}")),
            };
            return serde_json::to_string(&resp).unwrap_or_default();
        }
    };

    // The text actually served for `path`: the entry file gets the synthetic
    // symbol appended, every other (`INCLUDE`d) file is served verbatim. A
    // `BTreeMap` (not the deserialized `HashMap`) both backs the `InMemory`
    // tree `Project::load` walks and resolves each diagnostic's byte offsets
    // below, so a diagnostic inside the appended fragment resolves against
    // the exact text it was parsed from.
    let served: BTreeMap<String, String> = sources
        .iter()
        .map(|(path, src)| {
            let content = if path == entry {
                format!("{src}\n\n{synthetic_source}\n")
            } else {
                src.clone()
            };
            (path.clone(), content)
        })
        .collect();

    let tree = InMemory::new(served.clone());
    let (options, result) = compile_over_tree(&tree, entry);
    compile_result_json(&options, result, |path| {
        served.get(path).cloned().unwrap_or_default()
    })
}

#[derive(Serialize)]
pub(crate) struct CompileResult {
    pub(crate) ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) story_bytes: Option<Vec<u8>>,
    pub(crate) warnings: Vec<DiagnosticJs>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct DiagnosticJs {
    pub(crate) message: String,
    pub(crate) start: u32,
    pub(crate) end: u32,
    pub(crate) severity: String,
    /// Structured diagnostic code (e.g. `"E065"`), so consumers can filter or
    /// group diagnostics programmatically rather than string-matching the
    /// human-readable `message` (issue #1004: the compile/warnings channel
    /// previously carried `severity` but not the code itself).
    pub(crate) code: String,
    /// Path of the file this diagnostic belongs to. In a multi-file project a
    /// diagnostic may live in an included file rather than the entry, so the
    /// editor uses this to place it on the right tab.
    pub(crate) file: String,
}

// ── Tier-1 fragment compilation (F5.1) ───────────────────────────────
//
// `compile_fragment` is the "mechanism B" primitive: recompile the whole
// project plus a synthetic knot/function appended to the entry file. These
// tests exercise it directly (no wasm32 needed — it's plain Rust behind
// `#[wasm_bindgen]`), covering multi-file `INCLUDE` resolution, both wrap
// shapes, and the no-panic-on-garbage-input guarantee.
#[cfg(test)]
mod compile_fragment_tests {
    use super::compile_fragment;
    use std::collections::HashMap;

    fn sources_json(pairs: &[(&str, &str)]) -> String {
        let map: HashMap<&str, &str> = pairs.iter().copied().collect();
        serde_json::to_string(&map).expect("serialize sources")
    }

    #[test]
    fn expression_wrap_resolves_against_project_globals() {
        let src = sources_json(&[(
            "main.ink",
            "VAR gold = 5\n-> start\n\n=== start ===\nHi.\n-> END\n",
        )]);
        let synthetic = "=== function __eval_test() ===\n~ return (gold + 1)\n";
        let json = compile_fragment("main.ink", &src, synthetic);
        let v: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(v["ok"], true, "{json}");
        assert!(v["story_bytes"].is_array(), "{json}");
    }

    #[test]
    fn content_wrap_resolves_a_divert_into_an_included_file() {
        let src = sources_json(&[
            (
                "main.ink",
                "INCLUDE other.ink\n-> start\n\n=== start ===\nHi.\n-> END\n",
            ),
            ("other.ink", "=== helper ===\nHelper text.\n-> END\n"),
        ]);
        let synthetic = "=== __eval_test ===\n-> helper\n";
        let json = compile_fragment("main.ink", &src, synthetic);
        let v: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(v["ok"], true, "{json}");
    }

    #[test]
    fn content_wrap_interpolates_a_live_global() {
        let src = sources_json(&[(
            "main.ink",
            "VAR gold = 5\n-> start\n\n=== start ===\nHi.\n-> END\n",
        )]);
        let synthetic = "=== __eval_test ===\nYou have {gold} gold.\n";
        let json = compile_fragment("main.ink", &src, synthetic);
        let v: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(v["ok"], true, "{json}");
    }

    #[test]
    fn unresolved_name_is_a_diagnostic_not_a_panic() {
        let src = sources_json(&[("main.ink", "-> start\n\n=== start ===\nHi.\n-> END\n")]);
        let synthetic = "=== __eval_test ===\nYou have {nonexistent} gold.\n";
        let json = compile_fragment("main.ink", &src, synthetic);
        let v: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(v["ok"], false, "{json}");
        assert!(
            v["warnings"].as_array().is_some_and(|w| !w.is_empty()),
            "unresolved name surfaces as a diagnostic: {json}"
        );
    }

    #[test]
    fn missing_entry_file_reports_an_error_not_a_panic() {
        let src = sources_json(&[("main.ink", "-> END\n")]);
        let json = compile_fragment("nope.ink", &src, "=== __eval_test ===\nHi.\n");
        let v: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(v["ok"], false, "{json}");
    }

    #[test]
    fn malformed_sources_json_reports_an_error_not_a_panic() {
        let json = compile_fragment("main.ink", "not json", "=== __eval_test ===\nHi.\n");
        let v: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(v["ok"], false, "{json}");
    }

    #[test]
    fn served_brink_toml_lints_deny_relevels_e014_and_blocks_fragment_compile() {
        // Precedent: brink-environment's
        // `brink_toml_lints_deny_relevels_e014_and_blocks_compile`. A
        // `brink.toml` served alongside the project's sources is discovered
        // by `compile_over_tree`'s `Project::load` exactly as
        // `compile_project` would discover it — `[lints] E014 = "deny"`
        // must re-level a bare `~` line's E014 to Error and block the
        // fragment compile, not just the editor's project compile.
        let src = sources_json(&[
            ("brink.toml", "[lints]\nE014 = \"deny\"\n"),
            (
                "main.ink",
                "Hello.\n~\n-> start\n\n=== start ===\nHi.\n-> END\n",
            ),
        ]);
        let synthetic = "=== function __eval_test() ===\n~ return 1\n";
        let json = compile_fragment("main.ink", &src, synthetic);
        let v: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(v["ok"], false, "{json}");
        assert!(
            v["warnings"]
                .as_array()
                .is_some_and(|w| w.iter().any(|d| d["code"] == "E014")),
            "expected E014 among the surfaced diagnostics: {json}"
        );
    }

    // Issue #1387 (1/3): `compile_over_tree`/`Project::load` dispatches on
    // `entry`'s extension (`brink_environment`'s `collect_sources` doc) —
    // every test above exercises the `.ink` branch only. `compile_fragment`
    // itself is dialect-agnostic (it just appends `synthetic_source` to
    // `entry`'s served content and recompiles), so a `.brink` native entry
    // should work identically; these two pin that it actually does, using
    // native `fn`/`flow` syntax rather than ink's `=== ===` knot syntax.
    //
    // Caveat: this pins `compile_fragment` itself, not a reachable native
    // embedder path. `compile_fragment` is not re-exported from
    // `@brink-lang/web`; its only caller is `packages/wasm/src/index.ts`'s
    // `StoryRunnerHandle.compileFragment`, which hardcodes ink-only wraps
    // (`=== function NAME() ===` / `=== NAME ===`) — appended to a `.brink`
    // entry those are native parse errors. So `evaluate()`'s Tier-1 path
    // cannot succeed for a native project today; these tests prove the
    // primitive is dialect-agnostic, not that a native embedder can reach it
    // (see issue #1387 follow-up for wiring `compileFragment` to native
    // wrap syntax).

    #[test]
    fn native_entry_expression_wrap_resolves_against_project_globals() {
        let src = sources_json(&[(
            "main.brink",
            "var gold = 5\n\nflow main() {\n  Hi. -> END\n}\n",
        )]);
        let synthetic = "fn __eval_test() {\n  return gold + 1;\n}\n";
        let json = compile_fragment("main.brink", &src, synthetic);
        let v: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(v["ok"], true, "{json}");
        assert!(v["story_bytes"].is_array(), "{json}");
    }

    #[test]
    fn native_entry_content_wrap_interpolates_a_live_global() {
        let src = sources_json(&[(
            "main.brink",
            "var gold = 5\n\nflow main() {\n  Hi. -> END\n}\n",
        )]);
        let synthetic = "flow __eval_test() {\n  You have {gold} gold.\n}\n";
        let json = compile_fragment("main.brink", &src, synthetic);
        let v: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(v["ok"], true, "{json}");
        assert!(v["story_bytes"].is_array(), "{json}");
    }

    #[test]
    fn malformed_served_brink_toml_reports_an_error_not_a_panic() {
        // A discovered-but-unparsable `brink.toml` (bad `dialect` value) is
        // a `brink_environment::LoadError::Config`, surfaced through
        // `compile_over_tree`'s `LoadOrCompileError::Load` — must be a
        // populated `error` string, never a panic, and never a
        // `story_bytes`/diagnostics result.
        let src = sources_json(&[
            ("brink.toml", "[project]\ndialect = \"sideways\"\n"),
            ("main.ink", "-> start\n\n=== start ===\nHi.\n-> END\n"),
        ]);
        let synthetic = "=== __eval_test ===\nHi.\n";
        let json = compile_fragment("main.ink", &src, synthetic);
        let v: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(v["ok"], false, "{json}");
        assert!(v["story_bytes"].is_null(), "{json}");
        assert!(
            v["error"].as_str().is_some_and(|e| !e.is_empty()),
            "expected a populated error string: {json}"
        );
    }
}

// ── Tier-1 fragment evaluation wasm tests (F5.1) ──────────────────────
//
// Exercise the full "mechanism B" composition end-to-end through the real
// exported API, exactly as `packages/wasm/src/index.ts`'s `evaluate()`
// composes it: `compile_fragment` (recompile project + synthetic symbol) →
// `StoryRunner::new` (fresh runner over the recompiled program) → `load`
// (seed from the live runner's `save()`) → `speculate` → `eval_function` /
// `go_to_path`+`advance`. Proves the fragment sees live state, the live
// runner is untouched, and both wrap shapes (expression / content) work.
#[cfg(all(test, target_arch = "wasm32"))]
mod tier1_fragment_wasm_tests {
    use super::compile_fragment;
    use crate::story_runner::StoryRunner;
    use wasm_bindgen::JsValue;
    use wasm_bindgen_test::wasm_bindgen_test;

    const SRC: &str = "VAR gold = 0\n-> start\n\n=== start ===\nHi.\n-> END\n";

    fn bytes(src: &str) -> Vec<u8> {
        let out = brink_compiler::compile("main.ink", |_path| Ok(src.to_owned()))
            .expect("test source compiles");
        let mut b = Vec::new();
        brink_format::write_inkb(&out.data, &mut b);
        b
    }

    fn runner(src: &str) -> StoryRunner {
        StoryRunner::new(&bytes(src))
            .ok()
            .expect("runner constructs")
    }

    fn sources_json(src: &str) -> String {
        serde_json::to_string(&serde_json::json!({ "main.ink": src })).expect("json")
    }

    #[wasm_bindgen_test]
    fn expression_fragment_sees_live_state_and_never_mutates_the_live_runner() {
        let live = runner(SRC);
        assert!(live.set_var("gold", &JsValue::from_f64(7.0)));

        let compiled = compile_fragment(
            "main.ink",
            &sources_json(SRC),
            "=== function __eval_test() ===\n~ return (gold + 1)\n",
        );
        let compiled: serde_json::Value = serde_json::from_str(&compiled).expect("valid json");
        assert_eq!(compiled["ok"], true, "{compiled}");
        let story_bytes: Vec<u8> = compiled["story_bytes"]
            .as_array()
            .expect("story_bytes array")
            .iter()
            .map(|b| b.as_u64().expect("byte") as u8)
            .collect();

        let fragment_runner = StoryRunner::new(&story_bytes).ok().expect("constructs");
        let save = live.save().ok().expect("save");
        fragment_runner.load(&save).ok().expect("load");

        let spec = fragment_runner.speculate("{}").ok().expect("speculate");
        let json = spec
            .eval_function("__eval_test", Vec::new())
            .ok()
            .expect("eval_function");
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(value["type"], "returned");
        assert_eq!(value["value"]["type"], "int");
        assert_eq!(
            value["value"]["value"], 8,
            "sees the live gold=7, +1; {json}"
        );

        // The live runner's own state is untouched by any of the above.
        let live_gold = live.get_var("gold");
        assert_eq!(live_gold.as_f64(), Some(7.0));
    }

    #[wasm_bindgen_test]
    fn content_fragment_produces_a_transcript_reflecting_live_state() {
        let live = runner(SRC);
        assert!(live.set_var("gold", &JsValue::from_f64(3.0)));

        let compiled = compile_fragment(
            "main.ink",
            &sources_json(SRC),
            "=== __eval_test ===\nYou have {gold} gold.\n",
        );
        let compiled: serde_json::Value = serde_json::from_str(&compiled).expect("valid json");
        assert_eq!(compiled["ok"], true, "{compiled}");
        let story_bytes: Vec<u8> = compiled["story_bytes"]
            .as_array()
            .expect("story_bytes array")
            .iter()
            .map(|b| b.as_u64().expect("byte") as u8)
            .collect();

        let fragment_runner = StoryRunner::new(&story_bytes).ok().expect("constructs");
        let save = live.save().ok().expect("save");
        fragment_runner.load(&save).ok().expect("load");

        let spec = fragment_runner.speculate("{}").ok().expect("speculate");
        spec.go_to_path("__eval_test").ok().expect("go_to_path");
        let line = spec.advance().ok().expect("advance");
        assert!(
            line.contains("You have 3 gold."),
            "transcript reflects the live-seeded global; got {line}"
        );
    }

    #[wasm_bindgen_test]
    fn fragment_that_compiles_as_neither_kind_is_a_diagnostic_not_a_panic() {
        let compiled = compile_fragment(
            "main.ink",
            &sources_json(SRC),
            "=== __eval_test ===\nYou have {totally_unknown_name} gold.\n",
        );
        let compiled: serde_json::Value = serde_json::from_str(&compiled).expect("valid json");
        assert_eq!(compiled["ok"], false, "{compiled}");
        assert!(
            compiled["warnings"]
                .as_array()
                .is_some_and(|w| !w.is_empty()),
            "{compiled}"
        );
    }
}
