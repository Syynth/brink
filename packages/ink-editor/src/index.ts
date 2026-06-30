// Editor extension bundle (per-view options; see document-sessions.ts)
export { brinkStudio } from "./extensions.js";
export type { BrinkStudioOptions } from "./extensions.js";

// Argument Form (argument-widget spec §1.2) — opened from the in-editor glyph
// and from a host's tool windows (e.g. the Host Functions panel launcher).
export { openArgumentForm } from "./argument-form.js";
export type { FormField, FormGroup, ArgumentFormOptions } from "./argument-form.js";
export type { FormGlyphMode } from "./argument-widgets.js";
// Live source range of an argument literal (quoted or bare) — host-widget edits.
export { liveArgRange } from "./argument-widgets.js";
// Host argument widgets (argument-widget-spec §3): registered at mount from
// StudioExtensions.argumentWidgets.
export { setHostWidgets } from "./widget-registry.js";

// Types for line classification
export { elementTypeField, ElementType } from "./element-type.js";
export type { LineInfo } from "./element-type.js";

// "Play from here" (#186): hover ▶ gutter + right-click menu on knot/stitch
// declarations. `qualifiedInkPath`/`headerName` are the pure path core.
export { playFromHereExtension, qualifiedInkPath, headerName } from "./play-from-here.js";
export type { PlayFromHereOptions } from "./play-from-here.js";

// Per-view wasm document handles (issue #122 / #90)
export {
  DocHandle,
  documentHandleFacet,
  syncAnnotation,
} from "./document-handle.js";
export type { DocumentHandleSlot } from "./document-handle.js";

// Per-(document, group) view management
export {
  DocumentSessions,
  docKeyFor,
  docTitleFor,
  parseDocKey,
} from "./document-sessions.js";
export type { DocTarget, DocumentCallbacks, KeyHint } from "./document-sessions.js";

// Project session
export { ProjectSession } from "./project-session.js";
export type { ProjectSessionOptions } from "./project-session.js";

// File-change egress (issues #154/#137): the shared notify seam.
export { FileChangeHub } from "./file-change-hub.js";
export type {
  FileChange,
  FileChangeType,
  FileChangeHubOptions,
  FileConflict,
} from "./file-change-hub.js";

// Provider
export { InMemoryFileProvider } from "./provider.js";
export type { FileProvider } from "./provider.js";

// Theme
export { brinkTheme } from "./theme.js";

// Convert (CM6 dispatch version)
export { convertLineToType, CONVERTIBLE_TYPES, extractLineContent, getLineSigilRange } from "./convert.js";

// Transition helpers (for external update listeners)
export { getHintsForElement, lineHasContent, buildContext } from "./transitions.js";
export type { ElementConverter } from "./transitions.js";

// Completion kind → CodeMirror completion type (icon + auto-open keying)
export { completionType, toCompletionOption } from "./completions.js";

// Project-wide search engine (issue #94 / #322): framework-agnostic, pure
// string search over file sources — no CodeMirror/React involvement. UTF-16
// offsets match `editor.reveal` source spans so a match can be dispatched to
// the navigation protocol verbatim.
export {
  DEFAULT_SEARCH_OPTIONS,
  SEARCH_CONTEXT_BEFORE,
  SEARCH_RESULT_CAP,
  applyReplacements,
  buildSearchPattern,
  escapeRegExp,
  matchLineSegments,
  replacementTextFor,
  searchSources,
} from "./project-search.js";
export type {
  FileSearchResult,
  MatchLineSegments,
  ProjectSearchResult,
  ReplacementEdit,
  SearchMatch,
  SearchPatternResult,
  SearchQueryOptions,
} from "./project-search.js";

// Find panel (#319 Track N): opt-in @codemirror/search factory. Not auto-enabled
// in the studio editor — hosts opt in by adding the returned extension.
export { findPanel } from "./find-panel.js";
export type { FindPanelOptions } from "./find-panel.js";
