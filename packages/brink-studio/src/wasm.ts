import init, {
  compile as wasmCompile,
  semantic_tokens,
  token_type_names,
  token_modifier_names,
  completions as wasmCompletions,
  hover as wasmHover,
  goto_definition as wasmGotoDef,
  find_references as wasmFindRefs,
  prepare_rename as wasmPrepareRename,
  rename as wasmRename,
  code_actions as wasmCodeActions,
  inlay_hints as wasmInlayHints,
  signature_help as wasmSignatureHelp,
  folding_ranges as wasmFoldingRanges,
  document_symbols as wasmDocumentSymbols,
  format_document as wasmFormatDocument,
  StoryRunner,
} from "brink-web";

// ── Existing types ─────────────────────────────────────────────────

export interface Diagnostic {
  start: number;
  end: number;
  message: string;
  severity: "Error" | "Warning";
}

export interface CompileResult {
  ok: boolean;
  story_bytes?: number[];
  warnings?: Diagnostic[];
  error?: string;
}

export interface SemanticToken {
  line: number;
  start_char: number;
  length: number;
  token_type: number;
  token_modifiers: number;
}

export interface StepResult {
  status: "continue" | "choices" | "ended";
  text?: string;
  choices?: Choice[];
}

export interface Choice {
  index: number;
  text: string;
}

// ── IDE types ──────────────────────────────────────────────────────

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
  children: DocumentSymbol[];
}

export interface CodeAction {
  title: string;
  kind: string;
}

// ── Wasm initialization ────────────────────────────────────────────

export async function initWasm(): Promise<void> {
  await init();
}

// ── Compilation ────────────────────────────────────────────────────

export function compile(source: string): CompileResult {
  const json = wasmCompile(source);
  return JSON.parse(json) as CompileResult;
}

// ── Semantic tokens ────────────────────────────────────────────────

export function getSemanticTokens(source: string): SemanticToken[] {
  const json = semantic_tokens(source);
  return JSON.parse(json) as SemanticToken[];
}

let cachedTypeNames: string[] | null = null;
let cachedModifierNames: string[] | null = null;

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

// ── IDE features ───────────────────────────────────────────────────

export function getCompletions(source: string, offset: number): CompletionItem[] {
  const json = wasmCompletions(source, offset);
  return JSON.parse(json) as CompletionItem[];
}

export function getHover(source: string, offset: number): HoverInfo | null {
  const json = wasmHover(source, offset);
  const result = JSON.parse(json);
  return result ?? null;
}

export function gotoDefinition(source: string, offset: number): Location | null {
  const json = wasmGotoDef(source, offset);
  const result = JSON.parse(json);
  return result ?? null;
}

export function findReferences(source: string, offset: number): Location[] {
  const json = wasmFindRefs(source, offset);
  return JSON.parse(json) as Location[];
}

export function prepareRename(source: string, offset: number): Location | null {
  const json = wasmPrepareRename(source, offset);
  const result = JSON.parse(json);
  return result ?? null;
}

export function doRename(source: string, offset: number, newName: string): FileEdit[] {
  const json = wasmRename(source, offset, newName);
  return JSON.parse(json) as FileEdit[];
}

export function getCodeActions(source: string, offset: number): CodeAction[] {
  const json = wasmCodeActions(source, offset);
  return JSON.parse(json) as CodeAction[];
}

export function getInlayHints(source: string, start: number, end: number): InlayHint[] {
  const json = wasmInlayHints(source, start, end);
  return JSON.parse(json) as InlayHint[];
}

export function getSignatureHelp(source: string, offset: number): SignatureInfo | null {
  const json = wasmSignatureHelp(source, offset);
  const result = JSON.parse(json);
  return result ?? null;
}

export function getFoldingRanges(source: string): FoldRange[] {
  const json = wasmFoldingRanges(source);
  return JSON.parse(json) as FoldRange[];
}

export function getDocumentSymbols(source: string): DocumentSymbol[] {
  const json = wasmDocumentSymbols(source);
  return JSON.parse(json) as DocumentSymbol[];
}

export function formatDocument(source: string): string {
  const json = wasmFormatDocument(source);
  return JSON.parse(json) as string;
}

// ── Story runner ───────────────────────────────────────────────────

export class StoryRunnerHandle {
  private runner: StoryRunner;

  constructor(storyBytes: Uint8Array) {
    this.runner = new StoryRunner(storyBytes);
  }

  continueStory(): StepResult {
    const json = this.runner.continue_story();
    return JSON.parse(json) as StepResult;
  }

  choose(index: number): void {
    this.runner.choose(index);
  }

  reset(): void {
    this.runner.reset();
  }

  free(): void {
    this.runner.free();
  }
}
