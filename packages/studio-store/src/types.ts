/**
 * Local type definitions for studio-store.
 *
 * Types that will eventually come from @brink/ink-editor are defined here
 * as forward-compatible stubs until that package is created.
 */

// ── Element types (mirrors brink-studio/editor/element-type.ts) ──────

export enum ElementType {
  KnotHeader,
  StitchHeader,
  NarrativeText,
  Choice,
  ChoiceBody,
  Gather,
  Divert,
  Logic,
  VarDecl,
  Comment,
  Include,
  External,
  Tag,
  Blank,
  Character,
  Parenthetical,
  Dialogue,
}

export interface LineInfo {
  type: ElementType;
  depth: number;
  /** Whether the choice/gather uses sticky (+) sigils. */
  sticky: boolean;
  /** Whether a divert is standalone (just "-> target", not a tunnel). */
  standalone: boolean;
}

// ── Key hints ────────────────────────────────────────────────────────

export interface KeyHint {
  key: string;
  hint: string;
}

// ── Tab types ────────────────────────────────────────────────────────

export type TabTarget =
  | { kind: "file"; path: string }
  | { kind: "symbol"; path: string; name: string; start: number; end: number };

export interface TabInfo {
  id: string;
  target: TabTarget;
  pinned: boolean;
  label: string;
}

// ── Editor handle (will come from @brink/ink-editor) ─────────────────

export interface InkEditorHandle {
  focus(): void;
  getContent(): string;
  triggerCompile(): void;
  setContent(content: string): void;
  scrollTo(pos: number): void;
  convertLineToType(sigil: string): void;
}

// ── Editor types (the real classes from @brink/ink-editor) ───────────

export type { EditorStateManager, ProjectSession } from "@brink/ink-editor";
