// @brink-lang/studio — the public embedding surface (docs/embedder-api.md,
// docs/studio-shell-spec.md §8).
//
// Hosts mount the studio with `mountStudio` and extend it through the
// `StudioExtensions` config; host components talk to the studio through the
// `StudioApi` facade. The studio's Zustand store is deliberately NOT
// exported (spec §8.2): hosts observe `StudioPublicState` — an explicit,
// versioned subset — and anything else they need is an API addition, not a
// store leak.

// ── Mount (the embedding entry point) ───────────────────────────
export { mountStudio, type MountStudioOptions, type StudioHandle } from "./mount.js";

// ── File-content egress (issue #154) ────────────────────────────
// The `onFilesChanged` mount option's payload shapes, plus the save
// command ids (dispatchable via `api.dispatch`).
export type { FileChange, FileChangeType } from "@brink/ink-editor";
export { FILE_SAVE_COMMAND_ID, FILE_SAVE_ALL_COMMAND_ID } from "./file-commands.js";

// ── Extension config (spec §8.1) ────────────────────────────────
export type {
  StudioExtensions,
  // Item shapes for authoring extensions.
  Command,
  ToolWindowDescriptor,
  StatusBarItemDescriptor,
  StatusBarAlignment,
  Placement,
  Dock,
  Section,
  // Navigation protocol (§6.1): the editor.reveal argument shape.
  Location,
  Span,
  // Notification service (§7.5): notify() input/handle shapes.
  Notification,
  NotificationAction,
  NotificationHandle,
  NotificationInput,
  NotificationSeverity,
} from "@brink/studio-shell";

// ── StudioApi facade (spec §8.2) ────────────────────────────────
export {
  useStudioApi,
  type PublicElementInfo,
  type StudioApi,
  type StudioPublicState,
} from "@brink/studio-ui";

// ── Example extension (worked example, issues #95/#146) ─────────
export {
  createExampleExtension,
  manifestPanelItems,
  EXAMPLE_HOST_MANIFEST,
  EXAMPLE_REVEAL_COMMAND_ID,
  EXAMPLE_TOOL_WINDOW_ID,
  type HostFunctionItem,
} from "./example-extension.js";

// ── Compiler/runtime wasm bindings (no studio state involved) ────
//
// Lower-level building blocks for hosts that drive the compiler or a story
// directly (the docs/book examples): wasm init + handles and their result
// types. These carry no studio UI state.
export {
  initWasm,
  compile,
  EditorSessionHandle,
  StoryRunnerHandle,
} from "@brink-lang/web";
export type {
  CompileResult,
  Diagnostic,
  Line,
  LineType,
  Choice,
  // Host-capability manifest shapes (docs/host-capability-manifest.md) —
  // the `hostManifest` mount option's type and its parts.
  HostManifest,
  ManifestExternal,
  ManifestParam,
  SemanticTypeDef,
} from "@brink-lang/web";
