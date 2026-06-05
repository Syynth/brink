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

export type LineType = "text" | "done" | "choices" | "end";

export interface Line {
  type: LineType;
  text: string;
  tags: string[];
  choices?: Choice[];
}

export interface Choice {
  index: number;
  text: string;
  tags: string[];
}

// ── IDE types ───────────────────────────────────────────────────

export interface CompletionItem {
  name: string;
  kind: string;
  detail?: string;
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

export interface CodeAction {
  title: string;
  kind: string;
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

// ── Multi-file project types ────────────────────────────────────

export interface ProjectFile {
  path: string;
}

export interface FileOutline {
  path: string;
  symbols: DocumentSymbol[];
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
