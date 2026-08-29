// Editor extension bundle (per-view options; see document-sessions.ts)
export { brinkStudio, setDialect } from "./extensions.js";
export type { BrinkStudioOptions } from "./extensions.js";
export { DEFAULT_INDENT } from "./extensions.js";

// HIR structural overlay (#454): the extension plus identity-query helpers
// for hosts building on the projection (spans at a position, symbol identity).
// `refreshHirOverlay` / `refreshHirOverlayEffect` (#494) force a projection
// re-read without a doc change — `DocumentSessions` fires it automatically on
// compile delivery; hosts with custom wiring dispatch it from their own
// compile-complete signal.
export {
  hirOverlayExtension,
  hirSpansAt,
  hirIdentityAt,
  RAIL_LANE_WIDTH_PX,
  refreshHirOverlay,
  refreshHirOverlayEffect,
} from "./hir-overlay.js";
export type { HirOverlayOptions } from "./hir-overlay.js";

// Prose checking (#3209). The seam only — the engine is a separate, lazily
// loaded wasm module the HOST registers, so this package never depends on it.
export { proseExtension, proseRangesOf, withoutCueLines, refreshProseEffect } from "./prose.js";
export type { ProseChecker, ProseLint, ProseOptions, ProseRange } from "./prose.js";
export { diagnosticSources, publishDiagnostics, diagnosticsFrom } from "./diagnostic-sources.js";
export type { DiagnosticSource } from "./diagnostic-sources.js";

// Dialogue dialect (#368): the pure-JSON schema, the at-cue preset, and
// `extendDialect` for adding a kind without forking the preset. Classifier
// internals (`ResolvedDialect`, `validateDialect`, `compileAffix`, …) are
// exported for advanced hosts/tests but the day-to-day surface is the
// `dialect` mount option (`BrinkStudioOptions.dialect`) + `setDialect`.
export {
  AT_CUE_DIALECT,
  extendDialect,
  ResolvedDialect,
  validateDialect,
  compileAffix,
  resolveSourceShape,
  reservedStructuralKinds,
  DialectParser,
  detectCast,
} from "./dialect.js";
export type {
  DialogueDialect,
  DialectElement,
  ChainRule,
  SourceShape,
  PatternShape,
  AffixShape,
  TransitionRow,
  TransitionAction,
  TemplateEntry,
  Templates,
  ElementNature,
  DialectMatch,
  DialectValidationError,
  SourceLine,
  EmittedSegment,
} from "./dialect.js";

// Tier-1 boundary helpers (#369): the canonical positional diagnostic sort
// (file → offset → errors-first) and offset → 1-based line:col. Presentation
// ORDER is a host choice layered on the canonical positional sort — hosts may
// re-group (severity-first, per-file, …) on top.
export { sortDiagnostics, lineColAt } from "./boundary.js";
// Module-identity re-exports (#369): compile-result types re-exported from
// @brink-lang/web so hosts consuming them through the editor get the same
// module identity as code importing @brink-lang/web directly — no structural
// "CompileResultLike" shims needed.
export type { CompileResult, Diagnostic } from "@brink-lang/web";

// Inline rename (#323/#324): the in-editor rename widget + its pure badge/report
// logic (unit-tested via @brink-lang/studio).
export {
  renameExtension,
  startInlineRename,
  startInlineRenameEffect,
  isSafeRename,
  breakageCount,
  breakageEntries,
  RenameQueryCache,
} from "./rename.js";
export type {
  RenameOptions,
  BreakageEntry,
  BreakageContext,
} from "./rename.js";

// Shared inline name-prompt primitive (#315 H): the chip + "⚠ breaks N" badge +
// inline breakage report behind both inline rename and extract-to-knot/function.
export { InlineNameInput } from "./inline-name-input.js";
export type {
  InlineNameInputOptions,
  InlineNameBreakageContext,
} from "./inline-name-input.js";

// Idle-callback scheduling (#722): lets a heavy synchronous wasm call (a
// breakage/collision analysis) be kicked off a tick after the caller has
// already painted whatever "pending" UI it needs, instead of running inline
// in the same frame as the triggering event. `InlineNameInput` (above) is
// the in-tree consumer; exported so other rename/analysis surfaces — e.g.
// the modal `SymbolRenamePrompt` (#696) — can take the same off-paint-path
// discipline instead of re-implementing it.
export { scheduleIdleWork, cancelIdleWork } from "./idle-schedule.js";
export type { IdleHandle } from "./idle-schedule.js";

// Extract selection → knot/function code actions (#315 H): the code-actions
// menu entries + the name-prompt → wasm extract → apply flow.
export {
  extractCodeActions,
  isExtractAction,
  EXTRACT_TO_KNOT_ACTION,
  EXTRACT_TO_FUNCTION_ACTION,
} from "./extract-actions.js";
export type { ExtractActionsOptions, ExtractKind } from "./extract-actions.js";

// Argument Form (argument-widget spec §1.2) — opened from the in-editor glyph
// and from a host's tool windows (e.g. the Host Functions panel launcher).
export { openArgumentForm } from "./argument-form.js";
export type { FormField, FormGroup, ArgumentFormOptions } from "./argument-form.js";
export type { FormGlyphMode } from "./argument-widgets.js";
// Live source range of an argument literal (quoted or bare) — host-widget edits.
// `openCallForm` opens the whole-call Form for a resolved `CallWidgetSite`
// (the in-editor glyph / hover-card entry point); `matchHostWidget` is the
// slot → host-widget resolution it (and the CM decorations) share, including
// the base-type fallback (argument-widget-spec §3.1, #990).
export { liveArgRange, openCallForm, matchHostWidget } from "./argument-widgets.js";
// Host argument widgets (argument-widget-spec §3, §3.1): registered at mount
// from StudioExtensions.argumentWidgets. `type` may be a `host.<vendor>.<name>`
// semantic id or a base type (`bool`/`int`/`float`/`string`).
export { setHostWidgets, getHostWidget } from "./widget-registry.js";

// Types for line classification
export { elementTypeField, ElementType, assignOptionPaths, classifyLine } from "./element-type.js";
export type { LineInfo, DialectGeometry } from "./element-type.js";

// Code folding (#313 G): HIR-driven fold ranges, including the leading
// INCLUDE-block fold whose Rust `collapsed_text` renders as its placeholder.
// Fold kinds (#365): structural/machinery/narrative, a live-reconfigurable
// active-kinds set, and bulk fold/unfold-by-kind commands for a host's
// mode-entry auto-collapse (never forced by the extension itself).
export {
  foldingExtension,
  foldAllOfKind,
  unfoldAllOfKind,
  setActiveFoldKinds,
  activeFoldKindsFacet,
  activeFoldKindsCompartment,
} from "./folding.js";
export type { FoldingOptions, FoldPlaceholder, FoldKind, DeclKind } from "./folding.js";

// "Play from here" (#186): hover ▶ gutter + right-click menu on knot/stitch
// declarations. `qualifiedInkPath`/`headerName` are the pure path core.
export {
  playFromHereExtension,
  qualifiedInkPath,
  headerName,
  lineActionsAt,
} from "./play-from-here.js";
export type {
  PlayFromHereOptions,
  TextMenuRequest,
  IdentityMenuSection,
  LineMenuAction,
} from "./play-from-here.js";

// Host gutter markers (#343): host-contributed gutter affordances
// (breakpoints, annotations) in a slot coordinated with the built-in gutters.
// Wired via `BrinkStudioOptions.getGutterMarkers` / `onGutterMarkerClick`;
// standalone `hostGutterExtension` for hosts composing extensions directly.
export {
  hostGutterExtension,
  refreshGutterMarkers,
  refreshGutterMarkersEffect,
} from "./host-gutter.js";
export type { HostGutterMarker, HostGutterOptions } from "./host-gutter.js";

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
export type {
  DocTarget,
  DocumentCallbacks,
  DocumentSessionsOptions,
  KeyHint,
  ViewStateSnapshot,
} from "./document-sessions.js";

// Project session
export { ProjectSession } from "./project-session.js";
export type {
  ProjectSessionOptions,
  RenameFileResult,
  RenameDirResult,
} from "./project-session.js";

// File-change egress (issues #154/#137): the shared notify seam.
export { FileChangeHub } from "./file-change-hub.js";
export { OverlayPersistence } from "./persistence.js";
export type {
  BackupEntry,
  BackupMeta,
  BackupSink,
  CanonicalStore,
  OverlayPersistenceOptions,
  PersistenceSession,
} from "./persistence.js";
export type {
  FileChange,
  FileChangeType,
  FileChangeHubOptions,
  FileConflict,
} from "./file-change-hub.js";

// Provider
export { InMemoryFileProvider } from "./provider.js";
export type { FileProvider } from "./provider.js";

// Theme (opt-in — pass `theme: false` to brinkStudio for a headless editor, #363)
export { brinkTheme } from "./theme.js";
// Structural (non-skin) stylesheet — always-on, zero-specificity, injected on
// demand; exported for hosts mounting editor popups into another document.
export { ensureStructuralStyles } from "./structural-styles.js";

// Convert (CM6 dispatch version)
export { convertLineToType, CONVERTIBLE_TYPES, extractLineContent, getLineSigilRange } from "./convert.js";
export type { ConvertibleShape } from "./convert.js";

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
  locationsToSearchResult,
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

// Editable search results buffer (#322 Track V, design D): the Zed-style
// editor-owned buffer. `buildResultsRows` / `mapRowEditToSource` are the pure,
// unit-testable model; `SearchResultsBuffer` is the self-contained CM6 surface
// that routes match-row edits back to the source via the apply-edits seam.
export {
  buildResultsRows,
  mapRowEditToSource,
  SearchResultsBuffer,
  DEFAULT_COMMIT_DELAY_MS,
} from "./search-results-buffer.js";
export type {
  ResultRow,
  ResultsBufferModel,
  SearchResultsBufferOptions,
} from "./search-results-buffer.js";

// Per-match result cards (docs/search-results-cards-spec.md, PR C): the
// card's own small CM6 buffer (visible cards) plus the static-HTML segment
// model (off-screen / collapsed cards), sharing the host's per-file
// semantic-token cache.
export { SearchCardBuffer, cardLineSegments } from "./search-card.js";
export type {
  CardLineSegment,
  SearchCardBufferOptions,
  SearchCardHighlight,
  SearchCardModel,
} from "./search-card.js";

// Extensible inline-markup rules (#367): host-registered patterns decorated
// as `brink-markup-<name>` marks, scoped to the narrative content regions of
// classified lines (never over ink syntax). Ships zero rules by default;
// `rmmzAngleTagRule` is the optional angle-tag preset. `contentRegions` is the
// pure, unit-testable scoping core.
export { inlineMarkup, contentRegions, rmmzAngleTagRule } from "./inline-markup.js";
export type {
  InlineMarkupRule,
  InlineMarkupPatternRule,
  InlineMarkupPairRule,
  MarkupRegion,
} from "./inline-markup.js";

// Find panel (#319 Track N): opt-in @codemirror/search factory. Not auto-enabled
// in the studio editor — hosts opt in by adding the returned extension.
export { findPanel } from "./find-panel.js";
export type { FindPanelOptions } from "./find-panel.js";

// External-conflict merge view (#320 Track V): self-contained banner +
// side-by-side 2-way @codemirror/merge surface for a kept-buffer conflict.
// Framework-agnostic — the studio mounts it into a host container.
export { ConflictView } from "./conflict-view.js";
export type { ConflictViewOptions } from "./conflict-view.js";

// Performance probe (measure-first ruling 2026-08-24; prod-perf ruling
// 2026-08-25): the shared collector + observers behind the desktop perf
// work. Hosts enable collection at mount in ALL builds by default
// (`MountStudioOptions.perf: false` opts out); everything is inert
// branches while disabled, and bounded while enabled. The worker realm's
// own state reports through the host-level `hostPerfReport` query
// (`HostPerfBundle`, session-host.ts).
export {
  setPerfEnabled,
  isPerfEnabled,
  perfSpan,
  perfTime,
  perfRecord,
  perfMark,
  perfReport,
  perfReset,
} from "./perf/probe.js";
export type { PerfReport, PerfSpanAggregate, PerfRawSpan } from "./perf/probe.js";
export { attachPerfObservers } from "./perf/observers.js";
export { perfViewportProbe } from "./perf/viewport-probe.js";
export { withPerfTiming } from "./perf/wasm-proxy.js";

// Detached gutters (#3119): the WebKit editor-layout fix — gutters leave
// the scroller's flex/sticky flow, with the horizontal space paid back as
// content padding. Self-gating on line wrapping; included in the studio's
// slot extensions, exported for hosts building their own views.
export { detachedGutters } from "./gutter-layout.js";

// Session protocol substrate (docs/editor-worker-spec.md §5, W1): the
// async facade + transports consumers migrate onto in W2. Wire shapes
// come from @brink/wasm-types (Rust source of truth: brink-web protocol.rs).
export {
  SessionClient,
  QueryDroppedError,
  QueryFailedError,
} from "./worker/session-client.js";
export type { QueryHandle, QueryOptions, QueryResult } from "./worker/session-client.js";
export { LocalTransport, jsonRoundTrip } from "./worker/local-transport.js";
export type { SessionServerLike } from "./worker/local-transport.js";
export { SessionHostCore } from "./worker/session-host.js";
export type { HostPerfBundle } from "./worker/session-host.js";
export { WorkerTransport, createSessionWorker } from "./worker/worker-transport.js";
export type { WorkerLike } from "./worker/worker-transport.js";
export type { SessionTransport } from "./worker/transport.js";
export { AdmissionScheduler } from "./worker/scheduler.js";
export type { SchedulerAction } from "./worker/scheduler.js";

// Editor text size (beta feedback 2026-08-25): the CM6 theme reads
// `--bs-editor-font-size`; these are the default, bounds, and the shared
// clamp so the store and the settings parser cannot disagree.
export {
  DEFAULT_APP_FONT_SIZE,
  MIN_APP_FONT_SIZE,
  MAX_APP_FONT_SIZE,
  clampAppFontSize,
  DEFAULT_EDITOR_FONT_SIZE,
  MIN_EDITOR_FONT_SIZE,
  MAX_EDITOR_FONT_SIZE,
  clampEditorFontSize,
} from "./theme.js";
export { renderHoverContent } from "./hover.js";
