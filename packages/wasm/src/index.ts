/**
 * @brink-lang/web — the brink compiler, IDE session, and story runtime
 * compiled to WebAssembly, behind ergonomic TypeScript wrappers.
 *
 * Wraps the raw wasm module classes (brink-web, built with wasm-pack
 * `--target web`) in wrappers that parse JSON responses into the typed
 * interfaces re-exported below. Call {@link initWasm} once before using
 * anything else.
 */

import init, {
  compile as wasmCompile,
  compile_fragment as wasmCompileFragment,
  program_checksum as wasmProgramChecksum,
  program_model_of as wasmProgramModelOf,
  program_inkt_of as wasmProgramInktOf,
  lines_table_of as wasmLinesTableOf,
  size_report_of as wasmSizeReportOf,
  token_type_names,
  diagnostic_registry,
  token_modifier_names,
  EditorSession as WasmEditorSession,
  StoryRunner,
  WebSpeculation,
  WebSession,
  diffSnapshots as wasmDiffSnapshots,
} from "brink-web";
// Namespace import for feature-detected exports: `ClassifierSession` (W3 of
// docs/editor-worker-spec.md) is absent from older wasm builds and the test
// mock — a named import would fail ESM linking there, the namespace probe
// degrades to "not available" instead.
import * as brinkWebModule from "brink-web";

import type {
  CompileResult,
  SemanticToken,
  HirProjection,
  CompletionItem,
  HoverInfo,
  Location,
  LocationWithKind,
  ExplainMatch,
  InlayHint,
  ColorHint,
  CallWidgetSite,
  SignatureInfo,
  FoldRange,
  DocumentSymbol,
  CodeAction,
  CodeActionData,
  Fix,
  ProjectFile,
  FileOutline,
  PassageLine,
  PassageOrigin,
  PerfCounters,
  StoryGraph,
  LineContext,
  ConvertTarget,
  TextEdit,
  AutoImportResult,
  IncludeInfo,
  DocumentId,
  DocumentChangeSpec,
  Line,
  Choice,
  StructuralResult,
  DirMoveResult,
  DebugState,
  DebugLine,
  DebugSourceLocation,
  ProgramAddress,
  Breakpoint,
  DebugRunOutcome,
  StepMode,
  ProgramModel,
  LinesTable,
  SizeReport,
  SaveState,
  LoadReport,
  HostManifest,
  ValueItem,
  DialogueDialect,
  StepOutcome,
  SessionJournal,
  SessionLine,
  StateSnapshot,
  StateDiff,
  ReplayOutcome,
  JournalDirtySignal,
  SpeculationOptions,
  SpeculationContext,
  SpeculationKinds,
  SpeculationFunctionEval,
  SpeculationExternalsReport,
  SpeculationLine,
  SpeculationResult,
  TypedValue,
  ProjectSource,
  StructuralTranscript,
  RenderedTranscriptLine,
} from "@brink/wasm-types";

import {
  parseEvaluateSource,
  fragmentContentHash,
  cacheFragmentInto,
  isNativeEntry,
  expressionWrapSource,
  contentWrapSource,
  FRAGMENT_CACHE_LIMIT,
} from "./evaluate-dispatch";
import type { FragmentCompileEntry, ExternalValue } from "./evaluate-dispatch";

// Public surface: every interface the wasm boundary speaks is available
// from this package alone (the private @brink/wasm-types workspace package
// is rolled into the published declarations).
export type * from "@brink/wasm-types";

// ── Wasm initialization ─────────────────────────────────────────

/**
 * Initialize the wasm module. Must complete before any other export is used.
 * Safe to call more than once.
 *
 * By default the `.wasm` binary is located relative to this module
 * (`new URL("brink_web_bg.wasm", import.meta.url)`), which bundlers like
 * Vite resolve and emit automatically. Pass `wasmLocation` to load it from
 * somewhere else (a CDN URL, a string path, or a precompiled
 * `WebAssembly.Module`).
 */
export async function initWasm(
  wasmLocation?: string | URL | Request | WebAssembly.Module,
): Promise<void> {
  if (wasmLocation === undefined) {
    await init();
  } else {
    await init({ module_or_path: wasmLocation });
  }
}

// ── Compilation ─────────────────────────────────────────────────

export function compile(source: string): CompileResult {
  const json = wasmCompile(source);
  return JSON.parse(json) as CompileResult;
}

/**
 * The source-identity checksum of compiled `.inkb` bytes — identical to
 * `ProgramModel.checksum` (`"0x{:08x}"`), but computed without constructing a
 * `StoryRunnerHandle`. The studio compares a running program's identity to its
 * latest compile to detect "source out of sync" (live-inspector degraded mode).
 */
export function programChecksum(storyBytes: Uint8Array): string {
  return wasmProgramChecksum(storyBytes);
}

/**
 * Structured `ProgramModel` for compiled `.inkb` bytes, runner-free
 * (W7/#3300): since "no auto-start", the Program Explorer and Compiled
 * Output are compile-bound — they must not need a running session.
 */
export function programModelOf(storyBytes: Uint8Array): ProgramModel {
  return JSON.parse(wasmProgramModelOf(storyBytes)) as ProgramModel;
}

/** The `.inkt` disassembly for compiled `.inkb` bytes, runner-free. */
export function programInktOf(storyBytes: Uint8Array): string {
  return wasmProgramInktOf(storyBytes);
}

/**
 * The compiled lines table for `.inkb` bytes, runner-free (#3339) — the
 * static mirror of {@link StoryRunner.linesTable}, for the Program
 * Explorer's Line tables view, which shows compiled output whether or not
 * a story is running.
 */
export function linesTableOf(storyBytes: Uint8Array): LinesTable {
  return JSON.parse(wasmLinesTableOf(storyBytes)) as LinesTable;
}

/** The `.inkb` size report, runner-free (#3339 Size view). */
export function sizeReportOf(storyBytes: Uint8Array): SizeReport {
  return JSON.parse(wasmSizeReportOf(storyBytes)) as SizeReport;
}

// ── Token legend (stateless) ────────────────────────────────────

let cachedTypeNames: string[] | null = null;
let cachedModifierNames: string[] | null = null;

/** One diagnostic code, as the settings UI needs it (#3169). */
export interface DiagnosticInfo {
  /** `"E014"`. */
  code: string;
  /** One line; always present. */
  title: string;
  /** The code's DEFAULT severity. */
  default_severity: "error" | "warning" | "info";
  /**
   * Whether `[lints]` can override it AT ALL. Only 30 of the 189 codes can
   * — the analyzer refuses every code whose default severity is not
   * `warning`. A UI that ignores this offers a level picker for a code the
   * analyzer then discards, which is the silent no-op the settings surface
   * exists to prevent.
   */
  overridable: boolean;
  /**
   * The written explanation, absent when nobody has written one (158 of 189
   * today). Absent rather than empty, so forgetting to check cannot render
   * a blank panel.
   */
  explanation?: string;
  /**
   * The author-facing group this belongs to. Present only for overridable
   * codes, since those are the only ones the settings section lists.
   */
  category?: string;
  /**
   * Which source surfaces this code can arise on. A project filters its
   * Diagnostics list by this, so a `strict-ink` project is not offered
   * settings for markup spans it cannot write.
   *
   * Defaults to both: hiding a code an author is actually seeing is worse
   * than showing one that cannot fire, so only codes the compiler itself
   * calls native-only are narrowed.
   */
  surfaces: ("ink" | "native")[];
}

let cachedDiagnostics: DiagnosticInfo[] | null = null;

/**
 * Every diagnostic code the compiler knows, ordered by code (#3169).
 *
 * Static for a given build, so it is cached after the first call. Read this
 * rather than keeping a list in TypeScript: a hand-maintained copy is wrong
 * the moment a code is added, and wrong SILENTLY — a missing code simply
 * never appears, and nobody notices a diagnostic they cannot configure.
 */
export function getDiagnosticRegistry(): DiagnosticInfo[] {
  cachedDiagnostics ??= JSON.parse(diagnostic_registry()) as DiagnosticInfo[];
  return cachedDiagnostics;
}

export function getTokenTypeNames(): string[] {
  if (!cachedTypeNames) {
    cachedTypeNames = JSON.parse(token_type_names()) as string[];
  }
  return cachedTypeNames;
}

export function getTokenModifierNames(): string[] {
  if (!cachedModifierNames) {
    cachedModifierNames = JSON.parse(token_modifier_names()) as string[];
  }
  return cachedModifierNames;
}

// ── EditorSession wrapper ───────────────────────────────────────

/** The outbound-delta segment manifest (#3064 option A). */
export interface SegmentManifest {
  totalLines: number;
  segments: { key: string; ownedFrom: number }[];
}

/** One bounded edit in UTF-16 coordinates of the previous content (#3064 C1). */
export interface EditSpan {
  from: number;
  to: number;
  insert: string;
}

/** What one `[project] drafts` glob currently matches (#3145). */
export interface DraftGlobMatches {
  /** The glob exactly as the author wrote it in `brink.toml`. */
  glob: string;
  /** Files this glob makes drafts — matched, and outside the compile closure. */
  drafts: string[];
  /**
   * Files it matches that the entry still reaches, so they are NOT drafts
   * ("reachability wins", 2026-08-27). A non-empty list here is a glob that
   * looks like it took effect and did not.
   */
  inStory: string[];
}

/** See {@link EditorSessionHandle.getDraftGlobReport}. */
export interface DraftGlobReport {
  /**
   * Whether a compile has happened. When false every list is empty because
   * draft status is not yet knowable — which reads identically to "nothing
   * matched" in the data and means the opposite.
   */
  compiled: boolean;
  /** One entry per configured glob, in the order the author wrote them. */
  globs: DraftGlobMatches[];
}

export class EditorSessionHandle {
  private session: WasmEditorSession;
  private mutationCount = 0;

  constructor() {
    this.session = new WasmEditorSession();
  }

  /**
   * Monotonic counter bumped by every content-mutating call (file updates,
   * document pushes, structural moves, manifest changes). Lets consumers
   * cache derived results (e.g. a project compile) and invalidate exactly
   * when the session could have changed.
   */
  get generation(): number {
    return this.mutationCount;
  }

  /** Mark the session as (potentially) changed. */
  private bump(): void {
    this.mutationCount += 1;
  }

  /**
   * Session-wide CONFIG epoch (#3064 micro): bumped by registrations that
   * change query OUTPUTS without changing any segment's identity key —
   * dialect and host-manifest swaps. Slice caches keyed by segment
   * identity must also stamp this epoch, or a dialect change would serve
   * stale dialect-classified slices under unchanged keys.
   */
  private configEpochCounter = 0;

  configEpoch(): number {
    return this.configEpochCounter;
  }

  updateSource(source: string): void {
    this.bump();
    this.session.update_source(source);
  }

  updateFile(path: string, source: string): void {
    this.bump();
    this.session.update_file(path, source);
  }

  /** Remove a file from the project. Returns `false` (no-op) when `path`
   *  currently resolves to a mounted stdlib copy (issue #2306/#2343) — the
   *  Rust-side read-only fence, mirroring `updateFile`'s refusal for a
   *  mounted key. */
  removeFile(path: string): boolean {
    this.bump();
    return this.session.remove_file(path);
  }

  /**
   * Register (or replace) the host-capability manifest, then re-analyze.
   * Describes the host's external-function vocabulary for author-time
   * validation and richer hover/completion. Throws on an invalid manifest.
   */
  setHostManifest(manifest: HostManifest): void {
    this.configEpochCounter += 1;
    this.bump();
    this.session.set_host_manifest(JSON.stringify(manifest));
  }

  /** Clear any registered host manifest, then re-analyze. */
  clearHostManifest(): void {
    this.configEpochCounter += 1;
    this.bump();
    this.session.clear_host_manifest();
  }

  /**
   * Register (or replace) the dialogue dialect (#368). Describes the
   * project's dialogue-line conventions (cues, parentheticals, dialogue
   * chains) so `getLineContextsDoc`/`line_contexts` classify lines without
   * hardcoding any one convention. Tooling-only — never affects the runtime
   * or analysis; consumed at query time. Throws on an invalid dialect
   * (schema violation, non-portable pattern, undeclared chain/transition
   * kind, …).
   */
  setDialect(dialect: DialogueDialect): void {
    this.configEpochCounter += 1;
    this.bump();
    this.session.set_dialect(JSON.stringify(dialect));
  }

  /** Clear the registered dialect. Line classification reverts to plain
   *  structural kinds. */
  clearDialect(): void {
    this.configEpochCounter += 1;
    this.bump();
    this.session.clear_dialect();
  }

  /**
   * Enable or disable the machinery/narrative fold runs (#479 — **off by
   * default**). Hosts implementing prose/logic view modes turn this on
   * (typically once at mount, alongside `setActiveFoldKinds` in the editor);
   * everyone else skips the per-query run computation entirely. Session-wide,
   * like `setDialect`.
   */
  setFoldRunsEnabled(enabled: boolean): void {
    this.bump();
    this.session.set_fold_runs_enabled(enabled);
  }

  /**
   * Turn the D6 `DebugInfo` section on or off for this session's compiles
   * (`docs/debugger-spec.md` §1.2, issue #3229) — the toggle that makes the
   * debugger reachable at all.
   *
   * The studio's live session runs on exactly the bytes this session's
   * `compileProject` produces, so without the section the runtime position,
   * locals table and program→source resolver all resolve to nothing.
   *
   * **Per-session by ruling (2026-08-28)**: turn it on for the session you
   * are about to debug, off when that session ends. Ordinary authoring then
   * never pays the size/time cost, and debuggability is not baked into the
   * project.
   *
   * ⚠ **You must recompile for this to take effect.** It changes what the
   * *next* `compileProject` emits, not the artifact you already hold. That
   * recompile is codegen only — diagnostics are byte-identical either way
   * and stay memoized — so it is cheap, but it is not automatic.
   *
   * No-ops when unchanged, so calling it on every debug-session start is
   * safe. Deliberately does NOT bump the config epoch: no query OUTPUT the
   * editor renders changes, so invalidating the identity-keyed slice caches
   * would be pure waste.
   */
  setDebugInfoEnabled(enabled: boolean): void {
    this.bump();
    this.session.set_debug_info_enabled(enabled);
  }

  /**
   * Whether this session's compiles emit the `DebugInfo` section (#3229).
   * See {@link setDebugInfoEnabled}.
   */
  debugInfoEnabled(): boolean {
    return this.session.debug_info_enabled();
  }

  /**
   * Push the host's current values for `host`-source semantic types (#174) —
   * a full snapshot keyed by semantic-type name that **replaces** the cache.
   * The attached host (e.g. RPG Maker MZ) calls this with its named switches /
   * items / … so the argument picker + value-label inlay hints stay current.
   * Tooling-only — no re-analyze.
   */
  setHostValues(values: Record<string, ValueItem[]>): void {
    this.bump();
    this.session.set_host_values(JSON.stringify(values));
  }

  /** Clear the host-pushed value cache (e.g. on host disconnect). */
  clearHostValues(): void {
    this.bump();
    this.session.clear_host_values();
  }

  /**
   * Set the severity of manifest-driven external diagnostics: `"error"`
   * (default — a registered manifest is binding) or `"off"`.
   */
  setExternalCheck(level: "error" | "off"): void {
    this.bump();
    this.session.set_external_check(level);
  }

  /**
   * Set the severity policy for unknown-semantic-type diagnostics (#532):
   * `"tolerant"` (default — unresolved types are only diagnosed once a host
   * manifest is registered, #339/#527) or `"error"` (always diagnose, even
   * with no manifest registered — catches typo'd host semantic-type tags).
   */
  setSemanticTypeCheck(level: "tolerant" | "error"): void {
    this.bump();
    this.session.set_semantic_type_check(level);
  }

  /**
   * Set the T1b compiler dialect (docs/t1b-surface-spec.md §1, #589, #600,
   * #611): `"brink"` or `"strict-ink"` (default — any other value, or never
   * calling this at all, keeps `StrictInk`). Gates stdlib slice 1 completion
   * (`getCompletionsDoc`), dialect-aware signature help
   * (`getSignatureHelpDoc`), and the background analysis pass's `E051`
   * "brink extension" diagnostic — a `brink`-dialect project no longer shows
   * permanent spurious `E051` on valid extension syntax. Re-analyzes
   * immediately (like `setExternalCheck`/`setSemanticTypeCheck`).
   */
  setLanguageDialect(value: "brink" | "strict-ink"): void {
    this.bump();
    this.session.set_language_dialect(value);
  }

  /**
   * Set the TM-3 typed-mode policy (docs/typed-mode-spec.md §1, #660):
   * `"strict"` or `"gradual"`. Never calling this keeps the dialect-keyed
   * default (NS-A9, 2026-07-19): `"brink"` sessions resolve `strict`,
   * `"strict-ink"` sessions resolve `gradual`; an explicit call — or a
   * `types` field applied via `applyProjectConfig` — always wins over that
   * default. An unrecognized value is ignored (the resolved policy is
   * unchanged). Mirrors `setLanguageDialect` exactly.
   * `"strict"` requires `setLanguageDialect("brink")` to also be in effect,
   * or the compile/analysis surface a single project-level `E064`
   * config-error diagnostic instead of running the normal passes (the
   * caller's responsibility, same as the compiler CLI). Re-analyzes
   * immediately (like `setLanguageDialect`).
   */
  setTypePolicy(value: "strict" | "gradual"): void {
    this.bump();
    this.session.set_type_policy(value);
  }

  /**
   * Parse a `brink.toml` project settings file (#1005 — dialect + type
   * policy, one config every compiler mount reads) and apply its
   * `[project] dialect`/`types` *and* `[lints]`/`deny-warnings` (#1366) to
   * this session. Prefer {@link discoverProjectConfig} (#1414) when
   * `brink.toml` is (or can be) loaded into this session as an ordinary
   * document via {@link updateFile} — it resolves the file automatically
   * through the same discovery mechanism `brink compile`/`brink ide`/
   * `bevy-brink` use, so your embedder needs no host-specific directory-walk
   * code of its own. This method stays for embedders that read
   * `brink.toml`'s text with their own host API (Node `fs`, the File System
   * Access API, a bundler import, …) without ever loading it into the
   * session, and just want that text applied.
   *
   * Call this once, right after construction, before any explicit
   * {@link setLanguageDialect}/{@link setTypePolicy} call — an explicit
   * call always overrides the file for `dialect`/`types` (matches the
   * CLI's `--dialect`/`--types` flag precedence: the file is the default,
   * code wins). `[lints]`/`deny-warnings` has its own explicit-API
   * override tier too (#1417): {@link setLintOverrides}/
   * {@link setDenyWarningsOverride} always win over whatever this call
   * resolves from the file, in either call order. Re-analyzes immediately
   * for whichever field the file sets — including `[lints]`, which can
   * change diagnostic severity: a `[lints] E014 = "deny"` or
   * `deny-warnings = true` entry can promote a diagnostic that previously
   * rendered as `"Warning"` to `"Error"` in subsequent `compileProject`
   * results.
   *
   * Returns the list of warning strings for unrecognized `[project]` keys
   * *and* unrecognized/non-overridable `[lints]` codes — never an error
   * (forward compat). Throws only on malformed TOML or a recognized key
   * with an invalid value.
   */
  applyProjectConfig(toml: string): string[] {
    this.bump();
    const json = this.session.apply_project_config(toml);
    return JSON.parse(json) as string[];
  }

  /**
   * Discover and apply this session's `brink.toml`, if one exists among the
   * currently loaded documents (#1414) — the web-mount counterpart of
   * `brink compile`/`brink ide`'s filesystem-walk discovery, resolved
   * instead by walking this session's own in-memory document tree (the
   * `SourceTree`/producer seam every other mount already uses). Serve
   * `brink.toml` as an ordinary document — `updateFile("brink.toml", text)`,
   * at `entry`'s directory or any ancestor of it — and call this once (e.g.
   * right after loading the project's files) in place of
   * {@link applyProjectConfig}: no host-specific directory-walk code (Node
   * `fs`, the File System Access API, …) required.
   *
   * `entry` is a session document path (whatever was passed to
   * {@link updateFile}), not a filesystem path — and it must share the same
   * root-relative spelling (no leading `/`) as every document path in this
   * session, since the ancestor walk-up matches keys by exact string
   * equality. Mixing a `/`-prefixed `entry` or document path with
   * unprefixed ones is a silent no-op: discovery finds nothing and this
   * returns `[]` exactly as if no `brink.toml` existed, with no warning.
   *
   * Returns `[]` when no `brink.toml` is found anywhere from `entry`'s
   * directory up to the tree root — missing config is unchanged behavior,
   * never a thrown error. Otherwise applies and re-analyzes exactly like
   * {@link applyProjectConfig}: an explicit {@link setLanguageDialect}/
   * {@link setTypePolicy} call still wins over the file, `[lints]` still
   * always merges, and the returned array carries the same
   * unrecognized-key/lint-code warning strings. The `[lints]`/`deny-warnings`
   * override tier too (#1417): {@link setLintOverrides}/
   * {@link setDenyWarningsOverride} still win over whatever this call
   * resolves from the file. Throws only on malformed TOML or a recognized
   * key with an invalid value.
   */
  discoverProjectConfig(entry: string): string[] {
    this.bump();
    const json = this.session.discover_project_config(entry);
    return JSON.parse(json) as string[];
  }

  /**
   * The `[project] entry` value from the most recently applied
   * `brink.toml` (issue #2331, ruled 2026-08-07 "`[project] entry` beats
   * `mountStudio`'s `entryFile`") — `null` when no `brink.toml` was found,
   * or one was found that doesn't set `entry`. Call this after
   * {@link discoverProjectConfig}/{@link applyProjectConfig}; per the
   * ruling, a non-null result should supersede the host's own
   * constructor-time entry-file argument (`ProjectSession`'s `entryFile`
   * option, `mountStudio`'s `entryFile`), which is only the fallback for a
   * configless project. This wrapper doesn't check the path resolves to a
   * real file in the session — the caller does that (`ProjectSession` in
   * `packages/ink-editor/src/project-session.ts`).
   */
  getConfiguredEntry(): string | null {
    return this.session.configured_entry() ?? null;
  }

  /**
   * `[project] indent` from the applied `brink.toml` (#3149) — the width
   * the editor's `indentUnit` and the formatter both read, so they cannot
   * disagree.
   *
   * `null` means the project set no `indent`, NOT "four". The caller
   * applies the shared default, so that "the project said nothing" stays
   * distinguishable from "the project said four" and a later change to the
   * default is not silently baked in here.
   */
  getConfiguredIndent(): number | null {
    return this.session.configured_indent() ?? null;
  }

  /**
   * `[prose] dialect` from the applied `brink.toml` (#3211), or `null` when
   * the file set none — the host applies its own default.
   */
  getConfiguredProseDialect(): string | null {
    return this.session.configured_prose_dialect() ?? null;
  }

  /**
   * `[prose] enable` from the applied `brink.toml` (#3211), or `null` when
   * the file set none — tri-state on purpose, so the host's default stays
   * the host's rather than being baked in below it.
   */
  getConfiguredProseEnable(): boolean | null {
    return this.session.configured_prose_enable() ?? null;
  }

  /**
   * `[prose] dictionary` from the applied `brink.toml` — the author's own
   * word list, in the order the file writes it.
   *
   * Empty when the file sets none: unlike dialect and enable there is no
   * third state to model, because "declared but empty" and "absent" ask the
   * checker for exactly the same thing.
   */
  getConfiguredProseDictionary(): string[] {
    return JSON.parse(this.session.configured_prose_dictionary()) as string[];
  }

  /**
   * `[dialogue]` from the applied `brink.toml`, RESOLVED (#3387, RULED
   * 2026-08-30 "Project-declared dialogue dialect lives in brink.toml"):
   * the shipped preset merged with the file's affix-sugar overlays — the
   * one `DialogueDialect` every editor view and the Player read. `null`
   * when the project declares none: "no dialect by default" — plain
   * lines, never the preset.
   */
  /** Why `[dialogue]` did not resolve (#3391) — the resolver's readable
   * message — or `null` when it resolved or the project declares none.
   * State, not a one-shot warning: config warnings are a delta against
   * the previous apply, and a Problems panel needs the current truth. */
  getConfiguredDialogueError(): string | null {
    const raw = this.session as { configured_dialogue_error?: () => string | undefined };
    if (typeof raw.configured_dialogue_error !== "function") return null;
    return raw.configured_dialogue_error() ?? null;
  }

  getConfiguredDialogueDialect(): DialogueDialect | null {
    // Feature-detected on the raw session too: `session` is an injection
    // seam and stubs that predate this accessor must read as "declares
    // nothing", not throw at view mount.
    const raw = this.session as { configured_dialogue_dialect?: () => string | undefined };
    if (typeof raw.configured_dialogue_dialect !== "function") return null;
    const json = raw.configured_dialogue_dialect();
    return json === undefined || json === null ? null : (JSON.parse(json) as DialogueDialect);
  }

  /**
   * Set explicit CLI/API-tier per-code `[lints]` overrides (#1417) — the
   * wasm/editor counterpart of `brink compile`'s repeatable
   * `--deny`/`--warn`/`--allow <CODE>` flags and `brink-lsp`'s
   * `initializationOptions.lints`. `overrides` maps a diagnostic code to
   * `"deny" | "warn" | "allow" | "info" | "hint"`. Wholesale **replaces** this session's
   * explicit override map (mirrors {@link applyProjectConfig}'s own
   * `[lints]`-replace-not-merge semantics) — pass `{}` to clear every
   * override.
   *
   * Always wins over the same code in an applied `brink.toml`'s `[lints]`
   * table, in either call order: this reapplies on top of whatever
   * {@link applyProjectConfig}/{@link discoverProjectConfig} last
   * resolved from the file, and a later file re-apply reapplies these
   * overrides on top of itself — so a `brink.toml` reload can never
   * silently drop a previously-set explicit override.
   *
   * Throws only on malformed JSON. An unrecognized per-code level string
   * and an unrecognized/non-overridable diagnostic code are never thrown
   * — both are reported as warning strings in the returned array, the
   * same channel {@link applyProjectConfig} uses. Re-analyzes
   * immediately.
   */
  setLintOverrides(overrides: Record<string, "deny" | "warn" | "allow" | "info" | "hint">): string[] {
    this.bump();
    const json = this.session.set_lint_overrides(JSON.stringify(overrides));
    return JSON.parse(json) as string[];
  }

  /**
   * Set an explicit `deny-warnings` override (#1417), parallel to
   * {@link setLintOverrides} — the wasm/editor counterpart of
   * `brink compile`'s `-D warnings` and `brink-lsp`'s
   * `initializationOptions.denyWarnings`. Always wins over an applied
   * `brink.toml`'s `deny-warnings` key. Re-analyzes immediately.
   */
  setDenyWarningsOverride(deny: boolean): void {
    this.bump();
    this.session.set_deny_warnings_override(deny);
  }

  /**
   * Clear the explicit `deny-warnings` override set by
   * {@link setDenyWarningsOverride} — reverts to the applied
   * `brink.toml`'s `deny-warnings` value (or `false`, absent a file).
   * Re-analyzes immediately.
   */
  clearDenyWarningsOverride(): void {
    this.bump();
    this.session.clear_deny_warnings_override();
  }

  setActiveFile(path: string): boolean {
    return this.session.set_active_file(path);
  }

  getActiveFile(): string {
    return this.session.active_file();
  }

  /** Scope IDE queries to a sub-region `[start, end)` of the active file. */
  setViewContext(start: number, end: number): void {
    this.session.set_view_context(start, end);
  }

  /** Return to full-file mode. */
  clearViewContext(): void {
    this.session.clear_view_context();
  }

  /** Get the source text for the current view context (fragment or full file). */
  getViewSource(): string | null {
    const json = this.session.get_view_source();
    const result = JSON.parse(json);
    return result ?? null;
  }

  // ── Document handles (multi-document API) ─────────────────────
  //
  // Each handle pairs a file path with an optional fragment view, so N
  // live editor views can issue IDE queries independently of the legacy
  // active-file/view-context singleton above. Offsets are UTF-16 and
  // view-relative per handle, like the singleton queries.

  /** Open a full-file document handle. Returns null if the file is not loaded. */
  openDocument(path: string): DocumentId | null {
    const id = this.session.open_document(path);
    return id === 0 ? null : id;
  }

  /**
   * Open a fragment document handle scoping `path` to `[start, end)` (UTF-16
   * offsets, like setViewContext). Returns null if the file is not loaded.
   */
  openFragment(path: string, start: number, end: number): DocumentId | null {
    const id = this.session.open_fragment(path, start, end);
    return id === 0 ? null : id;
  }

  /** Close a document handle. Returns false if the handle was unknown. */
  closeDocument(doc: DocumentId): boolean {
    return this.session.close_document(doc);
  }

  /**
   * Replace a document's content: full-file replace for file handles,
   * fragment splice for fragment handles. Returns a change spec describing
   * what actually changed in the file (UTF-16 file coordinates) for
   * live-mirroring sibling views, or null for an unknown handle.
   */
  updateDocument(doc: DocumentId, source: string): DocumentChangeSpec | null {
    this.bump();
    const json = this.session.update_document(doc, source);
    const result = JSON.parse(json);
    return result ?? null;
  }

  /**
   * Apply a bounded edit list to a FILE handle's document (#3064 C1) —
   * the delta sibling of {@link updateDocument}: the full document never
   * crosses the wasm boundary, and the write is source-only (no fused
   * eager analysis; consumers pull what they need). Edits are ascending,
   * non-overlapping `{from, to, insert}` in UTF-16 coordinates of the
   * PREVIOUS content (CM6 `ChangeSet.iterChanges` A-side). Returns false
   * — applying nothing — for fragment handles, read-only files, or
   * malformed edits; the caller falls back to a full-text push.
   */
  applyEditsDocument(doc: DocumentId, edits: readonly EditSpan[]): boolean {
    // Tolerate an older wasm build or a test mock without the export —
    // callers fall back to the full-text push.
    const raw = (this.session as { apply_edits_document?: (d: DocumentId, e: string) => boolean })
      .apply_edits_document;
    if (typeof raw !== "function") return false;
    this.bump();
    return raw.call(this.session, doc, JSON.stringify(edits));
  }

  /** Get the source text for a handle's view (fragment or full file). */
  getViewSourceDoc(doc: DocumentId): string | null {
    const json = this.session.get_view_source_doc(doc);
    const result = JSON.parse(json);
    return result ?? null;
  }

  getLineContextsDoc(doc: DocumentId): LineContext[] {
    const json = this.session.line_contexts_doc(doc);
    return JSON.parse(json) as LineContext[];
  }

  getSemanticTokensDoc(doc: DocumentId): SemanticToken[] {
    const json = this.session.semantic_tokens_doc(doc);
    return JSON.parse(json) as SemanticToken[];
  }

  /**
   * The outbound-delta segment manifest (#3064 option A): per-segment
   * version keys (salsa identity `index:generation` — stable across shift
   * edits, changed exactly when a segment's content changes) plus each
   * segment's first owned line. `null` for fragment views, non-ink files,
   * older wasm builds, and test mocks — the consumer falls back to the
   * whole-document queries.
   */
  getSegmentManifestDoc(doc: DocumentId): SegmentManifest | null {
    const raw = (this.session as { segment_manifest_doc?: (d: DocumentId) => string })
      .segment_manifest_doc;
    if (typeof raw !== "function") return null;
    return JSON.parse(raw.call(this.session, doc)) as SegmentManifest | null;
  }

  /** One manifest segment's owned line-context slice; `null` on a stale key. */
  getSegmentLineContextsDoc(doc: DocumentId, key: string): LineContext[] | null {
    const raw = (this.session as { segment_line_contexts_doc?: (d: DocumentId, k: string) => string })
      .segment_line_contexts_doc;
    if (typeof raw !== "function") return null;
    return JSON.parse(raw.call(this.session, doc, key)) as LineContext[] | null;
  }

  /**
   * Classifier-only sibling of {@link getSegmentSemanticTokensDoc}
   * (#3064 micro): no resolution refinement, no analysis pull — the
   * keystroke path's source; the deferred refresh swaps in the refined
   * slice.
   */
  getSegmentSemanticTokensFastDoc(doc: DocumentId, key: string): SemanticToken[] | null {
    const raw = (
      this.session as { segment_semantic_tokens_fast_doc?: (d: DocumentId, k: string) => string }
    ).segment_semantic_tokens_fast_doc;
    if (typeof raw !== "function") return null;
    return JSON.parse(raw.call(this.session, doc, key)) as SemanticToken[] | null;
  }

  /**
   * One manifest segment's owned semantic-token slice, token lines RELATIVE
   * to the segment's owned start; `null` on a stale key.
   */
  getSegmentSemanticTokensDoc(doc: DocumentId, key: string): SemanticToken[] | null {
    const raw = (this.session as { segment_semantic_tokens_doc?: (d: DocumentId, k: string) => string })
      .segment_semantic_tokens_doc;
    if (typeof raw !== "function") return null;
    return JSON.parse(raw.call(this.session, doc, key)) as SemanticToken[] | null;
  }

  /**
   * The HIR structural projection (#454) for a document: nested semantic spans
   * plus the per-line container stack (rails view). Positions are 0-based
   * lines / UTF-16 columns, same conventions as semantic tokens.
   */
  getHirSpansDoc(doc: DocumentId): HirProjection {
    const json = this.session.hir_spans_doc(doc);
    return JSON.parse(json) as HirProjection;
  }

  getCompletionsDoc(doc: DocumentId, offset: number): CompletionItem[] {
    const json = this.session.completions_doc(doc, offset);
    return JSON.parse(json) as CompletionItem[];
  }

  /**
   * Auto-import (#312 F) `target` into the file backing `doc`. Returns whether
   * `target` was already reachable from the current file's INCLUDE graph and,
   * when not, the whole-file UTF-16 `INCLUDE`-insertion edit to apply. No
   * `bump()` — reachability is derived from the last analysed sources, and the
   * op mutates nothing on the wasm side (it only computes the edit).
   */
  autoImportIncludeDoc(doc: DocumentId, target: string): AutoImportResult {
    const json = this.session.auto_import_include_doc(doc, target);
    return JSON.parse(json) as AutoImportResult;
  }

  /**
   * Auto-import (#312 F, fragment-view path) `target` into the file backing
   * `doc` **and apply the INCLUDE edit out-of-band**, rebasing every open
   * fragment view on that file so a subsequent fragment splice lands at the
   * correct (post-shift) range. Unlike {@link autoImportIncludeDoc} this
   * mutates the session (it applies the INCLUDE), so it `bump()`s. Returns the
   * same result shape but with **no `edit`** on success — the INCLUDE is
   * already applied, so the caller only inserts the symbol text into the
   * fragment view. Use this for fragment (symbol-tab) views, where the INCLUDE
   * lives above the fragment and cannot be dispatched into its CM document.
   */
  autoImportApplyIncludeDoc(doc: DocumentId, target: string): AutoImportResult {
    this.bump();
    const json = this.session.auto_import_apply_include_doc(doc, target);
    return JSON.parse(json) as AutoImportResult;
  }

  getHoverDoc(doc: DocumentId, offset: number): HoverInfo | null {
    const json = this.session.hover_doc(doc, offset);
    const result = JSON.parse(json);
    return result ?? null;
  }

  /**
   * Document-handle variant of {@link explainMatch}. Same raw-byte,
   * file-absolute range caveat applies — see that method's own docstring.
   */
  explainMatchDoc(doc: DocumentId, offset: number): ExplainMatch | null {
    const json = this.session.explain_match_doc(doc, offset);
    const result = JSON.parse(json);
    return result ?? null;
  }

  gotoDefinitionDoc(doc: DocumentId, offset: number): Location | null {
    const json = this.session.goto_definition_doc(doc, offset);
    const result = JSON.parse(json);
    return result ?? null;
  }

  findReferencesDoc(doc: DocumentId, offset: number): Location[] {
    const json = this.session.find_references_doc(doc, offset);
    return JSON.parse(json) as Location[];
  }

  prepareRenameDoc(doc: DocumentId, offset: number): Location | null {
    const json = this.session.prepare_rename_doc(doc, offset);
    const result = JSON.parse(json);
    return result ?? null;
  }

  getCodeActionsDoc(doc: DocumentId, offset: number): CodeAction[] {
    const json = this.session.code_actions_doc(doc, offset);
    return JSON.parse(json) as CodeAction[];
  }

  /** Document-handle variant of `resolveCodeAction`. */
  resolveCodeActionDoc(doc: DocumentId, data: CodeActionData, offset: number): StructuralResult {
    this.bump();
    const json = this.session.resolve_code_action_doc(doc, JSON.stringify(data), offset);
    return JSON.parse(json) as StructuralResult;
  }

  /** Document-handle variant of `getFixes`. */
  getFixesDoc(doc: DocumentId, offset: number): Fix[] {
    const json = this.session.fixes_at_doc(doc, offset);
    return JSON.parse(json) as Fix[];
  }

  /** Document-handle variant of `applyFix`. */
  applyFixDoc(doc: DocumentId, fix: Fix): StructuralResult {
    this.bump();
    const json = this.session.apply_fix_doc(doc, JSON.stringify(fix));
    return JSON.parse(json) as StructuralResult;
  }

  getInlayHintsDoc(doc: DocumentId, start: number, end: number): InlayHint[] {
    const json = this.session.inlay_hints_doc(doc, start, end);
    return JSON.parse(json) as InlayHint[];
  }

  /** `hex_color` argument literals in range, for the built-in color picker. */
  getColorHintsDoc(doc: DocumentId, start: number, end: number): ColorHint[] {
    const json = this.session.color_hints_doc(doc, start, end);
    return JSON.parse(json) as ColorHint[];
  }

  /** Argument-widget sites in range — per-call slots + state (Edit/Fill). */
  getArgumentWidgetsDoc(doc: DocumentId, start: number, end: number): CallWidgetSite[] {
    const json = this.session.argument_widgets_doc(doc, start, end);
    return JSON.parse(json) as CallWidgetSite[];
  }

  getSignatureHelpDoc(doc: DocumentId, offset: number): SignatureInfo | null {
    const json = this.session.signature_help_doc(doc, offset);
    const result = JSON.parse(json);
    return result ?? null;
  }

  getFoldingRangesDoc(doc: DocumentId): FoldRange[] {
    const json = this.session.folding_ranges_doc(doc);
    return JSON.parse(json) as FoldRange[];
  }

  getDocumentSymbolsDoc(doc: DocumentId): DocumentSymbol[] {
    const json = this.session.document_symbols_doc(doc);
    return JSON.parse(json) as DocumentSymbol[];
  }

  convertElementDoc(doc: DocumentId, offset: number, target: ConvertTarget): TextEdit | null {
    const json = this.session.convert_element_doc(doc, offset, target);
    const result = JSON.parse(json);
    return result ?? null;
  }

  formatDocumentDoc(doc: DocumentId): string {
    const json = this.session.format_document_doc(doc);
    return JSON.parse(json) as string;
  }

  listFiles(): ProjectFile[] {
    const json = this.session.list_files();
    return JSON.parse(json) as ProjectFile[];
  }

  getFileSource(path: string): string | null {
    const json = this.session.get_file_source(path);
    const result = JSON.parse(json);
    return result ?? null;
  }

  /**
   * Whether `path` currently resolves to a mounted stdlib copy (issue #2306,
   * ruled 2026-08-06 "Mounted stdlib presents as a read-only library node") —
   * `false` for an unknown path, an ordinary project file, or a stdlib key a
   * real project file has already shadowed. Session-level enforcement query:
   * `updateDocument`/auto-import-apply on a mounted file's handle are
   * refused by the wasm side regardless of this check, but bulk-edit callers
   * that write through {@link updateFile} (which stays unguarded — it is
   * also the legitimate shadowing API) — like `ProjectSession.applyEdit` —
   * must consult this first to avoid silently forking the library into the
   * project. {@link updateSource}, the singleton-session sibling of
   * `updateFile`, is *also* left unguarded (no in-repo caller drives it, but
   * it is published surface) — a known, disclosed gap, not something this
   * query closes.
   */
  isReadOnly(path: string): boolean {
    return this.session.is_read_only(path);
  }

  getFileSymbols(path: string): DocumentSymbol[] {
    const json = this.session.file_symbols(path);
    return JSON.parse(json) as DocumentSymbol[];
  }

  compileProject(entry: string): CompileResult {
    const json = this.session.compile_project(entry);
    return JSON.parse(json) as CompileResult;
  }

  // ── Wasm-internal perf counters (measure-first ruling, 2026-08-24) ──

  /**
   * Enable/disable the wasm-internal perf counters. Off by default; the
   * studio's dev edge turns them on alongside its JS-side probe so the
   * boundary spans (`wasm.<method>`) can be decomposed into internal
   * phases (`ide.updateSource`, `ide.analyze` — the incremental db pull; the
   * pre-option-A `ide.snapshotClone`/`ide.applyAnalysis` names retired
   * with the off-db road, 2026-08-24).
   */
  setPerfEnabled(on: boolean): void {
    this.session.set_perf_enabled(on);
  }

  /** The internal counters: `{ [name]: { count, totalMs, maxMs } }`. */
  getPerfCounters(): PerfCounters {
    return JSON.parse(this.session.perf_counters_json()) as PerfCounters;
  }

  /** Clear the internal counters (scenario boundaries). */
  resetPerfCounters(): void {
    this.session.perf_reset();
  }

  /**
   * The #2885 revision-stamp experiment: two back-to-back compiles with
   * zero edits between them, returning `[firstMs, secondMs]`. Warm salsa
   * memoization would make the second near-free; the hypothesis under test
   * is that `compile`'s unconditional options write cold-prices every
   * editor compile — in which case the two are priced alike.
   */
  perfCompileProbe(entry: string): [number, number] {
    return JSON.parse(this.session.perf_compile_probe(entry)) as [number, number];
  }

  /**
   * The content lines of `path` (`knot` or `knot.stitch`) across the
   * project (#3408), or `null` when nothing declares it. Feature-detected
   * on the raw session: stubs that predate the query read as "no passage".
   */
  passageLines(path: string): PassageLine[] | null {
    const raw = this.session as { passage_lines?: (path: string) => string };
    if (typeof raw.passage_lines !== "function") return null;
    const json = raw.passage_lines(path);
    return json === "null" ? null : (JSON.parse(json) as PassageLine[]);
  }

  getProjectOutline(): FileOutline[] {
    const json = this.session.project_outline();
    return JSON.parse(json) as FileOutline[];
  }

  /**
   * Project-relative paths of the current compile closure (#3017) — the
   * exact file set codegen builds from, keyed by the entry the most recent
   * `compileProject` set. Empty before any compile. A file
   * {@link getProjectOutline} lists that is absent here is on disk but NOT
   * in the story — the out-of-scope editor banner and the Binder's "not
   * included" marks read exactly this difference. Read-only (never
   * perturbs the entry), so call it right after a compile for free.
   */
  getCompilationClosure(): string[] {
    const json = this.session.compilation_closure();
    return JSON.parse(json) as string[];
  }

  /**
   * Project-relative paths that are DRAFTS (#3145) — files matching a
   * `[project] drafts` glob that are also outside the compile closure.
   * Sorted; empty before any compile, and empty when `brink.toml` sets no
   * `drafts`.
   *
   * Both halves of that definition are applied on the Rust side on
   * purpose. Do not reconstruct draft status here by intersecting
   * {@link getCompilationClosure} with a glob list — the ruling
   * ("reachability wins", 2026-08-27) has exactly one implementation, and
   * a second one in TS would be free to drift from it.
   */
  getDraftPaths(): string[] {
    const json = this.session.draft_paths();
    return JSON.parse(json) as string[];
  }

  /**
   * Per-glob attribution for the `[project] drafts` list (#3145) — what
   * each glob the author wrote is actually doing.
   *
   * {@link getDraftPaths} answers "which files are drafts"; this answers
   * "did the glob I typed work", which a settings list has to show and
   * cannot get from the first. Two ordinary mistakes are invisible without
   * it: a glob that matches nothing (a typo) looks exactly like one that is
   * working, and a glob matching a file the entry still reaches produces no
   * draft at all under the "reachability wins" ruling — that file comes back
   * in `inStory` instead, so the view can say why.
   *
   * `compiled` is false before the first compile, when every list is empty
   * because nothing is known yet rather than because nothing matched.
   */
  getDraftGlobReport(): DraftGlobReport {
    const json = this.session.draft_glob_report();
    const raw = JSON.parse(json) as {
      compiled: boolean;
      globs: { glob: string; drafts: string[]; in_story: string[] }[];
    };
    return {
      compiled: raw.compiled,
      globs: raw.globs.map((g) => ({
        glob: g.glob,
        drafts: g.drafts,
        inStory: g.in_story,
      })),
    };
  }

  /**
   * The project's own proper nouns, for the prose checker's dictionary
   * (#3210) — declared names plus the cue names that say who the story's
   * characters are, split into words and sorted.
   *
   * Computed on the Rust side, like {@link getDraftPaths}, and for the same
   * reason: the cast lives in the dialect classification, which is analysis
   * output. Reconstructing it here from the outline would miss every
   * character whose name appears only in a cue line — the case the whole
   * feature turns on.
   */
  getProseDictionary(): string[] {
    const json = this.session.prose_dictionary();
    return JSON.parse(json) as string[];
  }

  /**
   * Whole-project story graph (studio-shell spec §4.1): knot/stitch nodes
   * plus END/DONE pseudo-nodes, and divert/choice/tunnel/thread edges. Each
   * edge lists the source occurrences (divert sites, UTF-16 spans) that
   * produced it (#371). Deterministically ordered; recomputed per call
   * (call after a compile, like the outline). Returns null when no analysis
   * is available.
   */
  getStoryGraph(): StoryGraph | null {
    const json = this.session.story_graph();
    const result = JSON.parse(json);
    return (result as StoryGraph | null) ?? null;
  }

  getLineContexts(): LineContext[] {
    const json = this.session.line_contexts();
    return JSON.parse(json) as LineContext[];
  }

  getSemanticTokens(): SemanticToken[] {
    const json = this.session.semantic_tokens();
    return JSON.parse(json) as SemanticToken[];
  }

  getCompletions(offset: number): CompletionItem[] {
    const json = this.session.completions(offset);
    return JSON.parse(json) as CompletionItem[];
  }

  getHover(offset: number): HoverInfo | null {
    const json = this.session.hover(offset);
    const result = JSON.parse(json);
    return result ?? null;
  }

  /**
   * Explain what would match the line containing `offset` in the active
   * file (issue #2113): is it matched, by what handler, what did it bind,
   * and — on a miss — what patterns were attempted, or — on a hit — what
   * else matched but was shadowed.
   *
   * Every range on the returned {@link ExplainMatch} (handler declaration
   * ranges, capture ranges) is a **raw byte offset**, not UTF-16 — unlike
   * every other DTO this class returns — and is **file-absolute**, not
   * relative to a fragment view set by {@link setViewContext}/
   * {@link openFragment}. A caller under a fragment view cannot map these
   * ranges back into its own document as-is; see
   * `crates/brink-web/src/editor/explain_match.rs`'s own module doc for why.
   *
   * `winner.kind` (issue #2310) is the claimed line's compile-time
   * structural shape, read live off this file's compiled `HirFile` (a
   * salsa query recomputed off the current revision on every edit — not a
   * snapshot that can go stale). It can still be absent (`undefined`) on a
   * hit: `path` is an ink-dialect file (which never populates a compiled
   * element record at all), nothing has compiled this file yet, or the
   * live match above claimed a line the compiler declined to record its
   * own claim for (a heading carrying a `[slug]`/tags, or a line folded
   * into a block handler's captured run). It is never a guess.
   *
   * `"content_line"`, `"scene_heading"`, `"cue"`, and `"parenthetical"` are
   * all reachable (issue #2351 fixed the node-selection mismatch that used
   * to keep `"cue"`/`"parenthetical"` from ever surfacing here).
   * `"bang_dispatch"` is declared in the type for completeness but cannot
   * yet surface here — see #2352 — because a `!name` line's claim-candidate
   * node is a `DISPATCH_NAME`/fused-content pair the claiming path never
   * runs `try_claim` against; see
   * `crates/brink-web/src/editor/explain_match.rs`'s own module doc.
   */
  explainMatch(offset: number): ExplainMatch | null {
    const json = this.session.explain_match(offset);
    const result = JSON.parse(json);
    return result ?? null;
  }

  gotoDefinition(offset: number): Location | null {
    const json = this.session.goto_definition(offset);
    const result = JSON.parse(json);
    return result ?? null;
  }

  findReferences(offset: number): Location[] {
    const json = this.session.find_references(offset);
    return JSON.parse(json) as Location[];
  }

  /**
   * Find all references at an explicit file path + offset, with control over
   * whether the declaration itself is included. Document-agnostic: resolves the
   * file by `path` rather than the active document.
   */
  findReferencesAt(
    path: string,
    offset: number,
    includeDeclaration: boolean,
  ): Location[] {
    const json = this.session.find_references_at(
      path,
      offset,
      includeDeclaration,
    );
    return JSON.parse(json) as Location[];
  }

  /**
   * `findReferencesAt`, with each site classified by how it uses the
   * symbol (`decl`/`call`/`divert`/`read`/`write` — the Search panel's
   * per-card badges, docs/search-results-cards-spec.md PR E).
   */
  findReferencesWithKindsAt(
    path: string,
    offset: number,
    includeDeclaration: boolean,
  ): LocationWithKind[] {
    const json = this.session.find_references_with_kinds_at(
      path,
      offset,
      includeDeclaration,
    );
    return JSON.parse(json) as LocationWithKind[];
  }

  /**
   * Find all references to a symbol identified by its canonical name. Returns
   * an empty array if the name is unknown or ambiguous.
   */
  referencesToSymbol(
    symbolName: string,
    includeDeclaration: boolean,
  ): Location[] {
    const json = this.session.references_to_symbol(
      symbolName,
      includeDeclaration,
    );
    return JSON.parse(json) as Location[];
  }

  prepareRename(offset: number): Location | null {
    const json = this.session.prepare_rename(offset);
    const result = JSON.parse(json);
    return result ?? null;
  }

  getCodeActions(offset: number): CodeAction[] {
    const json = this.session.code_actions(offset);
    return JSON.parse(json) as CodeAction[];
  }

  /**
   * Apply a code action selected from `getCodeActions`. Pass the action's
   * `data` payload back verbatim; `offset` is the cursor the action was offered
   * at. Returns a `StructuralResult` (`new_source` plus any `cross_file_edits`), or
   * `ok: false` with an `error` for malformed data or a no-op action.
   */
  resolveCodeAction(data: CodeActionData, offset: number): StructuralResult {
    this.bump();
    const json = this.session.resolve_code_action(JSON.stringify(data), offset);
    return JSON.parse(json) as StructuralResult;
  }

  /**
   * The auto-fixes offered for the diagnostics under `offset`
   * (`docs/autofix-spec.md` §7). Distinct from `getCodeActions`, which offers
   * structural refactors keyed off the *syntax* at the cursor: a `Fix` is
   * keyed off a *diagnostic* and carries its own minimal edits, which may
   * land in other files.
   */
  getFixes(offset: number): Fix[] {
    const json = this.session.fixes_at(offset);
    return JSON.parse(json) as Fix[];
  }

  /**
   * Turn a chosen fix (a `Fix` from `getFixes`, passed back verbatim) into
   * the sources to write: a `StructuralResult` with `new_source` for the
   * active file plus a `cross_file_edits` entry per other file the fix
   * touches. Side-effect-free — apply it through the host's own apply seam.
   */
  applyFix(fix: Fix): StructuralResult {
    this.bump();
    const json = this.session.apply_fix(JSON.stringify(fix));
    return JSON.parse(json) as StructuralResult;
  }

  getInlayHints(start: number, end: number): InlayHint[] {
    const json = this.session.inlay_hints(start, end);
    return JSON.parse(json) as InlayHint[];
  }

  getSignatureHelp(offset: number): SignatureInfo | null {
    const json = this.session.signature_help(offset);
    const result = JSON.parse(json);
    return result ?? null;
  }

  getFoldingRanges(): FoldRange[] {
    const json = this.session.folding_ranges();
    return JSON.parse(json) as FoldRange[];
  }

  getDocumentSymbols(): DocumentSymbol[] {
    const json = this.session.document_symbols();
    return JSON.parse(json) as DocumentSymbol[];
  }

  getFileIncludes(path: string): IncludeInfo[] {
    const json = this.session.file_includes(path);
    return JSON.parse(json) as IncludeInfo[];
  }

  formatDocument(): string {
    const json = this.session.format_document();
    return JSON.parse(json) as string;
  }

  convertElement(offset: number, target: ConvertTarget): TextEdit | null {
    const json = this.session.convert_element(offset, target);
    const result = JSON.parse(json);
    return result ?? null;
  }

  /** Reorder a stitch within its knot. direction: 1 = down, -1 = up. */
  reorderStitch(path: string, knot: string, stitch: string, direction: number): StructuralResult {
    this.bump();
    const json = this.session.reorder_stitch(path, knot, stitch, direction);
    return JSON.parse(json) as StructuralResult;
  }

  /** Reorder a knot within the top-level knot list. direction: 1 = down, -1 = up. */
  reorderKnot(path: string, knot: string, direction: number): StructuralResult {
    this.bump();
    const json = this.session.reorder_knot(path, knot, direction);
    return JSON.parse(json) as StructuralResult;
  }

  /**
   * Reorder all stitches in a knot to match `order` (a permutation of the
   * knot's stitch names). Used by drag-and-drop, which knows the full
   * destination order, and by multi-select moves.
   */
  reorderStitches(path: string, knot: string, order: string[]): StructuralResult {
    this.bump();
    const json = this.session.reorder_stitches(path, knot, order);
    return JSON.parse(json) as StructuralResult;
  }

  /** Reorder all top-level knots to match `order` (a permutation of the knot names). */
  reorderKnots(path: string, order: string[]): StructuralResult {
    this.bump();
    const json = this.session.reorder_knots(path, order);
    return JSON.parse(json) as StructuralResult;
  }

  /** Move a stitch from one knot to another. */
  moveStitch(path: string, srcKnot: string, stitch: string, destKnot: string): StructuralResult {
    this.bump();
    const json = this.session.move_stitch(path, srcKnot, stitch, destKnot);
    return JSON.parse(json) as StructuralResult;
  }

  /**
   * Rename or move a file, rewriting every `INCLUDE` that points at it plus
   * the moved file's own relative includes. `new_source` is the moved file's
   * content (write it at `newPath`); `cross_file_edits` carry the referencing
   * files' rewrites. The op computes edits only — the caller applies them
   * (write `newPath`, remove `oldPath`).
   */
  renameFile(oldPath: string, newPath: string): StructuralResult {
    this.bump();
    const json = this.session.rename_file(oldPath, newPath);
    return JSON.parse(json) as StructuralResult;
  }

  /**
   * Atomically rename or move a directory (#314): relocate every file under
   * `oldPrefix` to `newPrefix`, rewriting all affected `INCLUDE`s against a
   * single pre-move snapshot (moved files' outbound includes, inbound includes
   * from files outside the folder, and intra-folder sibling includes — all
   * mutually consistent, unlike a per-file rename loop). `moved_files` are the
   * relocated files (write each `new_source` at `new_path`, remove `old_path`);
   * `cross_file_edits` carry the outside referrers' rewrites. `safe` +
   * `introduced_diagnostics` are the safe-by-default breakage gate. The op
   * computes edits only — the caller applies them.
   */
  renameDir(oldPrefix: string, newPrefix: string): DirMoveResult {
    this.bump();
    const json = this.session.rename_dir(oldPrefix, newPrefix);
    return JSON.parse(json) as DirMoveResult;
  }

  /** Promote a stitch to a top-level knot. */
  promoteStitch(path: string, knot: string, stitch: string): StructuralResult {
    this.bump();
    const json = this.session.promote_stitch(path, knot, stitch);
    return JSON.parse(json) as StructuralResult;
  }

  /** Demote a top-level knot to a stitch inside another knot. */
  demoteKnot(path: string, knot: string, destKnot: string): StructuralResult {
    this.bump();
    const json = this.session.demote_knot(path, knot, destKnot);
    return JSON.parse(json) as StructuralResult;
  }

  /**
   * Delete a knot (`stitch` omitted) or a stitch, safe-by-default (#316).
   * Removes the knot's whole region (header, body, nested stitches) or the named
   * stitch's region, then runs the breakage gate: every divert / thread /
   * tunnel / call that targeted the removed symbol now dangles, surfacing in
   * `introduced_diagnostics`. When `safe`, apply directly via `applyMoveResult`;
   * otherwise show the breakage report and apply only on an explicit force.
   */
  deleteSymbol(path: string, knot: string, stitch?: string): StructuralResult {
    this.bump();
    const json = this.session.delete_symbol(path, knot, stitch ?? "");
    return JSON.parse(json) as StructuralResult;
  }

  /**
   * Rename a knot (`stitch` omitted) or a stitch, safe-by-default. The result
   * is a `StructuralResult` superset: when `safe` it can be applied directly via
   * `applyMoveResult`; otherwise `introduced_diagnostics` carries the breakage
   * report and the caller applies the (already-computed) edits only on force.
   */
  renameSymbol(path: string, knot: string, stitch: string, newName: string): StructuralResult {
    this.bump();
    const json = this.session.rename_symbol(path, knot, stitch, newName);
    return JSON.parse(json) as StructuralResult;
  }

  /**
   * Offset-based sibling of {@link renameSymbol}, used by the editor's F2 to
   * rename any symbol under the cursor (not just knots/stitches). `offset` is a
   * whole-file UTF-16 offset (fold in any fragment-view origin first). Same
   * safe-by-default `StructuralResult`.
   */
  renameSymbolAt(path: string, offset: number, newName: string): StructuralResult {
    this.bump();
    const json = this.session.rename_symbol_at(path, offset, newName);
    return JSON.parse(json) as StructuralResult;
  }

  /**
   * Extract the selected lines into a new top-level `=== name ===` knot,
   * replacing the selection with a tunnel call `-> name ->` (#315 H).
   * `startOffset`/`endOffset` are whole-file UTF-16 offsets; the selection is
   * snapped to whole lines and the new knot is appended at end of file (ending
   * with a `->->` tunnel return). The result is a safe-by-default
   * `StructuralResult`: when `safe`, apply directly via `applyMoveResult`;
   * otherwise `introduced_diagnostics` reports the weave/gather/local scope the
   * extraction would break, and the caller applies only on an explicit force.
   */
  extractToKnot(
    path: string,
    startOffset: number,
    endOffset: number,
    name: string,
  ): StructuralResult {
    this.bump();
    const json = this.session.extract_to_knot(path, startOffset, endOffset, name);
    return JSON.parse(json) as StructuralResult;
  }

  /**
   * Extract the selected lines into a new `=== function name() ===`, replacing
   * the selection with the call — `{name()}` for a single value expression,
   * `~ name()` for a statement (#315 H). Same offset/gate semantics as
   * {@link extractToKnot}.
   */
  extractToFunction(
    path: string,
    startOffset: number,
    endOffset: number,
    name: string,
  ): StructuralResult {
    this.bump();
    const json = this.session.extract_to_function(path, startOffset, endOffset, name);
    return JSON.parse(json) as StructuralResult;
  }

  free(): void {
    this.session.free();
  }
}

// ── Story runner ────────────────────────────────────────────────

/** A value that can cross the ink↔JS external-binding boundary. Re-exported
 * from the wasm-free `evaluate-dispatch` module (single source of truth). */
// ── ClassifierSession (docs/editor-worker-spec.md §4, W3) ──────────

/** The raw wasm classifier surface — probed at runtime (see the namespace
 *  import note above). */
interface RawClassifierSession {
  open(path: string, source: string): boolean;
  update_source(source: string): boolean;
  apply_edits(editsJson: string): boolean;
  segment_manifest(): string;
  segment_line_contexts(key: string): string;
  segment_semantic_tokens_fast(key: string): string;
  set_dialect(json: string): void;
  clear_dialect(): void;
  set_language_dialect(value: string): void;
  free(): void;
}

/**
 * The capability-stripped main-thread session (editor worker architecture
 * W3): one open document's segment substrate — per-segment lex/parse/lower,
 * classifier tokens, line contexts — plus the classification-affecting
 * config surface. It NEVER runs project analysis (the Rust type's write
 * paths are analysis-free by construction), and the exported surface is
 * the capability boundary: no project method exists to call.
 *
 * `available` is false on an older wasm build or a test mock — consumers
 * skip the classifier road entirely and keep the full-session road.
 */
export class ClassifierSessionHandle {
  private session: RawClassifierSession | null;
  private configEpochCounter = 0;

  constructor() {
    const ctor = (brinkWebModule as { ClassifierSession?: new () => RawClassifierSession })
      .ClassifierSession;
    this.session = typeof ctor === "function" ? new ctor() : null;
  }

  get available(): boolean {
    return this.session !== null;
  }

  /** Bumped on every config mutation — slice caches key on it (same
   *  contract as `EditorSessionHandle.configEpoch`). */
  configEpoch(): number {
    return this.configEpochCounter;
  }

  /** Open (or replace) THE document this classifier serves. */
  open(path: string, source: string): boolean {
    return this.session?.open(path, source) ?? false;
  }

  /** Full-text push (the delta path's fallback). Never analyzes. */
  updateSource(source: string): boolean {
    return this.session?.update_source(source) ?? false;
  }

  /** Bounded edit list — same shape as `applyEditsDocument`. */
  applyEdits(edits: readonly EditSpan[]): boolean {
    if (this.session === null) return false;
    return this.session.apply_edits(JSON.stringify(edits));
  }

  getSegmentManifest(): SegmentManifest | null {
    if (this.session === null) return null;
    return JSON.parse(this.session.segment_manifest()) as SegmentManifest | null;
  }

  getSegmentLineContexts(key: string): LineContext[] | null {
    if (this.session === null) return null;
    return JSON.parse(this.session.segment_line_contexts(key)) as LineContext[] | null;
  }

  getSegmentSemanticTokensFast(key: string): SemanticToken[] | null {
    if (this.session === null) return null;
    return JSON.parse(
      this.session.segment_semantic_tokens_fast(key),
    ) as SemanticToken[] | null;
  }

  setDialect(dialect: DialogueDialect): void {
    this.session?.set_dialect(JSON.stringify(dialect));
    this.configEpochCounter += 1;
  }

  clearDialect(): void {
    this.session?.clear_dialect();
    this.configEpochCounter += 1;
  }

  setLanguageDialect(value: "brink" | "strict-ink"): void {
    this.session?.set_language_dialect(value);
    this.configEpochCounter += 1;
  }

  free(): void {
    this.session?.free();
    this.session = null;
  }
}

export type { ExternalValue } from "./evaluate-dispatch";

/** An external-function binding: receives the call arguments as native JS
 * values and returns a value (or nothing) back to the story. May be async —
 * return a Promise and the story suspends until it resolves (drive with
 * `continueAsync`/`continueSingleAsync`). */
export type ExternalFn = (
  ...args: ExternalValue[]
) => ExternalValue | void | Promise<ExternalValue | void>;

export class StoryRunnerHandle {
  private runner: StoryRunner;

  /** Tier-1 `evaluate()` fragment-compile cache (F5.1): compile once per
   * distinct fragment source per program version, then every re-eval is a
   * cache hit. Keyed by `${checksum}\0${fragmentSource}` — see
   * `compileFragment` below. Bounded (`FRAGMENT_CACHE_LIMIT`, FIFO eviction)
   * so a long-lived runner fed many distinct one-off watches can't grow this
   * without bound. */
  private fragmentCache = new Map<string, FragmentCompileEntry>();

  constructor(storyBytes: Uint8Array) {
    this.runner = new StoryRunner(storyBytes);
  }

  /** Bind an ink `EXTERNAL <name>(...)` to a synchronous JS callback.
   * Re-binding the same name replaces the previous callback. */
  bindExternal(name: string, fn: ExternalFn): void {
    this.runner.bind_external(name, fn);
  }

  /** Remove a previously registered external binding. */
  unbindExternal(name: string): void {
    this.runner.unbind_external(name);
  }

  /** When `true`, an unbound external resolves to `null` instead of falling
   * through to its ink fallback body / erroring. Default `false`. */
  setLenientUnbound(lenient: boolean): void {
    this.runner.set_lenient_unbound(lenient);
  }

  /** Read a global ink variable by name. `undefined` if no such variable is
   * declared, `null` if it exists and holds null. */
  getVar(name: string): ExternalValue | undefined {
    return this.runner.get_var(name) as ExternalValue | undefined;
  }

  /** Set a global ink variable by name. Returns `false` if no such variable
   * is declared. */
  setVar(name: string, value: ExternalValue): boolean {
    return this.runner.set_var(name, value);
  }

  /** Set the RNG seed for reproducible `RANDOM`/shuffle output. Applies now
   * and is re-applied across `reset()`. Set before the first continue for a
   * fully deterministic playthrough. */
  setSeed(seed: number): void {
    this.runner.set_seed(seed);
  }

  /**
   * Enable the dev-tooling visibility override (M-2b, play-from-here). When
   * `allow` is `true`, host semantic access to `#@private` definitions —
   * `getVar`/`setVar`, `goToPath`/`runKnot`, `callFunction` — is permitted:
   * enforcement is turned off. Editors and debug hosts set this to start
   * flows at private knots and inspect private state; production hosts leave
   * it `false` (the default) to respect visibility. Applies now and is
   * re-applied across `reset()`/`reload()`. A host capability, not a language
   * switch — the compiled program is identical either way.
   */
  setDevVisibilityOverride(allow: boolean): void {
    this.runner.setDevVisibilityOverride(allow);
  }

  /** Capture durable game state as a typed object (dev/inspectable). */
  save(): SaveState {
    return JSON.parse(this.runner.save()) as SaveState;
  }

  /** Capture durable game state as a compact MessagePack blob (release). */
  saveBytes(): Uint8Array {
    return this.runner.save_bytes();
  }

  /** Reconcile a saved state into the running story; returns what couldn't be
   * applied (empty `unknown_globals`/`unresolved_renames` and zero
   * `anonymous_states_dropped` = clean). Tolerant of story patches. */
  load(state: SaveState): LoadReport {
    return JSON.parse(this.runner.load(JSON.stringify(state))) as LoadReport;
  }

  /** Reconcile a MessagePack blob from `saveBytes()`. */
  loadBytes(bytes: Uint8Array): LoadReport {
    return JSON.parse(this.runner.load_bytes(bytes)) as LoadReport;
  }

  /** Evaluate an ink function from the host (engine→ink), out-of-band: the
   * visible story is untouched. Externals it calls resolve through registered
   * synchronous bindings. Returns the function's value. */
  callFunction(name: string, ...args: ExternalValue[]): ExternalValue {
    return this.runner.call_function(name, args) as ExternalValue;
  }

  // ── Primary-flow drive (documented sugar) ────────────────────────
  //
  // FS-3w (`docs/flow-suspension-spec.md` §10.1): `continue` lives on the
  // flow, not the story. These story-level drive methods
  // (`continueStory`/`continueSingle`/`continueStoryAsync`/…) are
  // documented **sugar for the primary flow** — the always-present default
  // flow this runner was constructed with. Spawned/ambient flows are
  // addressable via `flow(name)` / `continueFlow(name)`, each with its own
  // `Line` stream. Nothing here changes behavior: the primary flow is what
  // every prior consumer has always been driving.

  /** Drive the **primary flow** to its next pause, returning every `Line`
   * up to and including the terminal one. Documented sugar for the default
   * flow (see the section note above); `flow(name).continueMaximally()` is
   * the per-flow equivalent for a spawned flow. */
  continueStory(): Line[] {
    const json = this.runner.continue_story();
    return JSON.parse(json) as Line[];
  }

  /** Produce one `Line` from the **primary flow**. Documented sugar for the
   * default flow (see the section note above). */
  continueSingle(): Line {
    const json = this.runner.continue_single();
    return JSON.parse(json) as Line;
  }

  /** Continue maximally, awaiting any async (Promise-returning) bindings. Use
   * this instead of `continueStory` when bindings may be async. */
  async continueStoryAsync(): Promise<Line[]> {
    const lines: Line[] = [];
    for (;;) {
      const line = await this.advanceAwaiting();
      if (line.type === "text") {
        lines.push(line);
        continue;
      }
      lines.push(line); // terminal: done | choices | end
      return lines;
    }
  }

  /** Produce one line, awaiting any async binding hit along the way. */
  async continueSingleAsync(): Promise<Line> {
    return this.advanceAwaiting();
  }

  // ── Low-level async primitives (for custom drive loops) ──────────
  // `continueStoryAsync`/`continueSingleAsync` are the ergonomic path; these
  // expose the raw park/resolve so a host can drive it manually.

  /** Advance one step; the line may be `{ type: "awaiting_external" }`. */
  advanceOne(): Line {
    return JSON.parse(this.runner.advance_one()) as Line;
  }

  /** Take the suspended async binding's Promise to await; `undefined` if none. */
  takePendingPromise(): Promise<ExternalValue> | undefined {
    const p = this.runner.take_pending_promise();
    return p === undefined ? undefined : (p as Promise<ExternalValue>);
  }

  /** Resolve the parked external with a value (the awaited Promise result). */
  resolveExternal(value: ExternalValue): void {
    this.runner.resolve_external(value);
  }

  /** Step until a real line, transparently awaiting+resolving any suspended
   * async binding (a Promise returned by a `bindExternal` callback). On a
   * rejected Promise, resolves the external with `null` to unstick the flow,
   * then rethrows so the host sees the failure. */
  private async advanceAwaiting(): Promise<Line> {
    for (;;) {
      const line = JSON.parse(this.runner.advance_one()) as Line;
      if (line.type !== "awaiting_external") {
        return line;
      }
      const promise = this.runner.take_pending_promise() as Promise<ExternalValue>;
      let value: ExternalValue;
      try {
        value = await promise;
      } catch (err) {
        this.runner.resolve_external(null); // unstick the parked flow
        throw err;
      }
      this.runner.resolve_external(value ?? null);
    }
  }

  choose(index: number): void {
    this.runner.choose(index);
  }

  /** Move the play head to a knot/stitch path (`"knot"` / `"knot.stitch"`) —
   * ink's `ChoosePathString` equivalent; subsequent `continue*` runs from
   * there. The session keeps its state: variables and visit counts survive,
   * and the jump itself counts as a visit (like a `-> path` divert). Pending
   * choices are abandoned; the transcript so far is kept. Throws on an
   * unknown path, or if the story is parked on an unresolved async external
   * (resolve it — or `reset()` — first).
   *
   * Pass `args` to enter a **parameterized** knot (`=== call(action, present)
   * ===`) with its declared parameters bound from the supplied values. Throws
   * if the argument count doesn't match the knot's declared parameters. */
  goToPath(path: string, ...args: ExternalValue[]): void {
    if (args.length === 0) {
      this.runner.go_to_path(path);
    } else {
      this.runner.go_to_path_with_args(path, args);
    }
  }

  /** Convenience alias for entering a parameterized knot by name with bound
   * arguments — `runKnot("call", "wave", true)` ≡ `goToPath("call", "wave",
   * true)`. */
  runKnot(name: string, ...args: ExternalValue[]): void {
    this.goToPath(name, ...args);
  }

  reset(): void {
    this.runner.reset();
  }

  /** Hot-reload a freshly compiled program **in place**, preserving the
   * session's external bindings, RNG seed, and replay recording, then reset
   * the play head to the start. Follow with `beginReplay()`, a silent re-walk
   * of the saved choice log, and `endReplay()` to restore position with
   * faithful externals (query-gated branches reproduce, effects don't
   * re-fire). Throws on decode/link failure — the old program keeps running. */
  reload(storyBytes: Uint8Array): void {
    this.runner.reload(storyBytes);
  }

  /** Enter replay mode and reset the replay cursor: visible playback
   * (`continueStory`/`continueSingle`/`advanceOne`) serves externals from the
   * recording and re-runs nothing. Bracket the post-`reload` choice re-walk
   * with this and `endReplay()`. */
  beginReplay(): void {
    this.runner.begin_replay();
  }

  /** Leave replay mode: visible playback resumes invoking bindings and
   * recording their results (appending to the existing log). */
  endReplay(): void {
    this.runner.end_replay();
  }

  /** Whether any external has been recorded this session — i.e. whether a
   * post-`reload` re-walk should `beginReplay()` (serve recorded externals)
   * or run live (a fresh load has nothing recorded yet). */
  hasRecording(): boolean {
    return this.runner.has_recording();
  }

  /** Whether the last execution cycle of the default flow ended with a safe
   * exit (an explicit `-> DONE`), as opposed to running out of content.
   * Both deliver a `done`-type line; read this right after one to tell
   * them apart — `false` means the next `continueStory`/`continueSingle`/
   * `advanceOne` call will throw instead of returning more text. `false`
   * if no story is loaded. Reflects only the default flow, not flows
   * spawned/continued via `spawnFlow`/`continueFlow`/
   * `continueFlowMaximally` (issue #1573). */
  didSafeExit(): boolean {
    return this.runner.did_safe_exit();
  }

  /** Structured, name-resolved snapshot of the runtime's current state. */
  debugSnapshot(): DebugState {
    return JSON.parse(this.runner.debug_snapshot()) as DebugState;
  }

  /** The compiled program as `.inkt` text (Program Explorer raw toggle). */
  programInkt(): string {
    return this.runner.program_inkt();
  }

  /** Structured model of the compiled program (Program Explorer). */
  programModel(): ProgramModel {
    return JSON.parse(this.runner.program_model()) as ProgramModel;
  }

  /** The compiler's line table for host-side analysis (#366): text + source
   * span (file/line), project-wide (`INCLUDE`s already resolved by the
   * compile). First consumer: cast detection (pair with `detectCast` from
   * `@brink-lang/editor`) feeding a speaker-color settings surface; the same
   * exposure serves per-speaker word counts and the #362 line-fit metrics
   * epic. Static for the loaded program (no running `Story` required). */
  linesTable(): LinesTable {
    return JSON.parse(this.runner.lines_table()) as LinesTable;
  }

  /** The source-identity checksum of the currently loaded program
   * (`"0x{:08x}"`) — identical to {@link programChecksum}, but read off the
   * already-linked program (survives `reload`). Used by `evaluate()`'s
   * Tier-1 fragment cache to key a compiled fragment to the program version
   * it was compiled against. */
  checksum(): string {
    return this.runner.checksum();
  }

  /**
   * The program→source resolver (D9, issue #3187): resolves a
   * `(containerIdx, offset)` bytecode position — exactly what
   * {@link debugSnapshot}'s `position`/call-stack frame `position` fields
   * report — to the source range it was compiled from, via the loaded
   * program's `DebugInfo` section (D6, #3184).
   *
   * Returns `null`, not a throw, when the program carries no `DebugInfo`
   * section (a compile without `--debug-info`) or the position doesn't
   * resolve — this is the expected shape for most builds, not a fault.
   *
   * Callers MUST gate this on program identity before trusting the result
   * for anything source-position-sensitive: this resolves against the
   * program THIS runner is executing, which can be stale relative to the
   * studio's latest compile. `docs/live-inspector-spec.md` §5's
   * `sessionDegraded(programChecksum, compiledChecksum)` is the gate —
   * compare {@link checksum} against the current compile's checksum first.
   */
  resolveDebugPosition(containerIdx: number, offset: number): DebugSourceLocation | null {
    return JSON.parse(
      this.runner.resolve_debug_position(containerIdx, offset),
    ) as DebugSourceLocation | null;
  }

  // ── Source→program resolvers (W2/#3295) — parity with
  // `StorySessionHandle`'s copies below; see them for the contracts. ──

  /** See `StorySessionHandle.resolveSourceRange`. */
  resolveSourceRange(file: string, start: number, end: number): ProgramAddress | null {
    return JSON.parse(
      this.runner.resolve_source_range(file, start, end),
    ) as ProgramAddress | null;
  }

  /** See `StorySessionHandle.resolveSourceLine` (`line` is 0-based). */
  resolveSourceLine(file: string, line: number): ProgramAddress | null {
    return JSON.parse(this.runner.resolve_source_line(file, line)) as ProgramAddress | null;
  }

  /** The `file:line` of a bytecode position (W6/#3299): `{ file, line }`
   * (0-based) or `null`. What the execution highlight and the paused chip
   * consume; degraded-gate before trusting it, like every resolver. */
  resolveDebugLine(containerIdx: number, offset: number): DebugLine | null {
    return JSON.parse(this.runner.resolveDebugLine(containerIdx, offset)) as DebugLine | null;
  }

  /** See `StorySessionHandle.hasDebugInfo`. */
  hasDebugInfo(): boolean {
    return this.runner.has_debug_info();
  }

  /** See `StorySessionHandle.sourceMatches` (tri-state; `null` = cannot tell). */
  sourceMatches(file: string, text: string): boolean | null {
    return JSON.parse(this.runner.source_matches(file, text)) as boolean | null;
  }

  /** See `StorySessionHandle.resolvePathAddress`. */
  resolvePathAddress(path: string): ProgramAddress | null {
    return JSON.parse(this.runner.resolve_path_address(path)) as ProgramAddress | null;
  }

  // ── Debug control (D8, #3186 — the control-half wasm bridge, #3232) ──
  // Parity with `StorySessionHandle`'s copy below — `LocalSessionProvider`
  // drives `StorySessionHandle`, not this type; see that copy's doc for the
  // full contract these delegate to.

  /** Add an enabled breakpoint at `(containerIdx, offset)`, returning its
   * id — pass it to {@link debugBreakpointRemove}/
   * {@link debugBreakpointSetEnabled}. An empty/omitted `name` is replaced
   * with a `container:offset` label. */
  debugBreakpointAdd(containerIdx: number, offset: number, name?: string): number {
    return this.runner.debugBreakpointAdd(containerIdx, offset, name);
  }

  /** Remove a breakpoint by id. Returns `false` if no breakpoint with that
   * id exists. */
  debugBreakpointRemove(id: number): boolean {
    return this.runner.debugBreakpointRemove(id);
  }

  /** Enable/disable a breakpoint without removing it. Returns `false` if no
   * breakpoint with that id exists. */
  debugBreakpointSetEnabled(id: number, enabled: boolean): boolean {
    return this.runner.debugBreakpointSetEnabled(id, enabled);
  }

  /** Every breakpoint currently armed, in insertion order. */
  debugBreakpoints(): Breakpoint[] {
    return JSON.parse(this.runner.debugBreakpoints()) as Breakpoint[];
  }

  /**
   * Run the default flow forward until an armed breakpoint, a choice point,
   * or a terminal outcome (D8, #3186). `budgetCeiling` defaults to the
   * runtime's `DEFAULT_DEBUG_BUDGET` when omitted — a debug-only step
   * ceiling, entirely separate from production's step limit; exceeding it
   * throws.
   */
  debugRun(budgetCeiling?: number): DebugRunOutcome {
    return JSON.parse(this.runner.debugRun(budgetCeiling)) as DebugRunOutcome;
  }

  /**
   * Step the default flow by one {@link StepMode} unit
   * (`docs/debugger-spec.md` §4's depth-delta semantics). Same budget
   * default as {@link debugRun}.
   */
  debugStep(mode: StepMode, budgetCeiling?: number): DebugRunOutcome {
    return JSON.parse(this.runner.debugStep(mode, budgetCeiling)) as DebugRunOutcome;
  }


  /**
   * Run forward until the next **content line** is delivered (2026-08-30
   * Continue ruling — the granularity ladder's top tier), or a
   * breakpoint/choices/terminal stop comes first. The stop lands past the
   * glue/commit boundary, so the crossed line is IN this outcome's
   * `lines` — no one-advance delivery lag (#3321). Needs no debug line
   * info. Same budget default as {@link debugRun}.
   */
  debugRunToLine(budgetCeiling?: number): DebugRunOutcome {
    return JSON.parse(this.runner.debugRunToLine(budgetCeiling)) as DebugRunOutcome;
  }

  /** Step to the next **source line** (#3264, W5/#3298) — the author-tier
   * step, bounded by any armed breakpoint. Reason `noLineInfo` when the
   * artifact carries no line index. Same journal-bypass contract as
   * {@link debugStep}; the outcome carries the emitted-lines delta. */
  debugStepLine(mode: StepMode, budgetCeiling?: number): DebugRunOutcome {
    return JSON.parse(this.runner.debugStepLine(mode, budgetCeiling)) as DebugRunOutcome;
  }

  // ── Flow-addressed consumption (#200, FS-3w) ─────────────────────
  // Concurrent flows of one story that SHARE this runner's globals / visit
  // counts / rng (true ink flow semantics), each with its own call stack
  // and its own `Line` stream. Drives the studio's "+ new flow". Distinct
  // from a separate `StoryRunnerHandle`, which is an isolated playthrough.
  //
  // FS-3w (`docs/flow-suspension-spec.md` §10.1): spawned/ambient flows are
  // **addressable handles**. Use `flow(name)` for an object handle, or the
  // name-addressed methods below directly. The primary flow is driven by
  // the story-level `continue*` methods above (documented sugar).

  /** Spawn a shared-context flow at the program root (or `path`), returning
   * an addressable {@link FlowHandle} for it (FS-3w §10.1). */
  spawnFlow(name: string, path?: string): FlowHandle {
    this.runner.spawn_flow(name, path);
    return new FlowHandle(this, name);
  }

  /** An addressable handle for a spawned/ambient flow — its own `Line`
   * stream, choices, debug snapshot, and teardown. A thin, allocation-free
   * view over the name-addressed methods; does not have to exist for those
   * to work. */
  flow(name: string): FlowHandle {
    return new FlowHandle(this, name);
  }

  /** Advance a shared flow by one line (that flow's `Line` stream). */
  continueFlow(name: string): Line {
    return JSON.parse(this.runner.continue_flow(name)) as Line;
  }

  /** Drive a shared flow to its next terminal line, collecting every `Line`
   * up to and including it — the raw entry point behind
   * `flow(name).continueMaximally()`. Capped at the runtime's
   * `FlowInstance::LINE_LIMIT` (10,000 lines/turn), the same bound
   * `continueStory` enforces for the primary flow: an infinite-emitting flow
   * throws (the wasm `RuntimeError::LineLimitExceeded` surfaced as a JS
   * `Error`, matching `continueStory`'s error shape) instead of growing an
   * unbounded array and hanging the host (#999). */
  continueFlowMaximally(name: string): Line[] {
    return JSON.parse(this.runner.continue_flow_maximally(name)) as Line[];
  }

  /** Select a choice in a shared flow. */
  chooseFlow(name: string, index: number): void {
    this.runner.choose_flow(name, index);
  }

  /** Destroy a shared flow. */
  destroyFlow(name: string): void {
    this.runner.destroy_flow(name);
  }

  /** Active flow names (sorted). */
  flowNames(): string[] {
    return JSON.parse(this.runner.flow_names()) as string[];
  }

  /** Per-flow debug snapshot (State View) for a named flow. */
  flowDebugSnapshot(name: string): DebugState {
    return JSON.parse(this.runner.flow_debug_snapshot(name)) as DebugState;
  }

  /** Re-evaluate parked flows' wake conditions and return the flow ids that
   * woke (`docs/flow-suspension-spec.md` §10.2). Waking never
   * auto-continues — drive a woken flow via `continueFlow`/`flow(id)` when
   * you want its output.
   *
   * **Returns `[]` until parks exist (FS-3r).** No flow can park in today's
   * runtime — the E052 fence keeps `await` from lowering, so
   * `Line.type === "suspended"` is never produced. Exported now (FS-3w) so
   * hosts wire the wake loop against a stable shape. */
  wakeCheck(): string[] {
    return JSON.parse(this.runner.wake_check()) as string[];
  }

  // ── Speculative evaluation (F4.3, docs/speculative-eval-spec.md) ─
  // A `Speculation` is a sandboxed, side-effect-proof fork of the story's
  // current state: driving it (via its own `goToPath`/`advance`/`choose`/
  // `evalFunction`/…) never mutates this runner's live story, and nothing it
  // does survives past `free()`. `speculate()` is the composable primary
  // surface; `evaluate()` is thin sugar over it for the common cases.

  /** Fork a `SpeculationHandle`. See the class docs for its verbs, and
   * `evaluate()` below for a thin convenience over the common cases. */
  speculate(options?: SpeculationOptions): SpeculationHandle {
    return new SpeculationHandle(this.runner.speculate(JSON.stringify(options ?? {})));
  }

  /**
   * Thin convenience over `speculate()`'s composable verbs: parse `source`,
   * drive the speculation to a natural stop, and collect the result. Composes
   * the verbs — it does not hide them; reach for `speculate()` directly for
   * anything this doesn't cover (probing multiple branches, bailing out
   * early, driving choices, etc).
   *
   * `source` is either:
   * - a knot/stitch path (`"cellar.intro"`) — driven with `goToPath` +
   *   advanced to a natural stop (a `done`/`end` line, or a `choices` line,
   *   which is reported via `reachedChoices` rather than picked); or
   * - a function call with **literal** arguments (`"check(1, 2)"` — numbers,
   *   quoted strings, `true`/`false`/`null` only) — driven with `evalFunction`.
   * - anything else (an arbitrary expression like `"has(sword) && gold > 2"`,
   *   content like `"You have {gold}"`, a lone divert like `"-> cellar"`, a
   *   call with non-literal arguments) — **Tier 1**: the fragment is wrapped
   *   as a synthetic knot/function, recompiled against `opts.projectSource`
   *   (cached per fragment per program version), and run the same way over a
   *   fresh runner seeded from this one's current state (F5.1,
   *   `docs/speculative-eval-spec.md`'s "mechanism B"). Requires
   *   `opts.projectSource` — a `StoryRunner` holds no reference to the file
   *   set it was compiled from, so the caller (which does) supplies it.
   *   Without it, or if the fragment fails to compile as either an
   *   expression or content, `diagnostics` comes back non-empty and nothing
   *   runs.
   *
   * Any async (`Promise`-returning) bound external hit along the way is
   * awaited transparently, exactly like `continueStoryAsync`. Pass
   * `opts.signal` to cancel an in-flight evaluation: the speculation is
   * dropped and the promise rejects with an `AbortError`.
   */
  async evaluate(source: string, opts: EvaluateOptions = {}): Promise<SpeculationResult> {
    const parsed = parseEvaluateSource(source);
    if (parsed.kind === "invalid") {
      return this.evaluateFragment(source, opts);
    }

    throwIfAborted(opts.signal);
    const speculation = this.speculate({
      steps: opts.budget?.steps,
      lines: opts.budget?.lines,
      context: opts.context,
      liveEffects: opts.liveEffects,
      kinds: opts.kinds,
    });
    try {
      if (parsed.kind === "path") {
        speculation.goToPath(parsed.path);
        const { stop, choices } = await driveSpeculationToTerminal(speculation, opts.signal);
        return {
          transcript: speculation.transcript(),
          reachedChoices: choices,
          stop,
          externals: speculation.externalsReport(),
          diagnostics: [],
        };
      }

      const { value, stop } = await driveSpeculationCall(
        speculation,
        parsed.name,
        parsed.args,
        opts.signal,
      );
      return {
        value,
        transcript: speculation.transcript(),
        stop,
        externals: speculation.externalsReport(),
        diagnostics: [],
      };
    } finally {
      speculation.free();
    }
  }

  /**
   * Tier-1: `evaluate()`'s fallback for a `source` that isn't a bare knot
   * path or a literal-arg call. Compiles the fragment (via `compileFragment`,
   * cached) as a synthetic symbol appended to `opts.projectSource`, then runs
   * it the same way `evaluate()`'s Tier-0 branches do — a fresh
   * `StoryRunnerHandle` over the recompiled program, seeded with this
   * runner's current state (`load(this.save())`, name-keyed — globals by
   * name, visit/turn counts by content-hashed id, both stable across the
   * recompile), then `speculate()` + drive to a natural stop. The fragment
   * runner and its speculation are discarded when done; nothing it does
   * touches this runner.
   */
  private async evaluateFragment(
    source: string,
    opts: EvaluateOptions,
  ): Promise<SpeculationResult> {
    if (!opts.projectSource) {
      return {
        transcript: [],
        stop: "completed",
        externals: { live: [], fallback: [] },
        diagnostics: [
          `evaluate: "${source}" is neither a knot/stitch path nor a literal-arg function ` +
            "call, so it needs Tier-1 fragment compilation — pass opts.projectSource " +
            "({ entry, files }) with the project's current sources.",
        ],
      };
    }

    throwIfAborted(opts.signal);
    const compiled = this.compileFragment(source, opts.projectSource);
    if (!compiled.ok) {
      return {
        transcript: [],
        stop: "completed",
        externals: { live: [], fallback: [] },
        diagnostics: compiled.diagnostics,
      };
    }

    const fragmentRunner = new StoryRunnerHandle(compiled.storyBytes);
    try {
      // A fresh `StoryRunner` starts with no external bindings — copy this
      // runner's live bindings + lenient-unbound policy across first, so a
      // query/effect external the fragment touches resolves the same way it
      // would here (Tier-0's `speculate()` gets this for free by forking the
      // same runner; Tier-1's scratch runner needs it done explicitly).
      fragmentRunner.setLenientUnbound(this.runner.lenient_unbound());
      for (const name of this.runner.binding_names()) {
        const fn = this.runner.get_binding(name);
        if (fn) {
          fragmentRunner.runner.bind_external(name, fn);
        }
      }
      fragmentRunner.load(this.save());
      const speculation = fragmentRunner.speculate({
        steps: opts.budget?.steps,
        lines: opts.budget?.lines,
        context: opts.context,
        liveEffects: opts.liveEffects,
        kinds: opts.kinds,
      });
      try {
        if (compiled.kind === "expression") {
          const { value, stop } = await driveSpeculationCall(
            speculation,
            compiled.symbolName,
            [],
            opts.signal,
          );
          return {
            value,
            transcript: speculation.transcript(),
            stop,
            externals: speculation.externalsReport(),
            diagnostics: [],
          };
        }

        speculation.goToPath(compiled.symbolName);
        const { stop, choices } = await driveSpeculationToTerminal(speculation, opts.signal);
        return {
          transcript: speculation.transcript(),
          reachedChoices: choices,
          stop,
          externals: speculation.externalsReport(),
          diagnostics: [],
        };
      } finally {
        speculation.free();
      }
    } finally {
      fragmentRunner.free();
    }
  }

  /**
   * Classify + compile a Tier-1 fragment as a synthetic symbol, cached by
   * `(this program's checksum, fragmentSource)` — a fragment compiles once
   * per program version; every re-eval (e.g. a watch panel re-running on
   * every step) is a cache hit. Robust classification: try the fragment as
   * an expression (native `fn NAME() { return (FRAG); }`, ink
   * `=== function NAME() === \n ~ return (FRAG)`); if that fails to compile,
   * fall back to content (native `flow NAME() { FRAG }`, ink
   * `=== NAME === \n FRAG`); if neither compiles, the content attempt's
   * diagnostics are returned (the more permissive grammar, so its failure is
   * the more informative one). Which spelling is tried is decided once from
   * `project.entry`'s extension ({@link isNativeEntry}, #1598) — `.brink`
   * gets native wrap syntax, everything else gets ink's, so a synthetic
   * symbol never lands as a parse error in the entry's own dialect.
   */
  private compileFragment(
    fragmentSource: string,
    project: ProjectSource,
  ): FragmentCompileEntry {
    const cacheKey = `${this.checksum()}\0${fragmentSource}`;
    const cached = this.fragmentCache.get(cacheKey);
    if (cached) {
      return cached;
    }

    const symbolName = `__eval_${fragmentContentHash(fragmentSource)}`;
    const sourcesJson = JSON.stringify(project.files);
    const native = isNativeEntry(project.entry);

    const exprSynthetic = expressionWrapSource(symbolName, fragmentSource, native);
    const exprResult = JSON.parse(
      wasmCompileFragment(project.entry, sourcesJson, exprSynthetic),
    ) as CompileResult;
    if (exprResult.ok && exprResult.story_bytes) {
      return this.cacheFragment(cacheKey, {
        ok: true,
        kind: "expression",
        symbolName,
        storyBytes: new Uint8Array(exprResult.story_bytes),
      });
    }

    const contentSynthetic = contentWrapSource(symbolName, fragmentSource, native);
    const contentResult = JSON.parse(
      wasmCompileFragment(project.entry, sourcesJson, contentSynthetic),
    ) as CompileResult;
    if (contentResult.ok && contentResult.story_bytes) {
      return this.cacheFragment(cacheKey, {
        ok: true,
        kind: "content",
        symbolName,
        storyBytes: new Uint8Array(contentResult.story_bytes),
      });
    }

    const diagnostics = [
      ...(contentResult.warnings ?? []).map((d) => d.message),
      ...(contentResult.error ? [contentResult.error] : []),
    ];
    return this.cacheFragment(cacheKey, {
      ok: false,
      diagnostics:
        diagnostics.length > 0
          ? diagnostics
          : [`evaluate: "${fragmentSource}" doesn't compile as either an expression or content`],
    });
  }

  /** Insert into `fragmentCache` with FIFO eviction at `FRAGMENT_CACHE_LIMIT`
   * — see `cacheFragmentInto` (pure, unit-tested in `evaluate-dispatch`). */
  private cacheFragment(key: string, entry: FragmentCompileEntry): FragmentCompileEntry {
    return cacheFragmentInto(this.fragmentCache, key, entry, FRAGMENT_CACHE_LIMIT);
  }

  free(): void {
    this.runner.free();
  }
}

/**
 * The name-addressed flow methods a {@link FlowHandle} needs from its owner.
 * Both {@link StoryRunnerHandle} and {@link StorySessionHandle} implement
 * this shape (#1000 — the two session surfaces stay parallel), so a
 * `FlowHandle` works identically whichever one spawned it.
 */
interface FlowHost {
  continueFlow(name: string): Line;
  continueFlowMaximally(name: string): Line[];
  chooseFlow(name: string, index: number): void;
  flowDebugSnapshot(name: string): DebugState;
  destroyFlow(name: string): void;
}

/**
 * An addressable handle for one spawned/ambient flow of a {@link FlowHost}
 * (a {@link StoryRunnerHandle} or {@link StorySessionHandle}) (FS-3w,
 * `docs/flow-suspension-spec.md` §10.1).
 *
 * Each flow shares its owner's globals / visit counts / rng (true ink
 * flow semantics) but keeps its own call stack and its own `Line` stream —
 * `continue`/`choose` here drive *this* flow, not the primary one. The
 * handle is a thin, allocation-free view over the owner's name-addressed
 * flow methods (`continueFlow`/`chooseFlow`/…); it holds only the owner
 * and the flow's name, so obtaining one never has to precede driving the
 * flow, and a flow can be driven by name without a handle at all.
 */
export class FlowHandle {
  constructor(
    private readonly host: FlowHost,
    /** This flow's id (its spawn name). */
    readonly name: string,
  ) {}

  /** Advance this flow by one line (this flow's `Line` stream). The line
   * may be `{ type: "suspended" }` once FS-3r lands parks; today it never
   * is. */
  continue(): Line {
    return this.host.continueFlow(this.name);
  }

  /** Drive this flow to its next pause, collecting every `Line` up to and
   * including the terminal one (the per-flow analogue of the primary
   * flow's `continueStory`).
   *
   * Bounded by the runtime's `FlowInstance::LINE_LIMIT` (10,000 lines/turn),
   * enforced wasm-side (`continue_flow_maximally`) rather than looped
   * client-side — an infinite-emitting flow throws (matching
   * `continueStory`'s `RuntimeError::LineLimitExceeded` error shape)
   * instead of growing this method's returned array without bound and
   * hanging the host (#999). */
  continueMaximally(): Line[] {
    return this.host.continueFlowMaximally(this.name);
  }

  /** Select a choice presented by this flow. */
  choose(index: number): void {
    this.host.chooseFlow(this.name, index);
  }

  /** Per-flow debug snapshot (State View) for this flow. */
  debugSnapshot(): DebugState {
    return this.host.flowDebugSnapshot(this.name);
  }

  /** Destroy this flow. The handle is inert afterward. */
  destroy(): void {
    this.host.destroyFlow(this.name);
  }
}

/** A sandboxed, side-effect-proof fork of a story's current state
 * (`StoryRunnerHandle.speculate()`), exposed as composable verbs — the
 * speculative-eval equivalents of `StoryRunnerHandle`'s own playback verbs.
 * Nothing driven through this handle ever reaches the runner it was forked
 * from; call `free()` (or let it fall out of scope) to discard it. */
export class SpeculationHandle {
  constructor(private readonly spec: WebSpeculation) {}

  /** Move this speculation's play head to a named knot/stitch path. Only
   * this speculation's own sandboxed position moves. */
  goToPath(path: string): void {
    this.spec.go_to_path(path);
  }

  /** Select a pending choice by index. */
  choose(index: number): void {
    this.spec.choose(index);
  }

  /** Advance by one step; the line may be `{ type: "awaiting_external" }`. */
  advance(): Line {
    return JSON.parse(this.spec.advance()) as Line;
  }

  /** Advance by one step, transparently awaiting any async (`Promise`-
   * returning) bound external hit along the way. Mirrors
   * `StoryRunnerHandle.continueSingleAsync`. */
  async advanceAsync(): Promise<Line> {
    for (;;) {
      const line = this.advance();
      if (line.type !== "awaiting_external") {
        return line;
      }
      await this.awaitPendingExternal();
    }
  }

  /** Resolve the external this speculation is parked on. No-op if none is
   * pending. */
  resolveExternal(value: ExternalValue): void {
    this.spec.resolve_external(value);
  }

  /** Take the suspended async binding's `Promise` to await; `undefined` if
   * none is pending. */
  takePendingPromise(): Promise<ExternalValue> | undefined {
    const p = this.spec.take_pending_promise();
    return p === undefined ? undefined : (p as Promise<ExternalValue>);
  }

  /** The ink-declared name of the external this speculation is paused on. */
  pendingExternalName(): string | undefined {
    return this.spec.pending_external_name();
  }

  /** Evaluate an ink function on this speculation, out-of-band: output is
   * isolated and the transcript untouched. */
  evalFunction(name: string, ...args: ExternalValue[]): SpeculationFunctionEval {
    return JSON.parse(this.spec.eval_function(name, args)) as SpeculationFunctionEval;
  }

  /** `evalFunction`, transparently awaiting any async bound external hit
   * along the way (including one hit again after `resumeFunctionEval`). */
  async evalFunctionAsync(name: string, ...args: ExternalValue[]): Promise<SpeculationFunctionEval> {
    return this.driveFunctionEval(this.evalFunction(name, ...args));
  }

  /** Resume a function evaluation paused on `awaiting_external`, after
   * `resolveExternal`. Same return shape as `evalFunction`. */
  resumeFunctionEval(): SpeculationFunctionEval {
    return JSON.parse(this.spec.resume_function_eval()) as SpeculationFunctionEval;
  }

  /** `resumeFunctionEval`, transparently awaiting any further async bound
   * external. */
  async resumeFunctionEvalAsync(): Promise<SpeculationFunctionEval> {
    return this.driveFunctionEval(this.resumeFunctionEval());
  }

  private async driveFunctionEval(
    outcome: SpeculationFunctionEval,
  ): Promise<SpeculationFunctionEval> {
    let current = outcome;
    while (current.type === "awaiting_external") {
      await this.awaitPendingExternal();
      current = this.resumeFunctionEval();
    }
    return current;
  }

  /** Take + await the parked binding's `Promise`, then `resolveExternal` with
   * its settled value (or `null`, then rethrow, on rejection — unsticking the
   * parked flow the same way `StoryRunnerHandle.continueStoryAsync` does). */
  private async awaitPendingExternal(): Promise<void> {
    const promise = this.spec.take_pending_promise() as Promise<ExternalValue> | undefined;
    if (promise === undefined) {
      return;
    }
    let value: ExternalValue;
    try {
      value = (await promise) ?? null;
    } catch (err) {
      this.spec.resolve_external(null);
      throw err;
    }
    this.spec.resolve_external(value);
  }

  /** This speculation's transcript so far, resolved to `(text, tags)` lines. */
  transcript(): SpeculationLine[] {
    return JSON.parse(this.spec.transcript()) as SpeculationLine[];
  }

  /** Which externals this speculation let through live versus fell back,
   * across every verb call made on it so far. Diagnostic only. */
  externalsReport(): SpeculationExternalsReport {
    return JSON.parse(this.spec.externals_report()) as SpeculationExternalsReport;
  }

  free(): void {
    this.spec.free();
  }
}

/** Options for `StoryRunnerHandle.evaluate()` — `speculate()`'s options plus
 * cancellation (a DOM concept, so kept out of `@brink/wasm-types`, which has
 * no DOM dependency). */
export interface EvaluateOptions {
  context?: SpeculationContext;
  liveEffects?: boolean;
  budget?: { steps?: number; lines?: number };
  kinds?: SpeculationKinds;
  /** Abort the in-flight evaluation: the speculation is dropped and the
   * returned promise rejects with an `AbortError`. */
  signal?: AbortSignal;
  /** Required for a Tier-1 fragment (anything beyond a bare knot path or
   * literal-arg call) — the project's current sources, so the fragment can
   * be compiled against the live project's real symbols. Ignored for a
   * Tier-0 `source`. */
  projectSource?: ProjectSource;
}

function throwIfAborted(signal: AbortSignal | undefined): void {
  if (signal?.aborted) {
    throw new DOMException("brink: evaluate() aborted", "AbortError");
  }
}

/** Drive a `goToPath`'d speculation to its next natural stop: a `done`/`end`
 * line (`"completed"`), a `choices` line (`"choices"`, with the choices
 * reported rather than picked), or a budget ceiling. */
async function driveSpeculationToTerminal(
  speculation: SpeculationHandle,
  signal: AbortSignal | undefined,
): Promise<{ stop: SpeculationResult["stop"]; choices?: Choice[] }> {
  for (;;) {
    throwIfAborted(signal);
    let line: Line;
    try {
      line = await speculation.advanceAsync();
    } catch (err) {
      const budgetStop = budgetStopFromError(err);
      if (budgetStop !== undefined) {
        return { stop: budgetStop };
      }
      throw err;
    }
    if (line.type === "text") {
      continue;
    }
    if (line.type === "choices") {
      return { stop: "choices", choices: line.choices };
    }
    return { stop: "completed" }; // "done" | "end"
  }
}

/** Drive a literal-arg `evalFunction` call to completion. Unlike
 * `driveSpeculationToTerminal`'s `advance`, `Speculation::eval_function`
 * doesn't yet honor the caller's step budget (F4.1/F4.2 upstream gap — it
 * runs under the runtime's own internal ceiling instead), so a
 * `"step-budget"` stop is currently unreachable from this path; the check
 * is here for when that's closed. */
async function driveSpeculationCall(
  speculation: SpeculationHandle,
  name: string,
  args: ExternalValue[],
  signal: AbortSignal | undefined,
): Promise<{ value?: TypedValue; stop: SpeculationResult["stop"] }> {
  throwIfAborted(signal);
  let outcome: SpeculationFunctionEval;
  try {
    outcome = await speculation.evalFunctionAsync(name, ...args);
  } catch (err) {
    const budgetStop = budgetStopFromError(err);
    if (budgetStop !== undefined) {
      return { stop: budgetStop };
    }
    throw err;
  }
  return {
    value: outcome.type === "returned" ? outcome.value : undefined,
    stop: "completed",
  };
}

/** Map a thrown budget-exceeded error (`RuntimeError::StepLimitExceeded`/
 * `LineLimitExceeded`, surfaced as a `JsError` message) to its
 * `SpeculationResult.stop` value. `undefined` for any other error, which the
 * caller should rethrow rather than swallow. */
function budgetStopFromError(err: unknown): "step-budget" | "line-budget" | undefined {
  const message = err instanceof Error ? err.message : String(err);
  if (message.includes("step limit exceeded")) {
    return "step-budget";
  }
  if (message.includes("line limit exceeded")) {
    return "line-budget";
  }
  return undefined;
}

// ── Story Session (#370/#387) ───────────────────────────────────
//
// `StorySessionHandle` wraps the Rust-canonical `StorySession` journal +
// replay layer (`docs/story-session-spec.md`) over the wasm `WebSession`
// binding. Distinct from `StoryRunnerHandle` above: no JS-binding registry of
// its own (every external not resolved inline via the ink fallback body
// parks as a deferred `StepOutcome`, resolved out-of-band via
// `resolveExternal`), and every input that reaches the VM through it is
// journaled for replay/persistence.

/** Pure diff of two `StateSnapshot`s captured from any `StorySessionHandle`
 * (or persisted separately) — doesn't require a live session. */
export function diffSnapshots(a: StateSnapshot, b: StateSnapshot): StateDiff {
  const json = wasmDiffSnapshots(JSON.stringify(a), JSON.stringify(b));
  return JSON.parse(json) as StateDiff;
}

/** A host-registered listener for `StorySessionHandle.onJournalDirty`. */
export type JournalDirtyListener = (signal: JournalDirtySignal) => void;

/**
 * Debounce window (ms) for the journal-dirty notification (#390). A burst of
 * `choose`/`advance`/etc. calls within this window coalesces into a single
 * notification carrying the latest event count, fired on a macrotask so it
 * never lands synchronously inside — or re-entrantly stacked under — a wasm
 * call. 0 still defers (via `setTimeout(0)`), it just doesn't coalesce
 * across separate synchronous bursts.
 */
const JOURNAL_DIRTY_DEBOUNCE_MS = 50;

export class StorySessionHandle {
  private session: WebSession;
  /** Registered `onJournalDirty` listeners. */
  private journalListeners: Set<JournalDirtyListener> = new Set();
  /** Journal event count as of the last dispatched (or scheduled) notification —
   * used both to detect growth and as the debounce dedupe key. */
  private lastNotifiedEventCount = 0;
  /** Pending debounce timer handle, if a notification is scheduled. */
  private dirtyTimer: ReturnType<typeof setTimeout> | undefined;

  /**
   * Register a listener for the deferred+debounced journal-append
   * notification (#390, `docs/story-session-spec.md`). Fires **after** the
   * call stack that grew the journal has fully unwound — never synchronously
   * inside a `StorySessionHandle` method, and never while another such method
   * is still on the stack — and coalesces bursts (e.g. rapid `choose`/
   * `advance` sequences) into a single call carrying the latest event count.
   * The signal is intentionally minimal: pull the journal itself via
   * `exportJournal()` when persisting. Returns an unsubscribe function.
   */
  onJournalDirty(listener: JournalDirtyListener): () => void {
    this.journalListeners.add(listener);
    return () => {
      this.journalListeners.delete(listener);
    };
  }

  /** Check whether the journal grew since the last notification and, if so,
   * (re)schedule a debounced dispatch. Safe to call liberally after any
   * method that might append a journal event — cheap when nothing changed. */
  private noteJournalActivity(): void {
    if (this.journalListeners.size === 0) {
      return;
    }
    const count = this.session.journal_event_count();
    if (count === this.lastNotifiedEventCount && this.dirtyTimer === undefined) {
      return;
    }
    if (this.dirtyTimer !== undefined) {
      clearTimeout(this.dirtyTimer);
    }
    this.dirtyTimer = setTimeout(() => {
      this.dirtyTimer = undefined;
      // Re-read at fire time (not schedule time): further activity may have
      // landed during the debounce window, and this dispatch should report
      // the latest count rather than a stale snapshot.
      const latest = this.session.journal_event_count();
      this.lastNotifiedEventCount = latest;
      const signal: JournalDirtySignal = { eventCount: latest };
      for (const listener of this.journalListeners) {
        listener(signal);
      }
    }, JOURNAL_DIRTY_DEBOUNCE_MS);
  }

  /**
   * Create a session from compiled story bytes. `seed`, if given, seeds the
   * RNG immediately and is re-applied across `restart()`/`reload()`.
   * `deferred` names externals that must always park as `awaiting_external`
   * (out-of-band) even when the story defines a fallback body for them — a
   * host uses this to route specific calls through `resolveExternal`
   * unconditionally, distinct from a promise-in-flight park (which this
   * binding never surfaces, having no JS-binding registry of its own).
   */
  constructor(storyBytes: Uint8Array, seed?: number, deferred?: string[]) {
    this.session = new WebSession(storyBytes, seed ?? undefined, deferred);
  }

  /** Advance one step. The result is either a `Line` or a deferred
   * `awaiting_external` pause — resolve it with `resolveExternal` before
   * stepping again. */
  advance(): StepOutcome {
    const result = JSON.parse(this.session.advance()) as StepOutcome;
    this.noteJournalActivity();
    return result;
  }

  /** Advance until one line of content or a yield point. Never parks —
   * externals resolve inline or via the ink fallback body. Use `advance`
   * when the story may have deferred externals. */
  continueSingle(): SessionLine {
    const result = JSON.parse(this.session.continue_single()) as SessionLine;
    this.noteJournalActivity();
    return result;
  }

  /** Advance to the next pause. The last element is always terminal
   * (`done` / `choices` / `end`). */
  continueToPause(): SessionLine[] {
    const result = JSON.parse(this.session.continue_to_pause()) as SessionLine[];
    this.noteJournalActivity();
    return result;
  }

  /** Select a choice by index, journaling the `choice` event. */
  choose(index: number): void {
    this.session.choose(index);
    this.noteJournalActivity();
  }

  /** Drain the non-fatal runtime warnings raised since the last call, as
   * already-rendered message strings (issue #3354). Today's only source is
   * a `~ temp` read on a path its declaration had not run on yet — the
   * runtime substitutes ink's missing-variable default and keeps playing,
   * exactly as the C# reference does, so the story never faults and the
   * host has to ask for the warning to learn about it. The studio's
   * Problems panel already carries the compile-time half of the same
   * story as `E193`.
   *
   * Draining, not borrowing: call this after each step you care about — a
   * caller that never polls does not leak memory (the runtime caps the
   * list independently at `brink_runtime::RUNTIME_WARNING_CAP`), but it
   * does lose warnings it never asked for. */
  takeRuntimeWarnings(): string[] {
    return JSON.parse(this.session.takeRuntimeWarnings()) as string[];
  }

  /** Resolve the external the session is parked on (a deferred
   * `awaiting_external` from `advance`). No-op if not awaiting. */
  resolveExternal(value: ExternalValue): void {
    this.session.resolve_external(value);
    this.noteJournalActivity();
  }

  /** Whether the session is parked on a deferred external. */
  hasPendingExternal(): boolean {
    return this.session.has_pending_external();
  }

  /** Whether the last execution cycle of the default flow ended with a safe
   * exit (an explicit `-> DONE`), as opposed to running out of content.
   * Both deliver a `done`-type line; read this right after one to tell
   * them apart — `false` means the next `continueSingle`/`advance` call
   * will throw instead of returning more text. `false` if no session is
   * initialized. Reflects only the default flow, not flows
   * spawned/continued via `spawnFlow`/`continueFlow`/
   * `continueFlowMaximally` (issue #1573). */
  didSafeExit(): boolean {
    return this.session.did_safe_exit();
  }

  /** Set a global variable. Turn-boundary only: throws mid-turn (drain the
   * current turn to `done`/`choices`/`end` first). Returns `false` if no such
   * global is declared (no-op, not journaled). */
  setVar(name: string, value: ExternalValue): boolean {
    const applied = this.session.set_var(name, value);
    this.noteJournalActivity();
    return applied;
  }

  /** Move the play head to a path (turn-boundary only, journaled). */
  goToPath(path: string, ...args: ExternalValue[]): void {
    this.session.go_to_path(path, args);
    this.noteJournalActivity();
  }

  /**
   * Enable the dev-tooling visibility override (M-2b, play-from-here). When
   * `allow` is `true`, host semantic access to `#@private` definitions —
   * `setVar`/`goToPath`/`callFunction` — is permitted (enforcement off). The
   * studio sets this on a "play from here" session so it can start a flow at
   * a private knot. Applies now and persists across `restart`/`reload`.
   * Default off; production hosts respect visibility.
   */
  setDevVisibilityOverride(allow: boolean): void {
    this.session.setDevVisibilityOverride(allow);
  }

  /** Capture durable game state (does not journal). */
  saveState(): SaveState {
    return JSON.parse(this.session.save_state()) as SaveState;
  }

  /** Load durable game state (turn-boundary only, journaled). Returns
   * the {@link LoadReport} — a stale load's drops surface to the caller
   * (W14/#3307 compat honesty), never silently. */
  loadState(state: SaveState): LoadReport {
    const report = JSON.parse(this.session.load_state(JSON.stringify(state))) as LoadReport;
    this.noteJournalActivity();
    return report;
  }

  /** Export the structural transcript (RULED 2026-08-30): `OutputPart`s
   * — line refs + slots, never resolved text — as human-readable JSON.
   * Pair with {@link renderTranscript} to re-render the story-so-far
   * against whatever compile is current at read time. */
  exportTranscript(): StructuralTranscript {
    return JSON.parse(this.session.export_transcript()) as StructuralTranscript;
  }

  /** Render a structural transcript (possibly exported against an OLDER
   * compile) against THIS session's current program and line tables.
   * Cross-compile re-render is the point — an edited line's restored row
   * shows the edited text; a container the current program no longer has
   * is dropped, never an error. */
  renderTranscript(transcript: StructuralTranscript): RenderedTranscriptLine[] {
    return JSON.parse(
      this.session.render_transcript(JSON.stringify(transcript)),
    ) as RenderedTranscriptLine[];
  }

  /** Evaluate an ink function from the host, journaling a `call` event. The
   * visible story is untouched; the function's own externals resolve through
   * an isolated (non-journaling) handler. */
  callFunction(name: string, ...args: ExternalValue[]): ExternalValue {
    const result = this.session.call_function(name, args) as ExternalValue;
    this.noteJournalActivity();
    return result;
  }

  /** A typed snapshot of the current game state (globals + list membership,
   * turn counts, callstack summary). */
  snapshot(): StateSnapshot {
    return JSON.parse(this.session.snapshot()) as StateSnapshot;
  }

  /** Pure diff of two snapshots captured from this session. */
  diff(a: StateSnapshot, b: StateSnapshot): StateDiff {
    const json = this.session.diff(JSON.stringify(a), JSON.stringify(b));
    return JSON.parse(json) as StateDiff;
  }

  // ── Program inspection (Program Explorer / State View) ────────

  /** A typed, name-resolved runtime snapshot (current location, globals,
   * call stack, visit counts, pending choices, RNG state) for the studio's
   * State View. Live position — reflects wherever the session currently is,
   * unlike `programModel`/`programInkt` below (compile-bound, captured once). */
  debugSnapshot(): DebugState {
    return JSON.parse(this.session.debug_snapshot()) as DebugState;
  }

  /** The compiled program as `.inkt` text (Program Explorer's Compiled
   * Output document). Static for the loaded program. */
  programInkt(): string {
    return this.session.program_inkt();
  }

  /** Structured model of the compiled program (Program Explorer). Static
   * for the loaded program. */
  programModel(): ProgramModel {
    return JSON.parse(this.session.program_model()) as ProgramModel;
  }

  /**
   * The program→source resolver (D9, issue #3187): resolves a
   * `(containerIdx, offset)` bytecode position — exactly what
   * {@link debugSnapshot}'s `position`/call-stack frame `position` fields
   * report — to the source range it was compiled from, via the loaded
   * program's `DebugInfo` section (D6, #3184). This is the resolver behind
   * the studio's `program` Location space
   * (`docs/studio-shell-spec.md` §6.1) — `LocalSessionProvider`
   * (`packages/studio-store`) is the actual live-session consumer, since it
   * drives the studio through `StorySessionHandle`, not
   * `StoryRunnerHandle`.
   *
   * Returns `null`, not a throw, when the program carries no `DebugInfo`
   * section or the position doesn't resolve. Callers MUST gate this on
   * program identity before trusting the result — see
   * `StoryRunnerHandle.resolveDebugPosition`'s doc for the full argument.
   */
  resolveDebugPosition(containerIdx: number, offset: number): DebugSourceLocation | null {
    return JSON.parse(
      this.session.resolve_debug_position(containerIdx, offset),
    ) as DebugSourceLocation | null;
  }

  // ── Source→program resolvers (W2/#3295 — the inverse direction) ──
  //
  // The half a breakpoint gutter needs: source identity in, program
  // address out. `null` is a real answer the UI must render (refuse to
  // arm visibly), never an error to swallow; use {@link hasDebugInfo} to
  // word the refusal honestly ("no debug info" vs "nothing on that line").

  /** The program address to break on for a half-open **byte** range of
   * `file` (#3246). `null` when the span holds no executable code or the
   * artifact carries no `DebugInfo`. */
  resolveSourceRange(file: string, start: number, end: number): ProgramAddress | null {
    return JSON.parse(
      this.session.resolve_source_range(file, start, end),
    ) as ProgramAddress | null;
  }

  /** The program address to break on for a **0-based line** of `file`
   * (#3261 — needs no source text; the `DebugInfo` file table carries a
   * line index). A UI showing 1-based numbers converts at its own edge. */
  resolveSourceLine(file: string, line: number): ProgramAddress | null {
    return JSON.parse(this.session.resolve_source_line(file, line)) as ProgramAddress | null;
  }

  /** The `file:line` of a bytecode position (W6/#3299): `{ file, line }`
   * (0-based) or `null`. What the execution highlight and the paused chip
   * consume; degraded-gate before trusting it, like every resolver. */
  resolveDebugLine(containerIdx: number, offset: number): DebugLine | null {
    return JSON.parse(this.session.resolveDebugLine(containerIdx, offset)) as DebugLine | null;
  }

  /** Whether the loaded program carries a `DebugInfo` section at all —
   * the discriminator between "compiled without debug info" (or the
   * App-settings opt-out, W1/#3294) and "nothing at that position". */
  hasDebugInfo(): boolean {
    return this.session.has_debug_info();
  }

  /** Per-file staleness: whether `text` is byte-identical to the source
   * `file` was compiled from. `null` = cannot tell (no `DebugInfo`,
   * unknown file, or no recorded hash) — never collapse it into "stale". */
  sourceMatches(file: string, text: string): boolean | null {
    return JSON.parse(this.session.source_matches(file, text)) as boolean | null;
  }

  /** The program address of a named knot/stitch/function path — name-based
   * addressing ("break on `tavern.order`", play-from-here targets). Reads
   * the container table; needs no `DebugInfo`. `null` for unknown paths. */
  resolvePathAddress(path: string): ProgramAddress | null {
    return JSON.parse(this.session.resolve_path_address(path)) as ProgramAddress | null;
  }

  // ── Debug control (D8, #3186 — the control-half wasm bridge, #3232) ──
  //
  // Binds `Story::debug_run`/`debug_step`/`BreakpointSet` onto the session
  // — this is the studio's ACTUAL drive path: `LocalSessionProvider` runs
  // `StorySessionHandle`, not `StoryRunnerHandle` (whose copy above exists
  // for parity only). Bypasses the session journal — the same escape-hatch
  // contract the Rust `debugRun`/`debugStep` bindings document: debug
  // stepping is not a turn the player took, so a resumed session must not
  // replay debugger single-steps.

  /** Add an enabled breakpoint at `(containerIdx, offset)`, returning its
   * id — pass it to {@link debugBreakpointRemove}/
   * {@link debugBreakpointSetEnabled}. An empty/omitted `name` is replaced
   * with a `container:offset` label. */
  debugBreakpointAdd(containerIdx: number, offset: number, name?: string): number {
    return this.session.debugBreakpointAdd(containerIdx, offset, name);
  }

  /** Remove a breakpoint by id. Returns `false` if no breakpoint with that
   * id exists. */
  debugBreakpointRemove(id: number): boolean {
    return this.session.debugBreakpointRemove(id);
  }

  /** Enable/disable a breakpoint without removing it. Returns `false` if no
   * breakpoint with that id exists. */
  debugBreakpointSetEnabled(id: number, enabled: boolean): boolean {
    return this.session.debugBreakpointSetEnabled(id, enabled);
  }

  /** Every breakpoint currently armed on this session, in insertion order. */
  debugBreakpoints(): Breakpoint[] {
    return JSON.parse(this.session.debugBreakpoints()) as Breakpoint[];
  }

  /**
   * Run the default flow forward until an armed breakpoint, a choice point,
   * or a terminal outcome (D8, #3186). `budgetCeiling` defaults to the
   * runtime's `DEFAULT_DEBUG_BUDGET` when omitted; exceeding it throws.
   */
  debugRun(budgetCeiling?: number): DebugRunOutcome {
    return JSON.parse(this.session.debugRun(budgetCeiling)) as DebugRunOutcome;
  }

  /**
   * Step the default flow by one {@link StepMode} unit
   * (`docs/debugger-spec.md` §4's depth-delta semantics). Same budget
   * default as {@link debugRun}.
   */
  debugStep(mode: StepMode, budgetCeiling?: number): DebugRunOutcome {
    return JSON.parse(this.session.debugStep(mode, budgetCeiling)) as DebugRunOutcome;
  }


  /**
   * Run forward until the next **content line** is delivered (2026-08-30
   * Continue ruling — the granularity ladder's top tier), or a
   * breakpoint/choices/terminal stop comes first. The stop lands past the
   * glue/commit boundary, so the crossed line is IN this outcome's
   * `lines` — no one-advance delivery lag (#3321). Needs no debug line
   * info. Same budget default as {@link debugRun}.
   */
  debugRunToLine(budgetCeiling?: number): DebugRunOutcome {
    return JSON.parse(this.session.debugRunToLine(budgetCeiling)) as DebugRunOutcome;
  }

  /** Arm a break-on-write data breakpoint on a global (W18/#3311,
   * RULED). Stored by AUTHOR NAME — re-resolved against the current
   * program per advance, so the arm survives hot reloads. `false` = no
   * such global, or already armed. A watched write stops the run and
   * Continue tiers with reason `{ type: "watchpoint", name }`. */
  debugWatchpointAdd(name: string): boolean {
    return this.session.debugWatchpointAdd(name);
  }

  /** Disarm a data breakpoint. `false` = wasn't armed. */
  debugWatchpointRemove(name: string): boolean {
    return this.session.debugWatchpointRemove(name);
  }

  /** The armed data breakpoints' names, in arm order. */
  debugWatchpoints(): string[] {
    return JSON.parse(this.session.debugWatchpoints()) as string[];
  }

  /** Live value editing (W16/#3309, RULED — scalars, paused-only at the
   * panel): parse `input` against the GLOBAL's current type and commit
   * via the observed write path. `false` = refused (unknown global,
   * non-scalar, or the input doesn't parse as its type) with NO write —
   * the panel's red-shake signal. An edit can never change a value's
   * type. */
  debugEditGlobal(name: string, input: string): boolean {
    return this.session.debug_edit_global(name, input);
  }

  /** Live value editing for a frame LOCAL (W16/#3309): same contract as
   * {@link debugEditGlobal}, addressed by the debug snapshot's
   * innermost-first frame index plus the local's `slot`. Note: at a
   * choice stop the pending choices carry captured thread snapshots, and
   * choosing restores that capture — a local edited there is overwritten;
   * the panel disables local editing at `waiting_for_choice` for exactly
   * this reason. */
  debugEditTemp(frameIdx: number, slot: number, input: string): boolean {
    return this.session.debug_edit_temp(frameIdx, slot, input);
  }

  /** Step to the next **source line** (#3264, W5/#3298) — the author-tier
   * step, bounded by any armed breakpoint. Reason `noLineInfo` when the
   * artifact carries no line index. Same journal-bypass contract as
   * {@link debugStep}; the outcome carries the emitted-lines delta. */
  debugStepLine(mode: StepMode, budgetCeiling?: number): DebugRunOutcome {
    return JSON.parse(this.session.debugStepLine(mode, budgetCeiling)) as DebugRunOutcome;
  }

  // ── Shared flows (#200) ────────────────────────────────────────
  // Concurrent flows of this session's story that SHARE its globals / visit
  // counts / rng (true ink flow semantics), each with its own call stack.
  // Drives the studio's "+ new flow". Flow stepping bypasses the journal by
  // design (docs/story-session-spec.md's "shared flows keep working; their
  // externals never journal").

  /** Spawn a shared-context flow at the program root (or `path`), returning
   * an addressable {@link FlowHandle} for it — aligned with
   * `StoryRunnerHandle.spawnFlow` (#1000), whose `FlowHandle` return this
   * mirrors so session consumers can drive a spawned flow the same way. */
  spawnFlow(name: string, path?: string): FlowHandle {
    this.session.spawn_flow(name, path);
    return new FlowHandle(this, name);
  }

  /** An addressable handle for a spawned/ambient flow of this session — its
   * own `Line` stream, choices, debug snapshot, and teardown. Mirrors
   * `StoryRunnerHandle.flow`. */
  flow(name: string): FlowHandle {
    return new FlowHandle(this, name);
  }

  /** Advance a shared flow by one line. */
  continueFlow(name: string): Line {
    return JSON.parse(this.session.continue_flow(name)) as Line;
  }

  /** Drive a shared flow to its next terminal line, collecting every `Line`
   * up to and including it. Capped at the runtime's `FlowInstance::LINE_LIMIT`
   * (10,000 lines/turn) — an infinite-emitting flow throws instead of
   * growing an unbounded array and hanging the host (#999). */
  continueFlowMaximally(name: string): Line[] {
    return JSON.parse(this.session.continue_flow_maximally(name)) as Line[];
  }

  /** Select a choice in a shared flow. */
  chooseFlow(name: string, index: number): void {
    this.session.choose_flow(name, index);
  }

  /** Destroy a shared flow. */
  destroyFlow(name: string): void {
    this.session.destroy_flow(name);
  }

  /** Active flow names (sorted). */
  flowNames(): string[] {
    return JSON.parse(this.session.flow_names()) as string[];
  }

  /** Per-flow debug snapshot (State View) for a named flow. */
  flowDebugSnapshot(name: string): DebugState {
    return JSON.parse(this.session.flow_debug_snapshot(name)) as DebugState;
  }

  /** Re-evaluate parked flows' wake conditions and return the flow ids that
   * woke (`docs/flow-suspension-spec.md` §10.2). Waking never
   * auto-continues — drive a woken flow via `continueFlow`/`chooseFlow` when
   * you want its output.
   *
   * **Returns `[]` until parks exist (FS-3r).** No flow can park in today's
   * runtime — the E052 fence keeps `await` from lowering, so
   * `Line.type === "suspended"` is never produced. Exported now (FS-3w) so
   * hosts wire the wake loop against a stable shape. */
  wakeCheck(): string[] {
    return JSON.parse(this.session.wake_check()) as string[];
  }

  /** Export the session journal — the durable save artifact (embeds a
   * fast-restore checkpoint). Persist this; `StorySessionHandle.restore`
   * rebuilds a session from it. */
  exportJournal(): SessionJournal {
    return JSON.parse(this.session.export_journal()) as SessionJournal;
  }

  /**
   * Rebuild a session from compiled story bytes + an exported journal.
   * Fast-restores from the journal's embedded checkpoint when the program
   * checksum matches; otherwise replays. Returns the session and the
   * `ReplayOutcome` from that restore/replay together (a wasm constructor can
   * only return the instance, so the outcome is read back via the
   * `WebSession`'s own bookkeeping).
   */
  static restore(
    storyBytes: Uint8Array,
    journal: SessionJournal,
    seed?: number,
    deferred?: string[],
  ): { session: StorySessionHandle; outcome: ReplayOutcome } {
    const inner = WebSession.restore(
      storyBytes,
      JSON.stringify(journal),
      seed ?? undefined,
      deferred,
    );
    const outcomeJson = inner.last_replay_outcome();
    // `Object.create` bypasses the constructor, so field initializers (the
    // journal-dirty listener set/debounce state) never ran — set them up
    // explicitly rather than leaving `handle` half-constructed.
    const handle = Object.create(StorySessionHandle.prototype) as StorySessionHandle;
    handle.session = inner;
    handle.journalListeners = new Set();
    handle.lastNotifiedEventCount = 0;
    handle.dirtyTimer = undefined;
    const outcome = outcomeJson === undefined
      ? { type: "failed" as const, at_event: 0, reason: { type: "budget" as const } }
      : (JSON.parse(outcomeJson) as ReplayOutcome);
    return { session: handle, outcome };
  }

  /**
   * Hot-reload: recompile-in-place against `storyBytes`, replaying the
   * current journal against the new program. The session's own state
   * (globals, call position) reflects wherever the replay landed, even on
   * divergence or failure (parked at the reached position).
   */
  reload(storyBytes: Uint8Array): ReplayOutcome {
    const json = this.session.reload(storyBytes);
    this.noteJournalActivity();
    return JSON.parse(json) as ReplayOutcome;
  }

  /**
   * Resume a replay parked on a deferred external (live-mode replay only —
   * recorded-mode replay never parks). Resolve the pending external first via
   * `resolveExternal`, then call this. With no parked replay tail, steps the
   * live story to its next pause instead.
   */
  continueReplay(): ReplayOutcome {
    const json = this.session.continue_replay();
    this.noteJournalActivity();
    return JSON.parse(json) as ReplayOutcome;
  }

  /** Restart: create a fresh session from the same program (new empty
   * journal), re-applying the host-set seed if any. Cancels any pending
   * debounced notification and resets the dirty baseline to 0 — the fresh
   * journal starts empty, so nothing is reported as dirty until new activity
   * grows it again. */
  restart(): void {
    this.session.restart();
    if (this.dirtyTimer !== undefined) {
      clearTimeout(this.dirtyTimer);
      this.dirtyTimer = undefined;
    }
    this.lastNotifiedEventCount = 0;
  }

  /** Releases the underlying wasm session and cancels any pending debounced
   * journal-dirty notification (a fired-but-unsubscribed timer would
   * otherwise touch a freed session on its next tick). */
  free(): void {
    if (this.dirtyTimer !== undefined) {
      clearTimeout(this.dirtyTimer);
      this.dirtyTimer = undefined;
    }
    this.session.free();
  }
}
