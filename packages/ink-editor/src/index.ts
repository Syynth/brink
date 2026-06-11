// Editor extension bundle (per-view options; see document-sessions.ts)
export { brinkStudio } from "./extensions.js";
export type { BrinkStudioOptions } from "./extensions.js";

// Types for line classification
export { elementTypeField, ElementType } from "./element-type.js";
export type { LineInfo } from "./element-type.js";

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
export type { FileChange, FileChangeType, FileChangeHubOptions } from "./file-change-hub.js";

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
