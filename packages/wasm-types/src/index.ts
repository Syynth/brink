/**
 * @brink/wasm-types — Pure TypeScript interfaces for the brink wasm module.
 *
 * Zero runtime code — only type definitions. Every other package imports
 * types from here to avoid coupling to the wasm bindings.
 */

// ── Compilation ─────────────────────────────────────────────────

export interface Diagnostic {
  start: number;
  end: number;
  message: string;
  severity: "Error" | "Warning";
  /** Path of the file this diagnostic belongs to (may be an INCLUDEd file). */
  file: string;
}

export interface CompileResult {
  ok: boolean;
  story_bytes?: number[];
  warnings?: Diagnostic[];
  error?: string;
}

// ── Semantic tokens ─────────────────────────────────────────────

export interface SemanticToken {
  line: number;
  start_char: number;
  length: number;
  token_type: number;
  token_modifiers: number;
}

// ── HIR structural projection (#454) ────────────────────────────

/** The kind of a projected HIR span. */
export type HirSpanKind =
  | "knot"
  | "stitch"
  | "choice"
  | "gather"
  | "cond_branch"
  | "seq_branch"
  | "label"
  | "param"
  | "var_decl"
  | "const_decl"
  | "list_decl"
  | "list_member"
  | "external"
  | "temp_decl"
  | "divert"
  | "var_ref"
  | "call"
  | "content"
  | "interpolation"
  | "tag"
  | "include"
  | "divert_stmt"
  | "divert_terminal"
  | "logic"
  | "conditional"
  | "sequence";

/**
 * One HIR span projected onto the source: 0-based lines, UTF-16 columns.
 * `def_id`/`target_id` are opaque `DefinitionId` strings (`$tt_hash`) — compare
 * by equality (a reference's `target_id` equals its declaration's `def_id`).
 */
export interface HirSpan {
  start_line: number;
  start_char: number;
  end_line: number;
  end_char: number;
  kind: HirSpanKind;
  /** Block-level container (participates in rails / the per-line stack). */
  container: boolean;
  depth: number;
  def_id?: string;
  target_id?: string;
  /** Stable-within-doc container id; absent on non-containers. */
  handle?: number;
}

/** One entry of a line's container stack. */
export interface HirLineContainer {
  kind: HirSpanKind;
  handle: number;
  depth: number;
}

/**
 * The structural projection of one document: nested spans plus, per line, the
 * stack of containers covering it (outermost→innermost) — the rails view.
 */
export interface HirProjection {
  spans: HirSpan[];
  lines: HirLineContainer[][];
}

// ── Runtime ─────────────────────────────────────────────────────

export type LineType =
  | "text"
  | "done"
  | "choices"
  | "end"
  | "awaiting_external";

export interface Line {
  type: LineType;
  text: string;
  tags: string[];
  choices?: Choice[];
  /** External name, present only on an `awaiting_external` line. */
  name?: string;
}

export interface Choice {
  index: number;
  text: string;
  tags: string[];
}

// ── Save / load ─────────────────────────────────────────────────

/** Durable, name-keyed game-state save (globals, visit/turn counts, turn
 * index, RNG). Captures game state only — not execution position. Tolerant of
 * story patches. Treat as an opaque blob unless inspecting in dev. */
export interface SaveState {
  /** Save-FORMAT version (not the story's). */
  version: number;
  /** Global variables by name. Each value is a tagged ink value
   * (e.g. `{ Int: 10 }`, `{ String: "x" }`, `"Null"`). */
  globals: Record<string, unknown>;
  visits: VisitEntry[];
  turns: VisitEntry[];
  turn_index: number;
  rng_seed: number;
  previous_random: number;
}

/** A visit/turn count for one scope. `id` (a `"$tt_hash"` string) is the load
 * key; `path` is an advisory author path present only for named scopes. */
export interface VisitEntry {
  id: string;
  path?: string;
  count: number;
}

/** What a load couldn't apply. Empty `unknown_globals` means a clean load;
 * listed names are saved globals the current story no longer declares. */
export interface LoadReport {
  unknown_globals: string[];
}

// ── Story Session (#370/#387) ────────────────────────────────────
//
// `WebSession`'s wire shapes. Distinct from `Line`/`LineType` above (the
// legacy `StoryRunner` union, which smuggles `awaiting_external` into the
// `Line` type tag) — `StepOutcome` here splits that out into its own tagged
// union, per the story-session-spec wire-format fix. Tagged `type` +
// snake_case discriminants throughout, matching `Line`.

/** A tagged ink value, as the journal/save layer serializes it (never the
 * lossy `List`/`Divert` → null mapping the `ExternalValue` boundary uses).
 * e.g. `{ Int: 10 }`, `{ Float: 1.5 }`, `{ Bool: true }`, `{ String: "x" }`,
 * `"Null"`. Treat as opaque unless inspecting in dev. */
export type JournalValue = unknown;

/** One line of session output (mirrors `Line` above; kept as a separate type
 * because `StepOutcome` below never carries the `awaiting_external` variant —
 * that lives on `StepOutcome` itself). */
export interface SessionLine {
  type: "text" | "done" | "choices" | "end";
  text: string;
  tags: string[];
  choices?: Choice[];
}

/**
 * Outcome of one `WebSession.advance()` step — the wire-format fix (#387):
 * `awaiting_external` is its own variant, not smuggled into the `Line` union.
 * The two park states stay distinct: this type is always the **deferred**,
 * out-of-band kind (`deferred: true`) that the host resolves via
 * `resolveExternal` — a promise-in-flight park (the session awaiting
 * internally) is never surfaced as `StepOutcome` at all.
 */
export type StepOutcome =
  | { type: "line"; line: SessionLine }
  | { type: "awaiting_external"; deferred: true; name?: string };

/** One input recorded in the session journal (mirrors Rust `EventKind`). */
export type JournalEventKind =
  | { type: "start"; path?: string; args?: JournalValue[] }
  | { type: "choice"; index: number; label?: string }
  | { type: "external"; name: string; args?: JournalValue[]; result: JournalValue }
  | { type: "set_var"; name: string; value: JournalValue }
  | { type: "go_to_path"; path: string; args?: JournalValue[] }
  | { type: "load_state"; state: SaveState }
  | { type: "call"; name: string; args?: JournalValue[] };

/** One journal entry: the event plus reserved (serialized, uninterpreted in
 * v1) anchor/flow dimensions — see `docs/story-session-spec.md`. */
export interface JournalEvent {
  kind: JournalEventKind;
  anchor?: number;
  flow?: string;
}

/** The durable session journal — every input that entered the VM, plus a
 * fast-restore checkpoint. Serialize as-is for a save slot; feed back into
 * `WebSession.restore`. */
export interface SessionJournal {
  version: number;
  program_checksum: number;
  seed?: number;
  events: JournalEvent[];
  truncated: boolean;
  checkpoint?: SaveState;
}

/** Lightweight dirty signal delivered by `StorySessionHandle.onJournalDirty`
 * (#390). Carries just enough for a host to decide whether/when to persist —
 * pull the actual journal via `exportJournal()`. `eventCount` is the journal
 * length at the time the debounced notification fired (a monotonically
 * increasing counter across a session's lifetime, reset only by `restart`). */
export interface JournalDirtySignal {
  eventCount: number;
}

/** A soft, non-fatal replay observation. */
export type ReplayWarning = {
  type: "choice_label_drift";
  at_event: number;
  index: number;
  recorded: string;
  found: string;
};

/** What replay found at a divergence point instead of the recorded event. */
export type DivergenceFound =
  | { type: "choice_index_out_of_range"; index: number; available: number }
  | { type: "not_waiting_for_choice" }
  | { type: "unknown_path"; path: string }
  | { type: "unexpected_event" };

/** Why replay stopped without diverging. */
export type FailReason =
  | { type: "runtime_error"; message: string }
  | { type: "budget" }
  | { type: "awaiting_external"; name: string };

/** Outcome of replaying a journal (prefix) against a program — from
 * `WebSession.restore`/`reload`/`continueReplay`. Typed, never silent. */
export type ReplayOutcome =
  | { type: "replayed"; warnings: ReplayWarning[] }
  | {
      type: "diverged";
      at_event: number;
      expected: JournalEvent;
      found: DivergenceFound;
    }
  | { type: "failed"; at_event: number; reason: FailReason };

/** Resolved membership of a `List`-valued global in a `StateSnapshot`. */
export interface SnapshotList {
  items: string[];
}

/** One summarized call frame in a `StateSnapshot`. */
export interface SnapshotFrame {
  /** root | function | tunnel | thread | external | eval */
  kind: string;
  location?: string;
  temps: number;
}

/** active | waiting_for_choice | done | ended */
export type SnapshotStatus = "active" | "waiting_for_choice" | "done" | "ended";

/** A typed, name-resolved snapshot of a session's game state — distinct from
 * the string-valued `DebugState` (that one is for the studio's read-only
 * State View; this one is a first-class, diffable session artifact). */
export interface StateSnapshot {
  globals: Record<string, JournalValue>;
  lists: Record<string, SnapshotList>;
  turn_index: number;
  visit_counts: Record<string, number>;
  turn_counts: Record<string, number>;
  call_stack: SnapshotFrame[];
  status: SnapshotStatus;
}

/** Membership delta for one list-valued global (part of a `StateDiff`). */
export interface ListDelta {
  added: string[];
  removed: string[];
}

/** A pure diff between two `StateSnapshot`s (`WebSession.diff`/`diffSnapshots`). */
export interface StateDiff {
  added_globals: Record<string, JournalValue>;
  removed_globals: Record<string, JournalValue>;
  /** `[before, after]` per changed global. */
  changed_globals: Record<string, [JournalValue, JournalValue]>;
  list_deltas: Record<string, ListDelta>;
  /** `[before, after]`, present only when `turn_index` changed. */
  turn_index?: [number, number];
  pushed_frames: SnapshotFrame[];
  popped_frames: SnapshotFrame[];
}

// ── IDE types ───────────────────────────────────────────────────

export interface CompletionItem {
  name: string;
  kind: string;
  detail?: string;
  /**
   * Literal to insert when it differs from the displayed `name` — the host
   * value picker (#174) shows `HarborGate` but inserts `5`. Absent ⇒ insert
   * `name` (the default for ordinary symbol completions).
   */
  insert?: string | null;
  /**
   * `true` when the symbol is defined in a file NOT reachable from the current
   * file's INCLUDE graph (#312 F). Such rows get a "from <file>" affordance and,
   * on accept, auto-insert the `INCLUDE` alongside the symbol.
   */
  out_of_scope?: boolean;
  /**
   * Project-relative path of the file declaring this symbol — set only for
   * out-of-scope completions (the auto-import target).
   */
  source_file?: string | null;
}

export interface HoverInfo {
  content: string;
  start?: number;
  end?: number;
}

export interface Location {
  file: string;
  start: number;
  end: number;
}

export interface FileEdit {
  start: number;
  end: number;
  new_text: string;
}

export interface InlayHint {
  offset: number;
  label: string;
  kind: string;
  padding_right: boolean;
}

/** A `hex_color` argument literal for the built-in color picker (#174). */
export interface ColorHint {
  /** Start offset of the literal (including quotes), UTF-16. */
  start: number;
  /** End offset of the literal (exclusive), UTF-16. */
  end: number;
  /** The bare hex value, quotes stripped (e.g. "#FF0000"). */
  value: string;
}

/** The authoring state of an argument slot (argument-widget spec §4). */
export type SlotState =
  /** A literal — Edit replaces `[start, end)`; `value` is quotes-stripped. */
  | { kind: "filled"; start: number; end: number; value: string }
  /** No argument — Fill inserts at `insert_at` (`, `-prefixed if needed). */
  | { kind: "empty"; insert_at: number; needs_leading_comma: boolean }
  /** A non-literal expression — no inline affordance. */
  | { kind: "expr" };

/** One parameter slot of a call (argument-widget spec §4). */
export interface SlotWidget {
  param_name: string;
  /** The built-in widget kind for this slot's type (`color`, …), if any. */
  widget?: string;
  /** The semantic-type name, if the param is typed. */
  type_name?: string;
  /** Static value-list items (#174) — the Form renders these as a dropdown. */
  values?: ValueItem[];
  state: SlotState;
}

/** The authoring state of an arg-group (uniform across members; spec §2). */
export type GroupState =
  /** All members are literals — Edit replaces each `spans[k]` with a value. */
  | { kind: "filled"; spans: [number, number][]; values: string[] }
  /** All members empty — Fill inserts the members joined by `, ` at `insert_at`. */
  | { kind: "empty"; insert_at: number; needs_leading_comma: boolean };

/** An arg-group widget at a call site (argument-widget spec §2) — one widget
 *  spanning several params, with inter-arg context resolved. */
export interface GroupWidgetSite {
  /** Widget / semantic type (matches a host `ArgumentWidget.type`). */
  type: string;
  /** Editor container — `"popover"` (default) or `"modal"`. */
  surface?: "popover" | "modal";
  /** Param indices the group spans (the studio skips these per-slot). */
  param_indices: number[];
  param_names: string[];
  state: GroupState;
  /** Resolved inter-arg context: key → the sibling arg's literal value (from the
   *  document — what inline editing uses). */
  context: Record<string, string>;
  /** Raw inter-arg context: key → the sibling param index. The Form resolves
   *  context from its own live draft values via this map. */
  context_params?: Record<string, number>;
}

/** A call site with a per-parameter widget slot (argument-widget spec §4). */
export interface CallWidgetSite {
  callee: string;
  /** The call-name span (UTF-16) — anchors the form glyph. */
  name_start: number;
  name_end: number;
  slots: SlotWidget[];
  /** Arg-group widgets (spec §2) — render the group inline, skip its slots.
   *  Only present when the group's members are uniformly filled/empty. */
  groups: GroupWidgetSite[];
  /** Every declared arg-group for the callee, independent of the current args —
   *  the Form renders these (seeding member values from `slots`), so a partial
   *  or over-full call still gets its widgets. */
  declared_groups?: DeclaredGroup[];
}

/** A declared arg-group widget — manifest structure with no arg-state (spec §2).
 *  Used by the Form to render one control per declared group regardless of how
 *  many arguments the call currently has. */
export interface DeclaredGroup {
  type: string;
  surface?: "popover" | "modal";
  param_indices: number[];
  param_names: string[];
  /** key → the sibling param index supplying its inter-arg context. */
  context_params?: Record<string, number>;
}

// ── Host argument widgets (argument-widget-spec §3) ──────────────────
//
// Studio/host API types (not wasm-boundary types) — kept here as the shared
// base both @brink-lang/editor (the registry) and @brink/studio-shell (the
// `StudioExtensions.argumentWidgets` surface) import without a cross-dependency.

/** Context handed to a host widget's renderers (one entry per group member). */
export interface ArgumentWidgetContext {
  /** The widget's semantic type / id. */
  type: string;
  /** The EXTERNAL being called. */
  external: string;
  /** Param name(s) in the group (one for a single-param widget). */
  paramNames: string[];
  /** Current literal value(s), quotes stripped; empty for an unfilled slot. */
  values: string[];
  /** Resolved inter-arg context, e.g. `{ map: "5" }` (Stage 5). */
  context?: Record<string, string>;
}

/** The studio-provided handle a host widget editor resolves/cancels through. */
export interface ArgumentWidgetEditorHost {
  /** New literal value(s) for the group — the studio writes them back. */
  resolve(values: string[]): void;
  cancel(): void;
}

/**
 * A host-provided argument widget. The inline chip is *studio-rendered* from
 * label DATA (`inline` returns text + an optional CSS class); only the editor is
 * host-rendered, into a studio-owned popover/modal via a mount-callback.
 */
export interface ArgumentWidget {
  /** Semantic type / widget id this renders for. Host ids: `host.<vendor>.<name>`. */
  type: string;
  /** Optional inline label data — the studio draws the chip from it. */
  inline?(ctx: ArgumentWidgetContext): { text: string; className?: string };
  /** The editor — the only host-rendered surface. Mount the body into
   *  `container`, resolve/cancel through `host`, and return a teardown. */
  editor: {
    surface?: "popover" | "modal";
    render(
      ctx: ArgumentWidgetContext,
      host: ArgumentWidgetEditorHost,
      container: HTMLElement,
    ): () => void;
  };
}

export interface SignatureInfo {
  label: string;
  documentation?: string;
  parameters: { label: string }[];
  active_parameter: number;
}

/** A fold's kind (#365): `"structural"` is everything the editor emitted
 *  before #365 (decls, doc comments, conditionals, sequences, choice sets) —
 *  user-invoked in every mode, never auto-collapsed. `"machinery"` and
 *  `"narrative"` are run-based folds (>=2 consecutive same-nature lines)
 *  computed from the line classification (base, or a registered dialect's
 *  declared `nature`). */
export type FoldKind = "structural" | "machinery" | "narrative";

export interface FoldRange {
  start_line: number;
  end_line: number;
  collapsed_text?: string;
  /** Whole-line declaration fold (docs + header + body): fold from the start
   *  of start_line and render the hidden header as the placeholder. */
  from_line_start?: boolean;
  /** The fold's kind (#365). */
  kind: FoldKind;
}

export interface DocumentSymbol {
  name: string;
  kind: string;
  detail?: string;
  start: number;
  end: number;
  /** Start of the full symbol body (including header through end of content). */
  full_start: number;
  /** End of the full symbol body. */
  full_end: number;
  children: DocumentSymbol[];
}

/**
 * Self-describing, internally-tagged payload identifying which transformation a
 * code action performs. The `action` field is the discriminator; the remaining
 * fields depend on it (e.g. `FormatStitch` carries `knot` and `stitch`). Pass
 * the whole object back to `resolveCodeAction` to apply the action — the caller
 * never reconstructs it from the cursor position.
 */
export interface CodeActionData {
  action: string;
  [key: string]: unknown;
}

export interface CodeAction {
  title: string;
  kind: string;
  /** Opaque, self-describing payload — feed back to `resolveCodeAction`. */
  data: CodeActionData;
}

// ── Document handles (multi-document EditorSession) ─────────────

/**
 * Opaque document-handle id returned by `EditorSession.openDocument` /
 * `openFragment`. At the wasm boundary `0` is the "file not loaded"
 * sentinel and never a valid handle.
 */
export type DocumentId = number;

/**
 * What an `updateDocument` call actually changed in the underlying file,
 * in UTF-16 **file** coordinates. `[start, end)` is the replaced range of
 * the file's previous content. The inserted text is the `source` argument
 * the caller already has — unless `text` is present, in which case a
 * fragment splice appended a `\n` separator and `text` carries the
 * actually-inserted text (`source` + `"\n"`). Sibling editor views of the
 * same file can apply this directly as a CM6 change spec.
 */
export interface DocumentChangeSpec {
  path: string;
  start: number;
  end: number;
  text?: string;
}

// ── Structural move types ───────────────────────────────────────

export interface CrossFileEdit {
  /** Path of the file whose content is replaced. */
  path: string;
  /** The file's full source after applying its reference edits. */
  new_source: string;
}

/** One entry in a structural op's breakage report — a diagnostic the op would
 * introduce. Locations are 1-based, matching the editor's status surfaces. */
export interface RenameDiagnostic {
  severity: "error" | "warning";
  /** Stable diagnostic code, e.g. `E022`. */
  code: string;
  message: string;
  /** Project-relative path of the file the diagnostic lands in. */
  path: string;
  /** 1-based line of the diagnostic's start. */
  line: number;
  /** 1-based column of the diagnostic's start. */
  col: number;
}

/** The unified result of every mutating structural op (#316): rename, move,
 * promote, demote, reorder, file-rename, and delete. `new_source` is the
 * rewritten primary file; `cross_file_edits` carry the referencing files'
 * rewrites. `safe` is true when the op introduces no new diagnostics; otherwise
 * `introduced_diagnostics` holds the breakage report and the edits apply only on
 * an explicit force. Reorders are trivially safe (empty breakage). */
export interface StructuralResult {
  ok: boolean;
  /** The file path this result applies to. */
  path?: string;
  new_source?: string;
  cross_file_edits: CrossFileEdit[];
  /** Diagnostics present after the op but not before. Empty ⇒ `safe`. */
  introduced_diagnostics: RenameDiagnostic[];
  /** True when the op introduces no new diagnostics. */
  safe: boolean;
  error?: string;
}

/** One file relocated by an atomic directory rename/move (#314). The caller
 * writes `new_source` at `new_path` and removes `old_path`. `new_source` already
 * carries the file's own outbound-include rewrites. */
export interface MovedFile {
  /** The file's project-relative path before the move. */
  old_path: string;
  /** The file's project-relative path after the move. */
  new_path: string;
  /** The moved file's full source with its relative includes rewritten. */
  new_source: string;
}

/** The result of an atomic directory rename/move (#314) — the multi-file analog
 * of {@link StructuralResult}. Every affected `INCLUDE` is rewritten against one
 * pre-move snapshot, so moved files' outbound includes, inbound includes from
 * outside the folder, and intra-folder sibling includes stay mutually
 * consistent. `moved_files` are the relocated files; `cross_file_edits` carry
 * the outside referrers' rewrites (full new source, path-keyed). `safe` is true
 * when the move introduces no new diagnostics; otherwise `introduced_diagnostics`
 * holds the breakage report and the edits apply only on an explicit force. */
export interface DirMoveResult {
  ok: boolean;
  /** Every file relocated by the move. */
  moved_files: MovedFile[];
  /** Reference edits in files outside the moved directory (full new source). */
  cross_file_edits: CrossFileEdit[];
  /** Diagnostics present after the move but not before. Empty ⇒ `safe`. */
  introduced_diagnostics: RenameDiagnostic[];
  /** True when the move introduces no new diagnostics. */
  safe: boolean;
  error?: string;
}

// ── Multi-file project types ────────────────────────────────────

export interface ProjectFile {
  path: string;
}

export interface FileOutline {
  path: string;
  symbols: DocumentSymbol[];
}

// ── Story graph types (studio-shell spec §4.1) ──────────────────

export type StoryGraphNodeKind = "knot" | "stitch" | "end" | "done";

export type StoryGraphEdgeKind = "divert" | "choice" | "tunnel" | "thread";

/**
 * A story-graph node: a knot, a stitch, or an `END`/`DONE` pseudo-node.
 * `file`/`start`/`end` are absent on pseudo-nodes; `start`/`end` are UTF-16
 * offsets of the declaration name within `file`.
 */
export interface StoryGraphNode {
  /** Stable id — the qualified name (`knot`, `knot.stitch`), or `END`/`DONE`. */
  id: string;
  /** The qualified display name (currently identical to `id`). */
  name: string;
  kind: StoryGraphNodeKind;
  file?: string;
  start?: number;
  end?: number;
  /** For stitches: the owning knot's node id (the UI nests them). */
  parent?: string;
}

/**
 * A source site that produced a story-graph edge: the divert target path's
 * span, or the whole divert statement for `-> DONE`/`-> END`. `start`/`end`
 * are UTF-16 offsets within `file` — the same convention as node spans.
 */
export interface StoryGraphEdgeOccurrence {
  file: string;
  start: number;
  end: number;
}

/** A directed story-graph edge between node ids. Function calls are excluded. */
export interface StoryGraphEdge {
  from: string;
  to: string;
  kind: StoryGraphEdgeKind;
  /**
   * The divert sites that produced this edge, sorted by (file, span). An
   * aggregated edge (e.g. two choices targeting the same knot) keeps one
   * entry per site. Absent when empty — only for HIR-synthesized diverts
   * with no source anchor.
   */
  occurrences?: StoryGraphEdgeOccurrence[];
}

/**
 * The whole-project story graph. Deterministically ordered: nodes sorted by
 * id, edges deduplicated and sorted by (from, to, kind). Recomputed per call,
 * like the project outline.
 */
export interface StoryGraph {
  nodes: StoryGraphNode[];
  edges: StoryGraphEdge[];
}

// ── Line conversion types ───────────────────────────────────────

export type ConvertTarget = "narrative" | "choice" | "sticky_choice" | "gather" | "choice_body";

export interface TextEdit {
  from: number;
  to: number;
  insert: string;
}

/**
 * Result of an auto-import (#312 F): whether `target` was already reachable
 * from the current file's INCLUDE graph and, when not, the `INCLUDE`-insertion
 * `edit` (whole-file UTF-16 coords) to apply to the current file's source.
 * Idempotent — `already_reachable` ⇒ no `edit`.
 */
export interface AutoImportResult {
  ok: boolean;
  already_reachable: boolean;
  edit?: TextEdit | null;
  error?: string | null;
}

// ── Include info types ──────────────────────────────────────────

export interface IncludeInfo {
  path: string;
  resolved: string;
  loaded: boolean;
}

// ── Line context types (from brink-ide) ─────────────────────────

export type LineElement =
  | "knot_header"
  | "stitch_header"
  | "narrative"
  | "choice"
  | "gather"
  | "divert"
  | "logic"
  | "var_decl"
  | "comment"
  | "include"
  | "external"
  | "tag"
  | "blank";

export interface WeavePosition {
  depth: number;
  element: WeaveElement;
}

export type WeaveElement =
  | "top_level"
  | { choice_line: { sticky: boolean } }
  | "choice_body"
  | "gather_continuation"
  | "conditional_branch"
  | "sequence_branch";

export interface LineContext {
  element: LineElement;
  weave: WeavePosition;
  has_tags: boolean;
  /** For a divert line: standalone (`-> x`, `-> END`) vs a tunnel call or
   *  thread start (#480). Structural fact — never re-sniff the text. */
  standalone: boolean;
  /** Option identity (#480): the full lineage of zero-based option indices
   *  through the weave, present on choice-line / choice-body lines. */
  option_path?: number[];
  block_comment: boolean;
  /** Dialect classification for this line (#368), present only when a
   *  dialect is registered (`EditorSession.set_dialect`) and this line
   *  matched one of its declared kinds (directly or via a chain rule). */
  dialect?: DialectLineInfo | null;
}

/** Dialect-classification result for one line (#368) — computed once at
 *  classification time on the Rust side; the editor never re-derives it. */
export interface DialectLineInfo {
  /** The dialect kind (e.g. `"character"`, `"parenthetical"`, `"dialogue"`). */
  kind: string;
  /** Captured named-group attributes, sorted by name. For chained lines,
   *  carries the `chain.carry` groups forward from the run's originating
   *  match (whole-run `data-speaker`, etc). */
  attrs: [string, string][];
  /** Hidden geometry byte spans (full-line-relative, UTF-8 byte offsets). */
  hidden_spans: [number, number][];
  /** The editable content byte span (full-line-relative). `null`/absent for
   *  chain-only (pattern-less) kinds — content is the whole trimmed line. */
  content_span?: [number, number] | null;
}

// ── Debug snapshot (State View) ──────────────────────────────────

export interface DebugGlobal {
  name: string;
  value: string;
}

export interface DebugFrame {
  /** root | function | tunnel | thread | external | eval */
  kind: string;
  /** Nearest named knot/stitch for this frame, if resolvable. */
  location?: string;
  temps: number;
}

export interface DebugVisit {
  path: string;
  count: number;
}

export interface DebugChoice {
  text: string;
  target?: string;
  /**
   * The raw `pending_choices` index — the same pre-filter position the
   * live `Choice.index` carries and that `choose()` expects. Not a
   * post-filter enumeration position: invisible-default choices are
   * filtered out of what's shown but still occupy a slot, so this can
   * skip values (e.g. 0, 2, 3 if index 1 was an invisible default).
   */
  index: number;
}

export interface DebugRng {
  seed: number;
  previous: number;
}

/** A read-only, name-resolved snapshot of the runtime's current state. */
export interface DebugState {
  /** active | waiting_for_choice | done | ended */
  status: string;
  current_location?: string;
  turn_index: number;
  globals: DebugGlobal[];
  call_stack: DebugFrame[];
  visit_counts: DebugVisit[];
  pending_choices: DebugChoice[];
  rng: DebugRng;
}

// ── Program model (Program Explorer) ─────────────────────────────

export interface ProgramGlobal {
  name: string;
  ty: string;
  default: string;
  mutable: boolean;
}

export interface ProgramListItem {
  name: string;
  ordinal: number;
}

export interface ProgramList {
  name: string;
  items: ProgramListItem[];
}

export interface ProgramExternal {
  name: string;
  arg_count: number;
  fallback?: string;
}

/** A knot or stitch in the compiled-program tree. */
export interface KnotNode {
  path: string;
  name: string;
  /** "knot" | "stitch" */
  kind: string;
  /** Counting flags: "visits" | "turns" | "start_only" */
  flags: string[];
  path_hash: number;
  /** Resolved bytecode disassembly, one mnemonic per entry. */
  disasm: string[];
  children: KnotNode[];
}

/** Structured view of the statically compiled program. */
export interface ProgramModel {
  checksum: string;
  globals: ProgramGlobal[];
  lists: ProgramList[];
  externals: ProgramExternal[];
  knots: KnotNode[];
}

// ── Lines table (#366, host-side analysis) ───────────────────────
//
// Mirrors `brink_intl::json_model` (the `export-xliff` `lines.json` shape,
// reused verbatim — see `StoryRunner.lines_table`/`docs/dialect-spec.md`).
// Project-wide: one entry per compiled scope (root/knot/stitch), INCLUDEs
// already resolved by the compile. Each line carries its text (plain or a
// slot/select template) and, when known, its source span (file + byte
// range in that file). The compiler's answer for host-side analyses that
// need to walk emitted lines project-wide — cast detection (#366), per-
// speaker word counts, the #362 line-fit metrics epic.

/** A single named-group slot in a template line. */
export interface LineSlot {
  index: number;
  name: string;
}

/** Where a line came from in the original `.ink` project. */
export interface LineSource {
  file: string;
  range_start: number;
  range_end: number;
}

/** One part of a template line's content. */
export type LinePart =
  | { slot: number }
  | { select: LineSelect }
  | string;

/** A plural/keyword select over a slot value. Each `variants` entry is a
 *  one-key object (`{ "cardinal:One": "..." }`, `{ "=0": "..." }`, etc.) —
 *  mirrors the Rust `format_select_key` tagging exactly. */
export interface LineSelect {
  slot: number;
  variants: Record<string, string>[];
  default: string;
}

/** A line's content — a plain string, or a template (literal/slot/select parts). */
export type LineContent = string | { template: LinePart[] };

/** One compiled line entry. */
export interface LinesTableLine {
  index: number;
  content?: LineContent;
  /** 16-hex-digit source-identity hash of this line's content. */
  hash: string;
  audio?: string | null;
  slots?: LineSlot[];
  source?: LineSource | null;
}

/** One scope (root, knot, or stitch) in the lines table. */
export interface LinesTableScope {
  /** The scope's qualified name (e.g. a knot or stitch name), when known. */
  name?: string | null;
  /** The scope's `DefinitionId`, formatted `0x{16 hex digits}`. */
  id: string;
  lines: LinesTableLine[];
}

/** The compiler's lines table (`StoryRunner.linesTable()`, #366):
 *  project-wide, `INCLUDE`s already resolved by the compile. */
export interface LinesTable {
  version: number;
  source_checksum: string;
  scopes: LinesTableScope[];
}

// ── Host capability manifest (tooling / author-time) ────────────
//
// Mirrors `brink_ir::host_manifest`. Authored by the host and passed as JSON
// to `EditorSession.setHostManifest`. Describes the host's external-function
// vocabulary for author-time validation and richer hover/completion. Never
// affects the runtime or codegen. See docs/host-capability-manifest.md.

/** A base type keyword, or the name of a registered semantic type. */
export type TypeRef = string;

/** The underlying base types at an external boundary. */
export type BaseType = "string" | "int" | "float" | "bool" | "void";

/** Presentation/effect category of an external (informational). */
export type ExternalKind = "query" | "effect" | "presentation" | "plain";

/** A closed-domain constraint, checkable against literal arguments. */
export type Constraint =
  | { kind: "enum"; values: string[] }
  | { kind: "regex"; pattern: string }
  | { kind: "range"; min?: number | null; max?: number | null };

/** One pickable value with its host-given display label (Tier 3, #174). */
export interface ValueItem {
  /** The literal inserted into source (e.g. "5"). */
  value: string;
  /** The display label (e.g. "HarborGate"). */
  label: string;
  /** Optional secondary text (e.g. "Switch #5"). */
  detail?: string | null;
}

/**
 * Where a semantic type's pickable values come from (Tier 3, #174). Advisory
 * tooling metadata only — it drives the author-time argument picker and never
 * affects checking or the compiled program. See docs/host-argument-picker-spec.md.
 */
export type ValueSource =
  | { source: "static"; items: ValueItem[] }
  | { source: "host" };

/** A studio-builtin argument widget for a semantic type (Tier 3). */
export interface WidgetDecl {
  /** The built-in widget kind, e.g. `"color"`. */
  kind: string;
}

/** A flat-nominal semantic type: a base type plus one optional constraint. */
export interface SemanticTypeDef {
  name: string;
  base: BaseType;
  constraint?: Constraint | null;
  /** The picker's value source (Tier 3) — `static` (no host) or `host`. */
  values?: ValueSource | null;
  /** A studio-builtin argument widget (Tier 3) — e.g. `{ kind: "color" }`. */
  widget?: WidgetDecl | null;
}

/** A registered external parameter. */
export interface ManifestParam {
  name: string;
  ty?: TypeRef;
}

/** An arg-group widget on an external (argument-widget spec §2): one widget over
 *  several params, with an editor surface + optional inter-arg context. */
export interface ArgGroupWidget {
  /** Argument indices the widget spans, e.g. `[0, 1]`. */
  group: number[];
  /** Semantic type / widget id (matches a host `ArgumentWidget.type`). */
  type: string;
  /** Editor container — `"popover"` (default) or `"modal"`. */
  surface?: "popover" | "modal";
  /** Inter-arg context: key → the sibling arg index supplying it, e.g. `{ map: 1 }`. */
  context?: Record<string, number>;
}

/** A registered external-function signature. */
export interface ManifestExternal {
  name: string;
  params?: ManifestParam[];
  returns?: TypeRef;
  kind?: ExternalKind;
  doc?: string | null;
  /** Arg-group widgets (argument-widget spec §2). */
  widgets?: ArgGroupWidget[];
  /** Category breadcrumb for the Host Functions panel (#210), e.g.
   *  ["Map", "Movement"] → nested collapsible sections. Empty/absent = ungrouped. */
  path?: string[];
}

/** The host-owned, project-wide external vocabulary. */
export interface HostManifest {
  externals?: ManifestExternal[];
  types?: SemanticTypeDef[];
}

// ── Dialogue dialect (#368, tooling / author-time) ──────────────
//
// Mirrors `brink_ir::dialect`. A versioned, pure-JSON schema describing a
// project's dialogue-line conventions (cues, parentheticals, dialogue
// chains) so the editor can classify and decorate lines without hardcoding
// any one convention. Authoring-time/tooling artifact only — never
// runtime-delivered (see docs/dialect-spec.md). Passed as JSON to
// `EditorSession.setDialect`.

/** The 3-way element nature (ruling: 3-way, not 2-way). */
export type ElementNature = "narrative" | "machinery" | "structural";

/** The general portable-regex pattern representation every interpreter
 *  (Rust, TS) executes directly.
 *
 *  Field names are **snake_case** on the wire — this type is serialized
 *  verbatim to/from the Rust `serde` struct (no `rename_all = "camelCase"`
 *  on the Rust side), and the same JSON file is consumed by both
 *  interpreters (the conformance corpus + `EditorSession.setDialect`). */
export interface PatternShape {
  /** Portable-regex pattern (JS `RegExp` ∩ Rust `regex` subset: named groups
   *  yes, lookaround/backreferences no), anchored `^...$` against the
   *  trimmed line. */
  pattern: string;
  /** Which named group is the editable content. Drives `content_span`
   *  geometry (markup/inline-decoration scoping) and classification
   *  `data-*` attrs — for a kind like `parenthetical` this legitimately
   *  includes wrapping punctuation that stays visible on the line. */
  content_group?: string | null;
  /** Which named group's captured value fills `template`'s placeholder for
   *  convert/strip round-trips. Defaults to `content_group` when absent —
   *  additive, byte-identical for every dialect that doesn't set it (#406).
   *  Exists because a kind can need a different answer to "what region is
   *  content" (`content_group`) than to "what value round-trips through
   *  `template`" (`template_group`) — e.g. `parenthetical`'s `content_group`
   *  is wrap-inclusive (parens stay part of the editable/markup-scoped
   *  region) while `template_group` names a nested bare-text group so the
   *  literal parens live in `template` itself, matching how every other
   *  convert/strip consumer treats "Parenthetical content" as bare text.
   *  Never emitted as a `data-*` attr and never hidden. */
  template_group?: string | null;
  /** Named groups whose matched span is hidden geometry. */
  hidden?: string[];
  /** Template string for insertion/conversion/format (e.g. `"@${speaker}:<>"`). */
  template: string;
}

/** Affix sugar: a content slot wrapped in literal prefix/suffix text —
 *  compiles mechanically to the pattern form (see `compileAffix` in the TS
 *  interpreter, mirroring `compile_affix` in Rust). Wire field names are
 *  snake_case (see {@link PatternShape}). */
export interface AffixShape {
  /** Literal prefix before the content (e.g. `"@"`). Hidden by construction. */
  prefix?: string | null;
  /** Literal suffix after the content (e.g. `":"`, or `":<>"` when glued). */
  suffix?: string | null;
  /** Whether the suffix's glue (`<>`) is appended and always hidden. */
  glued?: boolean;
  /** The semantic role of the content slot. Defaults to `"content"`. */
  content_role?: string;
}

/** How an element is recognized/produced in source text: either the general
 *  `pattern` form, or `affix` sugar (compiled to `pattern` before use). */
export type SourceShape = PatternShape | AffixShape;

/** The post-glue shape the runtime sees out of `continue_line()` output.
 *  Positionally constrained — non-reserved-prefix shapes (e.g. a
 *  parenthetical) peel only after a reserved-prefix segment (e.g. a cue).
 *  Wire field names are snake_case (see {@link PatternShape}). */
export interface EmittedShape {
  pattern: string;
  content_group?: string | null;
  reserved_prefix?: boolean;
}

/** A near-miss diagnostic: a pattern that almost matches a kind but doesn't
 *  quite, paired with a message and severity. */
export interface MalformedRule {
  pattern: string;
  message: string;
  severity?: string;
}

/** One declared element kind. */
export interface DialectElement {
  /** Open string taxonomy kind (e.g. `"character"`). CSS class derives as
   *  `brink-<kind>`. */
  kind: string;
  nature: ElementNature;
  /** Absent for chain-only kinds (e.g. `"dialogue"`, produced only by a
   *  chain rule). When absent, content is the whole trimmed line. */
  source?: SourceShape | null;
  emitted?: EmittedShape | null;
  malformed?: MalformedRule[];
}

/** "Narrative immediately after one of `after` becomes `becomes`." Blank
 *  lines always break the chain (not configurable in v1). */
export interface ChainRule {
  after: string[];
  /** Predecessor kinds must produce a line of this kind. Defaults to
   *  `["narrative"]`. */
  is?: string[];
  becomes: string;
  /** Named groups carried forward onto the whole chained run as `data-*`
   *  attributes (e.g. `["speaker"]` → `data-speaker`). */
  carry?: string[];
}

/** A transition action. Tagged so shapes are unambiguous. */
export type TransitionAction =
  | { action: "convert"; kind: string }
  | { action: "newline" }
  | { action: "strip" }
  | { action: "clear" }
  | { action: "trap" };

/** One Tab/Enter/Shift-Tab transition row, contributed by the dialect for a
 *  kind it declares — an overlay resolved before the built-in weave table.
 *  Wire field names are snake_case (see {@link PatternShape}). */
export interface TransitionRow {
  /** The kind this row applies to (must be declared, or reserved-structural). */
  on: string;
  key: string;
  has_content?: boolean | null;
  action: TransitionAction;
  /** Editor-facing hint text (status bar, etc). */
  hint?: string | null;
}

/** One picker/template entry for a declared kind. Wire field names are
 *  snake_case (see {@link PatternShape}). */
export interface TemplateEntry {
  kind: string;
  label: string;
  picker_key?: string | null;
  blank_tab?: boolean;
}

/** Editor-overlay template metadata (picker labels, blank-tab behavior). */
export interface Templates {
  entries?: TemplateEntry[];
}

/** A versioned, pure-JSON dialogue dialect. No functions, no `RegExp`
 *  objects — patterns are strings in the portable-regex subset. See
 *  docs/dialect-spec.md. */
export interface DialogueDialect {
  /** Schema version. Only `1` is defined. */
  version: number;
  /** Human-readable dialect name (e.g. `"at-cue"`). */
  name?: string;
  /** Element declarations, in classification precedence order. */
  elements?: DialectElement[];
  /** Chain rules: "narrative immediately after X becomes Y". */
  chain?: ChainRule[];
  /** Editor-overlay transition rows — never travels beyond tooling. */
  transitions?: TransitionRow[];
  /** Editor-overlay templates (picker key, blank-tab behavior, labels). */
  templates?: Templates;
}

// ── Speculative evaluation (F4.3, docs/speculative-eval-spec.md) ─────
//
// `WebSpeculation`'s wire shapes — a sandboxed, side-effect-proof fork of a
// running story (`StoryRunnerHandle.speculate()`), driven by its own
// composable verbs (`goToPath`/`advance`/`choose`/`evalFunction`/
// `resumeFunctionEval`/…). `TypedValue` is the richer sibling of
// `ExternalValue`: a binding argument only ever needs a scalar, but an
// `evalFunction` result is useful information worth keeping structured
// (a list's member names, a divert's destination) rather than collapsing
// to `null` the way the binding boundary does.

/** A structured ink value, richer than {@link ExternalValue} (which collapses
 * lists and divert targets to `null`). Returned by `SpeculationHandle`'s
 * `evalFunction`/`resumeFunctionEval` and the `evaluate()` convenience. */
export type TypedValue =
  | { type: "int"; value: number }
  | { type: "float"; value: number }
  | { type: "bool"; value: boolean }
  | { type: "string"; value: string }
  | { type: "null" }
  | { type: "list"; items: ListMember[] }
  | { type: "divert"; path?: string };

/** One active member of a `"list"`-typed value. */
export interface ListMember {
  /** The origin list's declared name (e.g. `"Weekday"`). */
  origin: string;
  /** The item's unqualified display name (e.g. `"Monday"`). */
  name: string;
  /** The item's ordinal within its origin list. */
  ordinal: number;
}

/** Outcome of `SpeculationHandle.evalFunction`/`resumeFunctionEval`. */
export type SpeculationFunctionEval =
  | { type: "returned"; value: TypedValue }
  | { type: "awaiting_external"; name?: string };

/** Which evaluation regime a speculation's external-kind tiering gates for
 * (see `docs/speculative-eval-spec.md` §7 / `KindTieredHandler`). `"watch"`
 * (default): `effect`-kind externals never fire live. `"eval"`: `effect`
 * externals fire live only when `liveEffects` is also set. */
export type SpeculationContext = "watch" | "eval";

/** Per-external policy tiering a speculation gates its externals by. A name
 * absent from this map is conservatively treated as `"effect"`. */
export type SpeculationKinds = Record<string, "query" | "effect">;

/** Options for `StoryRunnerHandle.speculate()`. All fields optional. */
export interface SpeculationOptions {
  /** VM step budget for a single `advance()` call. Default 100,000. */
  steps?: number;
  /** Total visible-line budget across this speculation's lifetime. Default 1,000. */
  lines?: number;
  /** Default `"watch"`. */
  context?: SpeculationContext;
  /** Arm `effect`-kind externals; only takes effect under `context: "eval"`.
   * Default `false`. */
  liveEffects?: boolean;
  /** Per-external `"query"`/`"effect"` policy tiering. Default `{}` (every
   * external conservatively treated as `"effect"`). */
  kinds?: SpeculationKinds;
}

/** Which externals a speculation let through live versus fell back, across
 * every verb call made on it so far. Diagnostic only. */
export interface SpeculationExternalsReport {
  live: string[];
  fallback: string[];
}

/** One resolved transcript line from a speculation — `SpeculationHandle`'s
 * own shape (no `type` discriminant, unlike {@link SessionLine}: a
 * speculation's transcript is a plain resolved log, not a step outcome). */
export interface SpeculationLine {
  text: string;
  tags: string[];
}

/** Result of the thin `evaluate()` convenience — composes `SpeculationHandle`'s
 * verbs into a single call for the common cases (a knot path, or a function
 * call with literal arguments). */
export interface SpeculationResult {
  /** Present when `source` was a function call. */
  value?: TypedValue;
  /** Resolved `(text, tags)` lines produced by the run. */
  transcript: SpeculationLine[];
  /** Present when the run stopped at a choice point. */
  reachedChoices?: Choice[];
  /** An abort (`opts.signal`) rejects the `evaluate()` promise instead of
   * resolving with a stop value — there is no `"aborted"` variant here. */
  stop: "completed" | "choices" | "step-budget" | "line-budget";
  externals: SpeculationExternalsReport;
  /** Non-empty only when `source` failed to compile as either an expression
   * or content fragment (Tier-1, `docs/speculative-eval-spec.md`'s
   * "mechanism B"), or when it needed Tier-1 but no `opts.projectSource` was
   * supplied; `stop`/`transcript`/`value` are meaningless in that case. */
  diagnostics: string[];
}

/**
 * A project's ink source, supplied by the caller for Tier-1 speculative
 * evaluation (`StoryRunnerHandle.evaluate()`'s fragment path, F5.1): a
 * `StoryRunner` only ever holds an already-linked program, not the file set
 * it was compiled from, so evaluating an arbitrary author-typed fragment
 * (anything beyond a bare knot path or literal-arg call — Tier 0) needs the
 * consumer to hand back the same sources the running program was last
 * compiled from, so the fragment resolves against the live project's real
 * globals/knots/lists.
 */
export interface ProjectSource {
  /** The entry file's path — must be a key of `files`. The fragment's
   * synthetic knot/function is appended to this file's content before
   * recompiling; every other file is served verbatim. */
  entry: string;
  /** Every source file in the project, keyed by path exactly as its
   * `INCLUDE` directives name it. */
  files: Record<string, string>;
}
