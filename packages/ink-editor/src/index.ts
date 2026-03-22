// Component
export { InkEditor } from "./InkEditor.js";
export type { InkEditorProps, InkEditorHandle, KeyHint } from "./InkEditor.js";

// Editor internals (used by store and for state management)
export { brinkStudio } from "./extensions.js";
export type { BrinkStudioOptions } from "./extensions.js";

// Types for line classification
export { elementTypeField, ElementType, setEditorSession } from "./element-type.js";
export type { LineInfo } from "./element-type.js";

// State management
export { EditorStateManager } from "./state-manager.js";
export type { TabTarget, TabInfo } from "./state-manager.js";

// Project session
export { ProjectSession } from "./project-session.js";
export type { ProjectSessionOptions } from "./project-session.js";

// Provider
export { InMemoryFileProvider } from "./provider.js";
export type { FileProvider } from "./provider.js";

// Theme
export { brinkTheme } from "./theme.js";

// Convert (CM6 dispatch version)
export { convertLineToType, CONVERTIBLE_TYPES, extractLineContent, getLineSigilRange } from "./convert.js";

// Transition helpers (for external update listeners)
export { getHintsForElement, lineHasContent, buildContext } from "./transitions.js";
