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
// base both @brink/ink-editor (the registry) and @brink/studio-shell (the
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

export interface FoldRange {
  start_line: number;
  end_line: number;
  collapsed_text?: string;
  /** Whole-line declaration fold (docs + header + body): fold from the start
   *  of start_line and render the hidden header as the placeholder. */
  from_line_start?: boolean;
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

export interface MoveResult {
  ok: boolean;
  /** The file path this result applies to. */
  path?: string;
  new_source?: string;
  cross_file_edits: CrossFileEdit[];
  error?: string;
}

// ── Safe-rename types (#305) ────────────────────────────────────

/** One entry in a rename's breakage report — a diagnostic the rename would
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

/** A `MoveResult` extended with the safe-rename gate. `safe` is true when the
 * rename introduces no new diagnostics; otherwise `introduced_diagnostics`
 * holds the breakage report and the edits apply only on an explicit force. */
export interface SymbolRenameResult extends MoveResult {
  introduced_diagnostics: RenameDiagnostic[];
  safe: boolean;
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

/** A directed story-graph edge between node ids. Function calls are excluded. */
export interface StoryGraphEdge {
  from: string;
  to: string;
  kind: StoryGraphEdgeKind;
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
  block_comment: boolean;
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
