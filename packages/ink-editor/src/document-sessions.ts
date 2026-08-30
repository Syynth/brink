/**
 * DocumentSessions — per-(document, group) CM6 views over wasm doc handles.
 *
 * Replaces the old EditorStateManager. The shell's editor-groups store owns
 * tab *structure* (which documents are open where, pin/preview state); this
 * class owns text-document *content* machinery:
 *
 * - one wasm document handle per mounted view (`open_document` /
 *   `open_fragment`), opened on mount and closed on unmount — IDE
 *   intelligence works in every group simultaneously, no active-file
 *   choreography (see document-handle.ts);
 * - cached EditorStates per (docKey, groupId) so backgrounded tabs keep
 *   selection/scroll/undo, rebuilt from the wasm session's authoritative
 *   content when it changed underneath;
 * - live same-document mirroring between split views (the CM6 sync-dispatch
 *   pattern: forward `update.changes` with `syncAnnotation`, selection and
 *   scroll stay per-view);
 * - fragment⇄file mirroring across views of the same file via the change
 *   specs `update_document` returns (#122), with refresh-from-file as the
 *   fallback;
 * - focused-view tracking: cursor/line-info reporting, the compile trigger,
 *   element conversion, and the e2e `__brinkView` hook all target the
 *   focused group's active view.
 *
 * Document keys reuse the old tab-id scheme: `"main.ink"` for file documents,
 * `"main.ink::start"` for symbol (fragment) documents.
 */

import { docString } from "./doc-string";
import {
  refreshBreakpoints as refreshBreakpointsInView,
  type BreakpointGutterMarker,
} from "./play-from-here.js";
import {
  refreshExecutionHighlight as refreshExecutionHighlightInView,
  type ExecutionHighlight,
} from "./execution-highlight.js";
import { EditorState, type ChangeSet, type Extension } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { defaultKeymap } from "@codemirror/commands";
import type {
  CodeAction,
  CompileResult,
  CompletionItem,
  DialogueDialect,
  DocumentChangeSpec,
  DocumentSymbol,
  HoverInfo,
  Location,
  SignatureInfo,
  StructuralResult,
} from "@brink/wasm-types";
import type { ExtractKind } from "./extract-actions.js";
import { ClassifierSessionHandle, getTokenTypeNames } from "@brink-lang/web";
import { brinkStudio, type BrinkStudioOptions } from "./extensions.js";
import { brinkBasicSetup } from "./setup.js";
import { startInlineRename, type BreakageContext } from "./rename.js";
import {
  setFormGlyphMode,
  setFormAutoOpen,
  DEFAULT_FORM_GLYPH_MODE,
  type FormGlyphMode,
} from "./argument-widgets.js";
import { ClassifierMirror } from "./classifier-mirror.js";
import { DocHandle, syncAnnotation } from "./document-handle.js";
import { refreshHirOverlay } from "./hir-overlay.js";
import { elementTypeField, type LineInfo } from "./element-type.js";
import { getHintsForElement, lineHasContent, buildContext } from "./transitions.js";
import { convertLineToType as cmConvertLineToType } from "./convert.js";
import type { ProjectSession } from "./project-session.js";
import type { ProseChecker, ProseLint } from "./prose.js";
import { refreshProseEffect } from "./prose.js";
import { refreshDiagnosticsEffect } from "./diagnostics.js";
import { perfSpan, perfTime } from "./perf/probe.js";
import { detachedGutters } from "./gutter-layout.js";

// ── Public types ───────────────────────────────────────────────────

/** What a document key addresses (kept from the old TabTarget shape). */
export type DocTarget =
  | { kind: "file"; path: string }
  | { kind: "symbol"; path: string; name: string; start: number; end: number };

export interface KeyHint {
  key: string;
  hint: string;
}

/**
 * Per-view cursor + scroll snapshot (#347). `anchor`/`head` are view-relative
 * UTF-16 offsets (the main selection range); `scrollTop` is the view's
 * scroller pixel offset. Read via `viewState`, re-applied via
 * `restoreViewState` — the seam editor-state persistence hosts snapshot every
 * open tab through and replay after a reload.
 */
export interface ViewStateSnapshot {
  anchor: number;
  head: number;
  scrollTop: number;
}

export interface DocumentSessionsOptions {
  /** The editor skin, forwarded to `brinkStudio` (#363): absent ⇒ the default
   *  `brinkTheme`; `false` ⇒ headless (the host styles the class taxonomy);
   *  an `Extension` ⇒ a custom CM theme. */
  theme?: Extension | false;
  /** The dialogue dialect (#368), forwarded to `brinkStudio`: absent ⇒
   *  `AT_CUE_DIALECT` (byte-identical default); `null` ⇒ headless (tears
   *  down the whole screenplay layer); a `DialogueDialect` ⇒ your own
   *  convention. Applied per mounted view — use the exported `setDialect`
   *  directly on a specific view for live reconfigure. */
  dialect?: DialogueDialect | null;
  /** Indent guides, forwarded to `brinkStudio` (ruled 2026-08-23): absent ⇒
   *  on; `false` ⇒ off (a fully headless composition draws its own). */
  indentGuides?: boolean;
  /** Indent width in spaces; see BrinkStudioOptions.indent (#3149). */
  indent?: number;
  /** Prose checking (#3209): the host's checker, or absent for none.
   *  Absent is the correct default — the engine is a separate 6.5 MB wasm
   *  module and an embedder that never registers one pays nothing. */
  proseChecker?: ProseChecker | null;
  /** `american` | `british` | `canadian` | `australian`, from `[prose]`. */
  proseDialect?: () => string;
  /**
   * Store a word in the project's dictionary — the "Add to dictionary"
   * action's implementation.
   *
   * The HOST's, not the session's: the list lives in `brink.toml`
   * (decision log, "Prose dictionary lives in `brink.toml`"), and editing
   * that file is a comment-preserving structured write the embedder owns.
   * Absent means the embedder has nowhere to put it, and the action is then
   * not offered at all rather than offered and silently inert.
   */
  onAddToDictionary?: (word: string) => void;
}

export interface DocumentCallbacks {
  /** Cursor moved in the focused view (or the focused view changed). */
  onCursorChange?(line: number, col: number): void;
  /** Line element info for the focused view's cursor line. */
  onLineInfoChange?(info: LineInfo | null, hints: KeyHint[]): void;
  /** A compile finished (debounced per-view compiles and triggerCompile).
   *  May deliver the same cached result more than once — dedupe by
   *  reference if the handler is not idempotent. */
  onCompileResult?(result: CompileResult): void;
  /** A user (non-mirrored) edit landed in the view of docKey in groupId. */
  onDocEdited?(docKey: string, groupId: string): void;
  /** A view gained DOM focus — the shell should focus its group. */
  onViewFocused?(docKey: string, groupId: string): void;
  /** The focused view changed or remounted (e2e `__brinkView` hook). */
  onFocusedViewChange?(view: EditorView | null): void;
  /**
   * Prose findings for `path` changed (#3256).
   *
   * Per FILE rather than per view: two views of one document produce the
   * same findings, and a host list keyed by path is what the Problems panel
   * wants. Delivered with `[]` when a file's findings clear, so a host list
   * never keeps rows the editor has stopped showing.
   */
  onProseLints?(path: string, lints: readonly ProseLint[]): void;
  /** Goto-definition targets a different file. */
  onNavigateToFile?(location: Location): void;
  /** "Play from here" (#186): start a session entered at a knot/stitch path. */
  onPlayFrom?(inkPath: string, label?: string): void;
  /** Breakpoint dots for `path` (W4/#3297) — 1-based lines, host-owned
   *  truth; after it changes call `refreshBreakpoints()` below. */
  getBreakpoints?(path: string): readonly BreakpointGutterMarker[];
  /** Gutter click toggled a breakpoint at a 1-based line of `path`. */
  onToggleBreakpoint?(path: string, line: number): void;
  /** "Reveal in Program Explorer" (W9/#3302) — 1-based line. */
  onRevealInstructions?(path: string, line: number): void;
  /** Runtime-value hover note (W12/#3305) — see the extension option. */
  getRuntimeValueNote?(name: string): string | null;
  /** Doc edits moved `path`'s breakpoint lines (1-based old→new pairs). */
  onBreakpointsMoved?(path: string, moves: readonly { from: number; to: number }[]): void;
  /** The execution highlights for `path` (W6/#3299) — plural: a choice
   *  point lights several lines. Re-read on `refreshExecutionHighlight()`. */
  getExecutionHighlights?(path: string): readonly ExecutionHighlight[];
  /** Right-click a knot/stitch declaration → open the shared symbol context
   *  menu (`path` is injected from the focused view's file). */
  onSymbolContextMenu?(
    info: { path: string; knot: string; stitch?: string; line?: number },
    x: number,
    y: number,
  ): void;
  /** Right-click on plain editor content → the text context menu request
   *  (position + Cut/Copy/Paste/Select All bound to the right view). */
  onTextContextMenu?(request: import("./play-from-here.js").TextMenuRequest): void;
  /** Find References routes its results here (the Search panel). */
  onShowReferences?(
    symbol: string,
    locations: Location[],
    declaration?: Location | null,
  ): void;
  /** Inline rename (#323/#324): commit a safe (or forced) rename. `result` is
   *  the already-computed safe-rename payload; the host applies its edits and
   *  re-keys any open symbol tab. `path` is the file the rename ran in.
   *
   *  ⚠ `result` CAN carry `ok: false` with `safe: true` — a REFUSED rename
   *  (the op declined), not a clean one. The widget only opens where
   *  `prepareRename` resolved a range, and those renames go on to succeed, so
   *  the refusals that actually arrive here are ones that appear *after* it
   *  opened: the file unloading out from under an open rename ("file not
   *  loaded"), or the analysis⇄db identity-space mismatch guarded by
   *  `rename_refuses_rather_than_silently_dropping_edits_when_identity_spaces_disagree`
   *  (`crates/internal/brink-ide/src/rename.rs`).
   *  `result.safe` describes whether the (possibly empty) computed
   *  edits introduce new diagnostics; it says nothing about whether the op
   *  actually happened. That's `result.ok`. A refusal's `error_json` (Rust,
   *  `crates/brink-web/src/editor_refactor.rs`) sets `safe: true` with no
   *  `introduced_diagnostics`, so `isSafeRename` (`breakage.ts`) — which reads
   *  only those two fields — calls a refusal "safe" and the editor's own
   *  `settleCommit` still calls this callback (#2543).
   *
   *  **Check `result.ok` before applying anything.** On `ok: false`, do not
   *  write `new_source`/`cross_file_edits`, push an undo entry, re-key the
   *  tab, or toast success — report `result.error` as a failure instead.
   *  `@brink-lang/studio`'s own apply seam
   *  (`applyComputedRename`/`applyMoveResult`) added exactly this guard
   *  in #2543/#2564 (rationale: `docs/studio-shell-spec.md` §7.5); every other
   *  host of this callback needs the same check in its own apply seam. */
  onRenameCommit?(req: {
    path: string;
    newName: string;
    currentName: string;
    result: StructuralResult;
  }): void;
  /** Optional host override for the inline breakage report (#324) — return
   *  `true` to suppress the default inline report and render a host surface. */
  onRenameBreakage?(result: StructuralResult, ctx: BreakageContext): boolean;
  /** Apply a structural code-action / extract result (#315 H / #321 studio
   *  side): the host writes `new_source` + cross-file edits through its apply
   *  seam (`applyMoveResult`), surfacing a toast + Undo. `description` labels
   *  the undo step / toast. When absent, the code-actions menu resolves nothing
   *  (the pre-#315 dismiss-only behavior).
   *
   *  ⚠ `result` carries the same `ok: false`/`safe: true` refusal hazard
   *  documented on `onRenameCommit` above — `result.ok`, not `result.safe`,
   *  is what tells a refusal apart from a clean success. The two paths that
   *  feed this callback reach it differently, though:
   *
   *  - **Extract** (`computeExtract`/`applyExtract`): a refusal never makes
   *    it here. `computeExtract` returns `null` on `!result.ok`, and
   *    `InlineNameInput` (`inline-name-input.ts`) treats a `null` query
   *    result as "no commit" — so `applyExtract`, and therefore this
   *    callback, only ever sees `ok: true` extract results.
   *  - **Code actions** (`applyCodeAction`, #321): forwards
   *    `resolveCodeAction`'s result to this callback UNCONDITIONALLY, with
   *    no `ok` filter. A stale pick (the source or doc changed between the
   *    menu opening and the selection) reaches `resolve_code_action_impl`
   *    (`crates/brink-web/src/editor/code_actions.rs`), which can refuse
   *    with `error_json("file not loaded")`, `error_json("no source")`,
   *    `error_json("invalid code-action data: …")`, or
   *    `error_json("code action produced no change")` — all `ok: false`,
   *    `safe: true`. This is the path that actually needs the guard below.
   *
   *  **Check `result.ok` before applying anything** — the same rule
   *  `onRenameCommit` requires, for the same reason (`docs/studio-shell-spec.md`
   *  §7.5). `@brink-lang/studio`'s own apply seam (`applyMoveResult`) already
   *  has this guard (#2543/#2564); every other host of this callback needs
   *  it in its own apply seam too. */
  onApplyStructural?(req: {
    path: string;
    description: string;
    result: StructuralResult;
  }): void;
}

/** Document key for a target (old tab-id scheme). */
export function docKeyFor(target: DocTarget): string {
  return target.kind === "file" ? target.path : `${target.path}::${target.name}`;
}

/** Tab label for a target (old tab-label scheme). */
export function docTitleFor(target: DocTarget): string {
  const slash = target.path.lastIndexOf("/");
  const fileName = slash >= 0 ? target.path.substring(slash + 1) : target.path;
  if (target.kind === "file") return fileName;
  return `${target.name} (${fileName})`;
}

/** Parse a document key back into path + optional symbol name. */
export function parseDocKey(key: string): { path: string; symbol: string | null } {
  const sep = key.indexOf("::");
  if (sep < 0) return { path: key, symbol: null };
  return { path: key.slice(0, sep), symbol: key.slice(sep + 2) };
}

// ── Internal slot model ────────────────────────────────────────────

/**
 * One (docKey, groupId) view slot. Lives from first mount until pruned
 * (tab closed / group collapsed); the wasm handle and EditorView exist only
 * while mounted, the EditorState persists across unmounts. Extension
 * closures capture the slot — never the handle — so cached states stay
 * valid across handle reopen.
 */
interface ViewSlot {
  docKey: string;
  groupId: string;
  path: string;
  symbol: string | null;
  handle: DocHandle | null;
  view: EditorView | null;
  state: EditorState | null;
  /** Scroller pixel offset snapshotted at unmount — EditorState does not
   *  carry scroll, so backgrounded tabs keep it here (#347). */
  scrollTop: number;
  extensions: Extension[] | null;
}

function slotId(docKey: string, groupId: string): string {
  return `${groupId}\u0000${docKey}`;
}

/**
 * Queued next-mount work, keyed by docKey. `reveal` is the revealAt jump
 * (select + scroll-to-center + focus); `restore` re-applies a persisted
 * `ViewStateSnapshot` (#347) without stealing focus, since hosts restore
 * every open tab, not just the active one.
 */
type PendingReveal =
  | { kind: "reveal"; offset: number }
  | { kind: "restore"; anchor: number; head: number; scrollTop: number };

/**
 * Set a view's scroller pixel offset. The direct write covers already
 * laid-out views (and test DOMs); the measure-phase write survives CM's
 * initial layout pass on a freshly-mounted view, where an immediate set
 * would be clamped by the not-yet-measured content height.
 */
function setScrollTop(view: EditorView, scrollTop: number): void {
  view.scrollDOM.scrollTop = scrollTop;
  view.requestMeasure({
    read: () => null,
    write: () => {
      view.scrollDOM.scrollTop = scrollTop;
    },
  });
}

// ── Manager ────────────────────────────────────────────────────────

/** A no-op safe-rename result — returned when the inline badge queries a slot
 *  whose handle has gone (unmounted mid-rename). It is `safe` with no edits, so
 *  the badge simply stays hidden rather than erroring. */
function emptyRenameResult(path: string): StructuralResult {
  return {
    ok: false,
    path,
    cross_file_edits: [],
    introduced_diagnostics: [],
    safe: true,
  };
}

export class DocumentSessions {
  private readonly project: ProjectSession;
  private readonly callbacks: DocumentCallbacks;
  private readonly extraExtensions: Extension[];
  private readonly options: DocumentSessionsOptions;
  private readonly slots = new Map<string, ViewSlot>();
  /** Fallback fragment ranges remembered from open requests (binder rows). */
  private readonly symbolHints = new Map<string, { start: number; end: number }>();
  private focusedSlotId: string | null = null;
  private readonly pendingReveals = new Map<string, PendingReveal>();
  private lastCompileDelivered: CompileResult | null = null;
  /** Inline form-glyph mode — applied to new views and switched live (Settings). */
  private formGlyph: FormGlyphMode = DEFAULT_FORM_GLYPH_MODE;
  /** Auto-open the Form on accepting a function completion (Settings; default off). */
  private autoOpen = false;

  constructor(
    project: ProjectSession,
    callbacks: DocumentCallbacks = {},
    extraExtensions: Extension[] = [],
    options: DocumentSessionsOptions = {},
  ) {
    this.project = project;
    this.callbacks = callbacks;
    this.extraExtensions = extraExtensions;
    this.options = options;
  }

  getProject(): ProjectSession {
    return this.project;
  }

  /** Switch the inline form-glyph mode live across all open editors (Settings). */
  setFormGlyph(mode: FormGlyphMode): void {
    this.formGlyph = mode;
    for (const slot of this.slots.values()) {
      if (slot.view !== null) setFormGlyphMode(slot.view, mode);
    }
  }

  /** Toggle completion-accept auto-open live across all open editors (Settings). */
  setAutoOpen(on: boolean): void {
    this.autoOpen = on;
    for (const slot of this.slots.values()) {
      if (slot.view !== null) setFormAutoOpen(slot.view, on);
    }
  }

  /** Re-render every open editor's breakpoint dots from the host's current
   *  set (W4/#3297) — call after `getBreakpoints`' backing data changes.
   *  The gutter re-reads only on this effect, never by polling. */
  refreshBreakpoints(): void {
    for (const slot of this.slots.values()) {
      if (slot.view !== null) refreshBreakpointsInView(slot.view);
    }
  }

  /** Re-render every open editor's execution highlight from the host's
   *  current position(s) (W6/#3299) — call whenever the runtime moved. */
  refreshExecutionHighlight(): void {
    for (const slot of this.slots.values()) {
      if (slot.view !== null) refreshExecutionHighlightInView(slot.view);
    }
  }

  /** The stashed HIR projection for an OPEN document, or `null` (unopened
   * path, or the projection hasn't landed yet). W11/#3304's choice-point
   * policy joins runtime ids against it — only open editors render the
   * highlight, so open-docs-only is the exact coverage needed. */
  getHirProjection(path: string): import("@brink/wasm-types").HirProjection | null {
    for (const slot of this.slots.values()) {
      if (slot.path === path) return slot.handle?.hirProjection() ?? null;
    }
    return null;
  }

  // ── Open hints ───────────────────────────────────────────────────

  /**
   * Remember a symbol target's range so the first mount can scope the
   * fragment even when the live outline disagrees with the (compile-time)
   * outline the opener used. Mounts prefer live symbol resolution; this is
   * the fallback.
   */
  noteTarget(target: DocTarget): void {
    if (target.kind === "symbol") {
      this.symbolHints.set(docKeyFor(target), { start: target.start, end: target.end });
    }
  }

  // ── Mounting ─────────────────────────────────────────────────────

  /**
   * Mount the view for (docKey, groupId) into `parent`. Returns a dispose
   * function for unmount: it snapshots the EditorState (background tabs keep
   * selection/undo) and closes the wasm handle.
   */
  mountView(docKey: string, groupId: string, parent: HTMLElement): () => void {
    const id = slotId(docKey, groupId);
    let slot = this.slots.get(id);
    if (slot === undefined) {
      const { path, symbol } = parseDocKey(docKey);
      slot = {
        docKey,
        groupId,
        path,
        symbol,
        handle: null,
        view: null,
        state: null,
        scrollTop: 0,
        extensions: null,
      };
      this.slots.set(id, slot);
    }
    if (slot.view !== null) {
      // Defensive: a second mount for the same slot replaces the first.
      this.unmountSlot(slot, { snapshot: false });
    }

    slot.handle = this.openHandle(slot);
    const content = slot.handle?.viewSource() ?? this.fileContent(slot.path);

    // Reuse the cached state when the authoritative content didn't change
    // underneath; otherwise rebuild (undo history resets — same contract as
    // the old invalidateFile).
    let state = slot.state;
    if (state === null || state.doc.toString() !== content) {
      state = EditorState.create({ doc: content, extensions: this.slotExtensions(slot) });
    }
    slot.state = state;

    // `cm.dispatch` times the WHOLE synchronous transaction cycle (state
    // update + every extension + CM's DOM sync) for the main editor view.
    // Added when a real-project capture showed ~113 ms of per-keystroke
    // handler time that no existing cm.* span accounted for — this span
    // splits "inside the editor update" from "outside it" (React dispatch,
    // other listeners, engine work). The meta carries the transaction count.
    const view = new EditorView({
      state,
      parent,
      dispatchTransactions: (trs, v) => {
        const end = perfSpan("cm.dispatch");
        const endState = perfSpan("cm.dispatch.state");
        // Materialize the new state first (runs every StateField update);
        // what remains in view.update is plugins + DOM sync.
        void trs[trs.length - 1]?.state;
        endState();
        const endView = perfSpan("cm.dispatch.view");
        try {
          v.update(trs);
        } finally {
          endView();
          end(trs.length);
        }
      },
    });
    slot.view = view;

    if (this.focusedSlotId === id) {
      this.applyFocusSideEffects(slot);
    }
    // Re-apply the scroll snapshotted at unmount (EditorState keeps the
    // selection across the background/remount cycle, but not scroll) unless a
    // pending reveal/restore (docKey-wide or targeting this slot) is about to
    // position the view itself.
    if (
      slot.scrollTop !== 0 &&
      !this.pendingReveals.has(docKey) &&
      !this.pendingReveals.has(slotId(docKey, slot.groupId))
    ) {
      setScrollTop(view, slot.scrollTop);
    }
    this.applyPendingReveal(slot);

    // #518 (follow-up to #494): deliverCompile refreshes only the views
    // mounted when the result lands — for a viewless slot the refresh is
    // dropped, not queued. A view mounting after that delivery would show
    // whatever its overlay field last held: the create()-time seed on a
    // fresh state, or — when the cached EditorState is reused above —
    // whatever was current at unmount, since create() never re-runs. A
    // passive load never compiles again, so nothing else would repaint it.
    // Self-serve the missed refresh here: the slot's handle was (re)opened
    // above, so the projection read is live at mount time. The trigger set
    // is {compile-deliver} ∪ {view-mount-after-a-deliver}.
    if (this.lastCompileDelivered !== null) {
      this.refreshOverlayPrepared(slot);
    }

    return () => {
      this.unmountSlot(slot, { snapshot: true });
    };
  }

  /**
   * Drop slots (cached states) whose (docKey, groupId) pair is no longer
   * open anywhere — called by the shell-store subscriber so closed tabs and
   * collapsed groups don't accumulate states (unbounded-growth guard).
   * Mounted slots are never pruned.
   */
  retainSlots(live: ReadonlySet<string>, liveDocKeys?: ReadonlySet<string>): void {
    for (const [id, slot] of [...this.slots]) {
      if (slot.view !== null) continue;
      if (!live.has(slotId(slot.docKey, slot.groupId))) {
        this.slots.delete(id);
      }
    }
    if (liveDocKeys !== undefined) {
      for (const key of [...this.symbolHints.keys()]) {
        if (!liveDocKeys.has(key)) this.symbolHints.delete(key);
      }
      // Undelivered reveals/restores for closed tabs must not linger — a
      // stale entry would fire (reveals steal focus) on a much-later remount.
      // Slot-targeted keys contain the slotId separator; docKey-wide ones
      // are plain docKeys.
      for (const key of [...this.pendingReveals.keys()]) {
        const alive = key.includes("\u0000") ? live.has(key) : liveDocKeys.has(key);
        if (!alive) this.pendingReveals.delete(key);
      }
    }
  }

  /** Slot-id helper for retainSlots callers. */
  static slotId(docKey: string, groupId: string): string {
    return slotId(docKey, groupId);
  }

  // ── Focus ────────────────────────────────────────────────────────

  /**
   * Declare the focused view (the focused group's active tab). Reports the
   * new view through onFocusedViewChange and refreshes cursor/line info.
   */
  setFocused(docKey: string | null, groupId: string | null): void {
    const id = docKey !== null && groupId !== null ? slotId(docKey, groupId) : null;
    if (this.focusedSlotId === id) return;
    this.focusedSlotId = id;
    const slot = id !== null ? this.slots.get(id) : undefined;
    if (slot !== undefined && slot.view !== null) {
      this.applyFocusSideEffects(slot);
    } else {
      this.callbacks.onFocusedViewChange?.(null);
    }
  }

  /** The focused group's active view, when mounted. */
  getFocusedView(): EditorView | null {
    if (this.focusedSlotId === null) return null;
    return this.slots.get(this.focusedSlotId)?.view ?? null;
  }

  // ── Focused-view operations ──────────────────────────────────────

  /**
   * Compile the project now and deliver the result through onCompileResult.
   * Views push their content on every change, so the session is current.
   * Rides the async session facade (W4 — the last sync compile caller):
   * on the worker road this is what keeps even the mount-time compile off
   * the main thread. A rejected compile (superseded/teardown) delivers
   * nothing — a newer compile follows.
   */
  triggerCompile(): void {
    void this.project.compileProjectAsync().then(
      (result) => this.deliverCompile(result),
      () => {},
    );
  }

  /** Convert the focused view's current line to the given element sigil. */
  convertLineToType(sigil: string): void {
    const view = this.getFocusedView();
    if (view) cmConvertLineToType(view, sigil);
  }

  /**
   * Insert text at the cursor in the focused view (replacing the selection
   * if there is one), leaving the cursor after the insertion. The StudioApi
   * facade's `insertText` (spec §8.2) lands here; a no-op when no editor
   * view is focused. The change mirrors to sibling views like any user edit.
   */
  insertAtCursor(text: string): void {
    const view = this.getFocusedView();
    if (view === null) return;
    const { from, to } = view.state.selection.main;
    view.dispatch({
      changes: { from, to, insert: text },
      selection: { anchor: from + text.length },
      scrollIntoView: true,
    });
    view.focus();
  }

  focus(): void {
    this.getFocusedView()?.focus();
  }

  /**
   * Push the focused view's current text to the wasm session immediately
   * (bypassing the editor's compile/flush debounce) and report the change
   * (`file.save`). Returns the flushed file path, or null when no editor
   * view is focused.
   */
  flushFocused(): string | null {
    if (this.focusedSlotId === null) return null;
    const slot = this.slots.get(this.focusedSlotId);
    if (slot === undefined || slot.view === null) return null;
    this.flushSlot(slot);
    return slot.path;
  }

  /**
   * Push every mounted view's current text to the wasm session immediately
   * (`file.saveAll`, unmount). Returns the flushed file paths (deduped —
   * split views of one file flush once-per-view but report one path).
   */
  flushAll(): string[] {
    const paths = new Set<string>();
    for (const slot of this.slots.values()) {
      if (slot.view === null) continue;
      this.flushSlot(slot);
      paths.add(slot.path);
    }
    return [...paths].sort();
  }

  getContent(): string {
    return this.getFocusedView()?.state.doc.toString() ?? "";
  }

  // ── Reveal ───────────────────────────────────────────────────────

  /**
   * Select + scroll to `offset` (view-relative) in the view of `docKey`,
   * once it is mounted — the shell store focuses/opens the tab first, this
   * applies as soon as React commits the mount (or immediately when the
   * view already exists).
   */
  revealAt(docKey: string, offset: number): void {
    this.pendingReveals.set(docKey, { kind: "reveal", offset });
    this.applyToMountedView(docKey);
  }

  // ── View state (#347) ────────────────────────────────────────────

  /**
   * Read a view slot's current cursor + scroll — the snapshot seam for
   * editor-state persistence. Works for every open tab: a mounted view reads
   * live, a backgrounded tab reads its cached EditorState (selection) and the
   * scroll snapshotted at unmount. With `groupId` this addresses one slot
   * exactly; without it, prefers the focused slot for `docKey`, then any
   * mounted one, then any cached one. Returns null when no slot (or cached
   * state) exists for the key.
   */
  viewState(docKey: string, groupId?: string): ViewStateSnapshot | null {
    const slot = this.findViewStateSlot(docKey, groupId);
    if (slot === undefined) return null;
    if (slot.view !== null) {
      const { anchor, head } = slot.view.state.selection.main;
      return { anchor, head, scrollTop: slot.view.scrollDOM.scrollTop };
    }
    if (slot.state !== null) {
      const { anchor, head } = slot.state.selection.main;
      return { anchor, head, scrollTop: slot.scrollTop };
    }
    return null;
  }

  /**
   * Re-apply a persisted cursor + scroll to the view of `docKey`, once it is
   * mounted (or immediately when it already is) — the restore seam matching
   * `viewState`. Rides the pending-reveal mechanism, but unlike `revealAt` it
   * restores the full selection (`anchor`/`head`, clamped to the document)
   * plus the pixel scroll offset, and does not focus the view — hosts replay
   * every open tab on reload, not just the active one.
   *
   * With `groupId` the restore targets one (docKey, groupId) slot exactly —
   * matching `viewState`'s addressing, so a split view (same doc open in two
   * groups) restores each pane independently. Without it, whichever view of
   * `docKey` mounts first consumes the entry.
   */
  restoreViewState(docKey: string, state: ViewStateSnapshot, groupId?: string): void {
    const key = groupId !== undefined ? slotId(docKey, groupId) : docKey;
    this.pendingReveals.set(key, { kind: "restore", ...state });
    this.applyToMountedView(docKey, groupId);
  }

  /** Apply the pending entry for `docKey` now if a matching view is mounted. */
  private applyToMountedView(docKey: string, groupId?: string): void {
    for (const slot of this.slots.values()) {
      if (slot.docKey !== docKey || slot.view === null) continue;
      if (groupId !== undefined && slot.groupId !== groupId) continue;
      this.applyPendingReveal(slot);
      return;
    }
  }

  private findViewStateSlot(docKey: string, groupId?: string): ViewSlot | undefined {
    if (groupId !== undefined) return this.slots.get(slotId(docKey, groupId));
    const focused =
      this.focusedSlotId !== null ? this.slots.get(this.focusedSlotId) : undefined;
    if (focused !== undefined && focused.docKey === docKey) return focused;
    let cached: ViewSlot | undefined;
    for (const slot of this.slots.values()) {
      if (slot.docKey !== docKey) continue;
      if (slot.view !== null) return slot;
      cached ??= slot;
    }
    return cached;
  }

  /**
   * Start the inline rename (#323/#324) at a whole-file UTF-16 `offset` in a
   * mounted view of `path` — the editor context-menu "Rename…" route. Prefers
   * the focused view, then any file view, then any fragment view (translating
   * the whole-file offset into that view's coords). A no-op when no view of the
   * path is mounted; the modal `SymbolRenamePrompt` covers the Binder/graph.
   */
  async startInlineRenameAt(path: string, offset: number): Promise<boolean> {
    const focused =
      this.focusedSlotId !== null ? this.slots.get(this.focusedSlotId) : undefined;
    const candidates: ViewSlot[] = [];
    if (focused !== undefined && focused.path === path && focused.view !== null) {
      candidates.push(focused);
    }
    for (const slot of this.slots.values()) {
      if (slot === focused || slot.path !== path || slot.view === null) continue;
      // File views first (whole-file coords), then fragments.
      if (slot.symbol === null) candidates.unshift(slot);
      else candidates.push(slot);
    }
    for (const slot of candidates) {
      if (slot.view === null) continue;
      const base = slot.handle?.fragmentRange()?.start ?? 0;
      const viewOffset = offset - base;
      if (viewOffset < 0 || viewOffset > slot.view.state.doc.length) continue;
      // #3110: the target resolves on the worker road — await so a
      // non-renameable offset can fall through to the next candidate
      // view (and ultimately the caller's modal fallback).
      if (await startInlineRename(slot.view, viewOffset)) return true;
    }
    return false;
  }

  // ── Invalidation (binder structural ops, undo) ───────────────────

  /**
   * The file changed underneath its views (structural move / undo): refresh
   * every mounted view of `path` from the session. File views reload their
   * content; symbol views re-resolve their range by name, degrading to the
   * full file when the symbol vanished (the old invalidateFile contract).
   * Backgrounded states refresh on their next mount via the content check.
   */
  invalidateFile(path: string): void {
    for (const slot of this.slots.values()) {
      if (slot.path !== path) continue;
      if (slot.view === null) {
        // Cached-only slot: drop the state, the next mount rebuilds.
        slot.state = null;
        continue;
      }
      this.refreshSlotFromFile(slot);
    }
  }

  /**
   * A file moved from `oldPath` to `newPath` (rename/move): re-key every view
   * slot for that file (and its `oldPath::symbol` fragment slots) in place —
   * re-deriving the slot map key, `path`, `docKey`, focused-slot id, and any
   * symbol hint — then reopen mounted handles against the new session path
   * (the old DocId is invalid once the session re-keys). The shell re-keys the
   * tabs themselves (editor-groups `updateTabRef`); this keeps the per-view
   * machinery aligned so an open editor survives the rename in place.
   */
  renameDocPath(oldPath: string, newPath: string): void {
    if (oldPath === newPath) return;
    const migrated: ViewSlot[] = [];
    // A file slot has path === oldPath; its fragment (symbol) slots do too.
    for (const [id, slot] of [...this.slots]) {
      if (slot.path !== oldPath) continue;
      const newDocKey = slot.symbol === null ? newPath : `${newPath}::${slot.symbol}`;
      const oldDocKey = slot.docKey;
      this.slots.delete(id);
      slot.path = newPath;
      slot.docKey = newDocKey;
      const newId = slotId(newDocKey, slot.groupId);
      this.slots.set(newId, slot);
      const hint = this.symbolHints.get(oldDocKey);
      if (hint !== undefined) {
        this.symbolHints.delete(oldDocKey);
        this.symbolHints.set(newDocKey, hint);
      }
      if (this.focusedSlotId === id) this.focusedSlotId = newId;
      migrated.push(slot);
    }
    // The session has already re-keyed (old removed, new added), so reopen each
    // mounted handle against newPath; cached-only slots rebuild on next mount.
    for (const slot of migrated) {
      if (slot.view !== null) this.refreshSlotFromFile(slot);
      else slot.state = null;
    }
  }

  /**
   * An external change landed in the session (#320's clean path — a real
   * fs watcher's update, or `pushExternalChange` in tests): re-sync every
   * mounted view of `path` from the session, exactly like the mount-time
   * refresh. The replace is sync-annotated, so it never flows back through
   * the user-edit flush as an edit of its own.
   *
   * This is the wire `onExternalFileChange` existed for and never had:
   * without it, an open view of an externally-updated file keeps its stale
   * text and the next view flush silently writes that stale text back over
   * the session — the reverse of the clobber #320 fixed (found live by the
   * brink-desktop D2 watcher, the clean path's first real consumer:
   * session updated, Player compiled the update, the visible editor and
   * every later save reverted it).
   */
  refreshExternal(path: string): void {
    for (const slot of this.slots.values()) {
      if (slot.path !== path) continue;
      if (slot.view !== null) this.refreshSlotFromFile(slot);
      else slot.state = null; // cached-only slot rebuilds from the session on next mount
    }
  }

  /**
   * A file open in a view was deleted externally (issue #2371, "External
   * deletion of an open file: keep the view, mark orphaned"): the opposite
   * of `refreshExternal` — the kept buffer is never touched here, no view is
   * re-synced or closed. `ProjectSession`'s external-change handler already
   * dropped the file from the wasm session and flagged the path orphaned
   * (`FileChangeHub.applyExternal(path, null)`); this repairs the one thing
   * that skip left broken: with the file gone from the session, every
   * wasm-backed query against a still-open handle (hover, compile,
   * semantic tokens) degrades until something re-adds the file, and a save
   * with an *unedited* buffer would silently write nothing (the mounted
   * handle's own no-op-push cache — `DocHandle.pushSource`'s `lastPushed`
   * guard, comparing against text that hasn't changed since deletion —
   * would skip the very push meant to recreate it).
   *
   * Re-adds the file to the session from the *kept* full-file buffer's
   * current text (through `ProjectSession.recreateOrphaned`) so the buffer
   * stops being a session-level ghost: IDE queries work again, and the path
   * — with no baseline (`applyExternal(path, null)` dropped it) — reads
   * dirty by the existing `FileChangeHub` rule immediately, not only after
   * the user's next keystroke. The provider itself is not touched yet
   * (`recreateOrphaned`'s doc explains why) — a later save
   * (`file.save`/`file.saveAll`) is what actually recreates the file on
   * disk, through the normal save path.
   *
   * Only a full-file view's text is trustworthy here: a symbol (fragment)
   * view holds just a slice of the file, and the file's *other* content is
   * gone from the session along with everything else `removeFile` dropped
   * — there is nothing in the TS layer to reconstruct it from. When only
   * fragment views of `path` are open, this leaves the session without the
   * file (queries against those views keep degrading, and a save is a
   * no-op — no worse than before this method existed); a full-file view
   * anywhere among the slots is enough to recover.
   */
  markOrphaned(path: string): void {
    let content: string | null = null;
    for (const slot of this.slots.values()) {
      if (slot.path !== path || slot.symbol !== null) continue;
      if (slot.view !== null) {
        content = slot.view.state.doc.toString();
        break;
      }
      if (slot.state !== null) {
        content = slot.state.doc.toString();
        // Keep looking — a mounted full-file view (if any) is preferred.
      }
    }
    if (content !== null) this.project.recreateOrphaned(path, content);
  }

  /**
   * A knot/stitch was renamed in place (#305): re-key the open fragment slot
   * for `${path}::${oldName}` to `${path}::${newName}` — slot map key, docKey,
   * `slot.symbol`, focused-slot id — dropping the stale symbol hint so the view
   * re-resolves by the new name. The shell re-keys the tab itself; this keeps
   * the per-view machinery aligned so an open symbol view survives the rename.
   * (Symbol tabs are keyed by the bare name, so only the renamed symbol's own
   * tab is affected; child stitches keep their independent `path::stitch` keys.)
   */
  renameSymbolDoc(path: string, oldName: string, newName: string): void {
    if (oldName === newName) return;
    const oldDocKey = `${path}::${oldName}`;
    const newDocKey = `${path}::${newName}`;
    for (const [id, slot] of [...this.slots]) {
      if (slot.docKey !== oldDocKey) continue;
      this.slots.delete(id);
      slot.symbol = newName;
      slot.docKey = newDocKey;
      const newId = slotId(newDocKey, slot.groupId);
      this.slots.set(newId, slot);
      // Drop the stale range hint; the refresh re-resolves by the new name.
      this.symbolHints.delete(oldDocKey);
      if (this.focusedSlotId === id) this.focusedSlotId = newId;
      if (slot.view !== null) this.refreshSlotFromFile(slot);
      else slot.state = null;
    }
  }

  // ── Teardown ─────────────────────────────────────────────────────

  dispose(): void {
    for (const slot of this.slots.values()) {
      if (slot.view !== null) this.unmountSlot(slot, { snapshot: false });
    }
    this.slots.clear();
    this.symbolHints.clear();
    this.pendingReveals.clear();
  }

  // ── Private: save flush ──────────────────────────────────────────

  /** Push one mounted slot's text through its handle (no-op pushes are
   *  collapsed by DocHandle) and report through the project's notify seam. */
  private flushSlot(slot: ViewSlot): void {
    if (slot.view === null) return;
    slot.handle?.pushSource(slot.view.state.doc.toString());
    this.project.notifyFileChanged(slot.path);
  }

  // ── Private: mount plumbing ──────────────────────────────────────

  private unmountSlot(slot: ViewSlot, opts: { snapshot: boolean }): void {
    if (slot.view !== null) {
      if (opts.snapshot) {
        slot.state = slot.view.state;
        slot.scrollTop = slot.view.scrollDOM.scrollTop;
      }
      slot.view.destroy();
      slot.view = null;
    }
    slot.handle?.close();
    slot.handle = null;
    if (this.focusedSlotId === slotId(slot.docKey, slot.groupId)) {
      this.callbacks.onFocusedViewChange?.(null);
    }
  }

  private openHandle(slot: ViewSlot, opts?: { allowHint?: boolean }): DocHandle | null {
    const session = this.project.getSession();
    if (slot.symbol === null) {
      const id = session.openDocument(slot.path);
      if (id === null) return null;
      const handle = new DocHandle(session, id, slot.path, false);
      this.attachClassifier(handle, slot.path);
      return handle;
    }
    const range = this.resolveSymbolRange(
      slot.path,
      slot.symbol,
      slot.docKey,
      opts?.allowHint ?? true,
    );
    if (range === null) {
      // Symbol unknown: degrade to a full-file handle (old invalidateFile
      // behavior — the tab keeps its label but shows the whole file).
      const id = session.openDocument(slot.path);
      return id === null ? null : new DocHandle(session, id, slot.path, false);
    }
    const id = session.openFragment(slot.path, range.start, range.end);
    if (id === null) return null;
    const handle = new DocHandle(session, id, slot.path, true);
    handle.setFragmentRange(range.start, range.end);
    return handle;
  }

  /**
   * Attach the main-thread classifier mirror (W3 of
   * docs/editor-worker-spec.md §4) to a full-file handle. No-op when the
   * wasm build lacks `ClassifierSession` (older builds, test mocks) or
   * the file has no session content — the handle then keeps every road
   * on the project session. Fragment handles never get one (their views
   * are small; the session road serves them).
   */
  private attachClassifier(handle: DocHandle, path: string): void {
    const classifier = new ClassifierSessionHandle();
    if (!classifier.available) return;
    const content = this.project.getSession().getFileSource(path);
    if (content === null || !classifier.open(path, content)) {
      classifier.free();
      return;
    }
    handle.attachClassifier(new ClassifierMirror(classifier));
  }

  /**
   * Resolve a symbol's current body range from the live session. The hint
   * remembered from the open request is a first-mount fallback only (the
   * opener's outline can be fresher than a not-yet-analyzed session) —
   * refresh paths must not use it: there a missing symbol means it is gone,
   * and the view degrades to the full file.
   */
  private resolveSymbolRange(
    path: string,
    symbol: string,
    docKey: string,
    allowHint: boolean,
  ): { start: number; end: number } | null {
    // #3110: the synchronous answer is the HINT (the opener's worker-fed
    // outline, or a previous async resolution below) — never a main-thread
    // symbol-index pull. The worker verifies asynchronously: a landing
    // that finds the symbol at a different range (or finds it where we
    // degraded to the full file) updates the hint and re-resolves the
    // slot. Refresh paths (allowHint false) keep the old contract —
    // degrade now, upgrade when the worker answers.
    this.verifySymbolRange(path, symbol, docKey);
    return allowHint ? (this.symbolHints.get(docKey) ?? null) : null;
  }

  /** The async half of {@link resolveSymbolRange}: ask the worker for the
   *  symbol's current range; on landing, update the hint and re-open the
   *  slot's handle if the mounted range disagrees. */
  private verifySymbolRange(path: string, symbol: string, docKey: string): void {
    void this.project
      .docClient()
      .query<DocumentSymbol[]>("getFileSymbols", [path], { priority: "interactive" })
      .promise.then((r) => {
        const found = findSymbolByName(r.value, symbol);
        if (found === null) return; // symbol gone — the degrade stands
        const range = { start: found.full_start, end: found.full_end };
        const previous = this.symbolHints.get(docKey);
        this.symbolHints.set(docKey, range);
        if (previous?.start === range.start && previous?.end === range.end) return;
        const slot = [...this.slots.values()].find((sl) => sl.docKey === docKey);
        if (slot === undefined || slot.view === null) return;
        const current = slot.handle?.fragmentRange() ?? null;
        if (current?.start === range.start && current?.end === range.end) return;
        // Re-open against the fresh hint (allowHint stays true here — the
        // hint was just written by this landing, so there is no loop).
        this.refreshSlotFromFile(slot, { allowHint: true });
      })
      .catch(() => {});
  }

  private fileContent(path: string): string {
    return this.project.getSession().getFileSource(path) ?? "";
  }

  private slotExtensions(slot: ViewSlot): Extension[] {
    if (slot.extensions === null) {
      slot.extensions = [
        brinkStudio(this.slotOptions(slot)),
        brinkBasicSetup,
        keymap.of(defaultKeymap),
        EditorView.lineWrapping,
        // #3119: gutters leave the scroller's flex/sticky flow, which
        // costs WebKit ~5x on every editor layout. Self-gating on the
        // wrapping above.
        detachedGutters(),
        this.slotListener(slot),
        EditorView.domEventHandlers({
          focus: () => {
            this.callbacks.onViewFocused?.(slot.docKey, slot.groupId);
            return false;
          },
        }),
        ...this.extraExtensions,
        // A mounted stdlib file's view is genuinely read-only (issue
        // #2306/#2343): `updateDocument`/`applyEdit` already refuse the
        // write at the wasm/session layer, but without this a keystroke
        // still lands in the CM6 doc and silently reverts on the next
        // wasm round-trip (`DocHandle.pushSource` drops a refused push) —
        // the user sees their typing vanish rather than being told the
        // file can't be edited. `EditorView.editable.of(false)` is what
        // actually stops the keystroke (-> `contenteditable="false"` on the
        // DOM); `EditorState.readOnly` is advisory — CM6 core doesn't
        // consult it for typing, but `@codemirror/commands` and
        // search/replace do, so it still guards those paths. Matches
        // `conflict-view.ts`'s "ON DISK" read-only pane.
        ...(this.project.isReadOnly(slot.path)
          ? [EditorState.readOnly.of(true), EditorView.editable.of(false)]
          : []),
      ];
    }
    return slot.extensions;
  }

  /**
   * Per-slot studio options: every wasm query routes through the slot's
   * *current* handle (per-view DocId). Closures capture the slot, not the
   * handle, so states cached across unmount/remount stay valid.
   */
  private slotOptions(slot: ViewSlot): BrinkStudioOptions {
    const project = this.project;
    return {
      theme: this.options.theme,
      indentGuides: this.options.indentGuides,
      // `[project] indent` wins over the embedder's option (#3149, ruled
      // 2026-08-27: there is ONE place the width comes from, and it is
      // brink.toml). Read live rather than captured at construction —
      // config discovery lands after the session is built, so a captured
      // value would be the pre-config one forever.
      //
      // Known limit: a view already mounted keeps the width it was built
      // with until it remounts, since `indentUnit` is not in a compartment.
      // Editing `[project] indent` therefore takes effect on the next
      // mount (or reload), not mid-keystroke.
      indent: this.project.getConfiguredIndent() ?? this.options.indent,
      dialect: this.options.dialect,
      compile: (source) => {
        slot.handle?.pushSource(source);
        project.notifyFileChanged(slot.path);
        // Kick off async INCLUDE resolution — next compile picks up new files.
        // Rejects if destroy() lands mid-await (#2802's assertLive, this PR) —
        // this call site has no result to await and nothing to react to a
        // rejection with, so swallow it rather than surface an unhandled
        // promise rejection at unmount.
        void project.refreshIncludes().catch(() => {});
        // W2a (docs/editor-worker-spec.md): the compile rides the async
        // session facade — the diagnostics extension awaits and lands it
        // under its own staleness guards.
        return project.compileProjectAsync();
      },
      onCompile: (result) => this.deliverCompile(result),
      // No re-push (#14): the source for this transaction was already pushed by
      // the elementTypeField StateField (CM runs StateFields before decoration
      // facets), so every per-keystroke query just reads by DocId.
      getSemanticTokens: (_source) => slot.handle?.semanticTokens() ?? [],
      getSemanticTokensFast: (_source) => slot.handle?.semanticTokens(true) ?? [],
      getHirProjection: () =>
        slot.handle?.hirProjection() ?? { spans: [], lines: [] },
      // Prose checking (#3209). The dictionary comes from the SESSION, not
      // the host: the project's own knot and cue names are what stop every
      // invented character name reporting as a misspelling (#3210), and the
      // session is the only thing that knows them.
      // `[prose] enable = false` unregisters the checker rather than
      // filtering its output: not doing the work is the point, and a
      // disabled checker that still loads 6.5 MB and runs would be a toggle
      // that only hides its results.
      getProseChecker: () =>
        this.project.isProseEnabled() ? (this.options.proseChecker ?? null) : null,
      getProseDictionary: () => this.project.getProseDictionary(),
      getProseDialect: () =>
        this.options.proseDialect?.() ?? this.project.getProseDialect(),
      // Passed straight through, INCLUDING when absent: `proseExtension`
      // offers the action only when this is defined, so forwarding an
      // always-present wrapper would put a control in the tooltip that does
      // nothing for an embedder that cannot store a word.
      onAddToDictionary: this.options.onAddToDictionary,
      onProseLints: (lints) => this.callbacks.onProseLints?.(slot.path, lints),
      getTokenTypeNames,
      handleSlot: slot,
      getActiveFile: () => slot.path,
      onNavigateToFile: (location) => this.callbacks.onNavigateToFile?.(location),
      // Only expose the editor affordance when a handler is wired, so the
      // hover ▶ / right-click menu never appears as a dead control.
      onPlayFrom: this.callbacks.onPlayFrom
        ? (inkPath, label) => this.callbacks.onPlayFrom?.(inkPath, label)
        : undefined,
      // Breakpoints (W4/#3297): per-slot closures inject the file path,
      // same shape as onSymbolContextMenu below. Only exposed when both
      // halves are wired, so the gutter never shows a dead affordance.
      getBreakpoints:
        this.callbacks.getBreakpoints && this.callbacks.onToggleBreakpoint
          ? () => this.callbacks.getBreakpoints?.(slot.path) ?? []
          : undefined,
      onToggleBreakpoint:
        this.callbacks.getBreakpoints && this.callbacks.onToggleBreakpoint
          ? (line) => this.callbacks.onToggleBreakpoint?.(slot.path, line)
          : undefined,
      onBreakpointsMoved: this.callbacks.onBreakpointsMoved
        ? (moves) => this.callbacks.onBreakpointsMoved?.(slot.path, moves)
        : undefined,
      getExecutionHighlights: this.callbacks.getExecutionHighlights
        ? () => this.callbacks.getExecutionHighlights?.(slot.path) ?? []
        : undefined,
      onRevealInstructions: this.callbacks.onRevealInstructions
        ? (line) => this.callbacks.onRevealInstructions?.(slot.path, line)
        : undefined,
      getRuntimeValueNote: this.callbacks.getRuntimeValueNote
        ? (name) => this.callbacks.getRuntimeValueNote?.(name) ?? null
        : undefined,
      // Inject the focused view's file path so the host can resolve the symbol.
      onSymbolContextMenu: this.callbacks.onSymbolContextMenu
        ? (info, x, y) =>
            this.callbacks.onSymbolContextMenu?.({ path: slot.path, ...info }, x, y)
        : undefined,
      onTextContextMenu: this.callbacks.onTextContextMenu
        ? (request) => this.callbacks.onTextContextMenu?.(request)
        : undefined,
      onShowReferences: this.callbacks.onShowReferences
        ? (symbol, locations, declaration) =>
            this.callbacks.onShowReferences?.(symbol, locations, declaration)
        : undefined,
      // W2c: interactive queries ride the async facade (interactive
      // priority — never coalesced or dropped); the sync fallback array
      // covers a slot whose handle is not live yet.
      getCompletions: (_source, offset) =>
        this.interactiveQuery<CompletionItem[]>(slot, "getCompletionsDoc", [offset]) ?? [],
      // Auto-import on completion-accept (#312 F): ensure the current file
      // INCLUDEs an out-of-scope symbol's source file. The wasm op reports
      // reachability and, when the target is not yet reachable, the whole-file
      // UTF-16 INCLUDE-insertion edit. For a whole-file view those coords match
      // the CM document, so the edit is returned for the accept handler to
      // dispatch in-view (visible immediately, flowing through the normal
      // edit→recompile pipeline). For a fragment view the INCLUDE lives above
      // the fragment and cannot be dispatched into it, so it is applied to the
      // whole-file source AND the open fragment view range is rebased in one
      // wasm op (`autoImportApply`). A raw whole-file write would leave the
      // fragment handle's stored range at pre-shift offsets, so the very next
      // fragment push would splice at a stale range and clobber the INCLUDE
      // line + surrounding content (#312 F regression).
      autoImport: (target) => {
        const handle = slot.handle;
        if (!handle) return { ok: false, already_reachable: false };
        const isFragment = handle.fragmentRange() != null;
        if (isFragment) {
          const result = handle.autoImportApply(target);
          if (result.ok && !result.already_reachable) {
            // The INCLUDE mutated the whole file: notify the file-change hub
            // (sibling views + host egress) and refresh INCLUDE resolution so
            // the new edge is picked up on the next compile.
            this.project.notifyFileChanged(slot.path);
            void this.project.refreshIncludes();
          }
          // `autoImportApply` already applied the INCLUDE and rebased the view;
          // it returns `edit: null`, so the accept handler only inserts the
          // symbol text into the fragment view.
          return result;
        }
        return handle.autoImport(target);
      },
      getHover: (_source, offset) =>
        this.interactiveQuery<HoverInfo | null>(slot, "getHoverDoc", [offset]) ?? null,
      // #3110: the one-shot family rides the worker road too.
      gotoDefinition: (_source, offset) =>
        this.interactiveQuery<Location | null>(slot, "gotoDefinitionDoc", [offset]) ?? null,
      findReferences: (_source, offset) =>
        this.interactiveQuery<Location[]>(slot, "findReferencesDoc", [offset]) ?? [],
      prepareRename: (_source, offset) =>
        this.interactiveQuery<Location | null>(slot, "prepareRenameDoc", [offset]) ?? null,
      // Inline rename (#323/#324): the badge's live breakage query. Fold any
      // fragment-view origin into a whole-file UTF-16 offset, then compute the
      // safe-rename result (side-effect-free). Only wired alongside a commit
      // handler, so the F2 inline widget never appears as a dead control.
      renameSymbolAt: this.callbacks.onRenameCommit
        ? (offset, newName) => {
            if (slot.handle === null) return emptyRenameResult(slot.path);
            const base = slot.handle.fragmentRange()?.start ?? 0;
            // #3110: the safe-rename compute (side-effect-free) rides the
            // worker road at interactive priority.
            return this.project.structuralQuery<StructuralResult>("renameSymbolAt", [
              slot.path,
              base + offset,
              newName,
            ]);
          }
        : undefined,
      // Commit a safe (or forced) inline rename through the host: apply the
      // already-computed cross-file edits and re-key any open symbol tab.
      commitRename: this.callbacks.onRenameCommit
        ? (result, newName, currentName) =>
            this.callbacks.onRenameCommit?.({
              path: slot.path,
              newName,
              currentName,
              result,
            })
        : undefined,
      onRenameBreakage: this.callbacks.onRenameBreakage,
      getCodeActions: (_source, offset) =>
        this.interactiveQuery<CodeAction[]>(slot, "getCodeActionsDoc", [offset]) ?? [],
      // Code-actions apply seam (#321 studio side): resolve the chosen action's
      // StructuralResult through the slot's handle (doc-relative offset — the
      // doc variant folds any fragment origin), then apply through the host.
      // Only wired alongside an apply callback so the menu never resolves into a
      // dead seam.
      applyCodeAction: this.callbacks.onApplyStructural
        ? (action: CodeAction) => {
            const handle = slot.handle;
            if (!handle) return;
            const offset = slot.view?.state.selection.main.head ?? 0;
            const result = handle.resolveCodeAction(action.data, offset);
            this.callbacks.onApplyStructural?.({
              path: slot.path,
              description: action.title,
              result,
            });
          }
        : undefined,
      // Extract (#315 H): compute is side-effect-free — fold any fragment-view
      // origin into whole-file UTF-16 offsets, then call the matching wasm op.
      // Apply routes the (safe or forced) result through the host apply seam.
      computeExtract: this.callbacks.onApplyStructural
        ? (kind, start, end, name) => {
            const handle = slot.handle;
            if (!handle) return null;
            const base = handle.fragmentRange()?.start ?? 0;
            const result =
              kind === "knot"
                ? handle.extractToKnot(base + start, base + end, name)
                : handle.extractToFunction(base + start, base + end, name);
            // An op error (name collision, header crossing, illegal function
            // body) comes back `ok:false, safe:true` — treat it as "no result"
            // so the prompt cancels instead of committing an empty edit. v1
            // surfaces the reason via the console; the breakage gate handles the
            // scope-breaking (unsafe-but-ok) case with the inline report.
            if (!result.ok) return null;
            return result;
          }
        : undefined,
      applyExtract: this.callbacks.onApplyStructural
        ? (kind: ExtractKind, result, name) =>
            this.callbacks.onApplyStructural?.({
              path: slot.path,
              description: `Extract to ${kind === "knot" ? "knot" : "function"} ${name}`,
              result,
            })
        : undefined,
      getInlayHints: (_source, start, end) =>
        slot.handle?.inlayHints(start, end) ?? [],
      // No re-push (#14) — the elementTypeField StateField already pushed this
      // transaction's source before decorations/auto-open run, so the call
      // right after a completion-accept (#229) still sees the new `()`.
      getArgumentWidgets: (_source, start, end) => slot.handle?.argumentWidgets(start, end) ?? [],
      argumentFormGlyph: this.formGlyph,
      argumentAutoOpen: this.autoOpen,
      getSignatureHelp: (_source, offset) =>
        this.interactiveQuery<SignatureInfo | null>(slot, "getSignatureHelpDoc", [offset]) ??
        null,
      getFoldingRanges: () => slot.handle?.foldingRanges() ?? [],
      // ── W2b (docs/editor-worker-spec.md): deferred-refresh warm-ups ──
      // Each prepare rides the async session facade with a per-surface
      // coalesce key, so a burst of quiet-fires across sibling views
      // collapses to one execution; the refresh effect then rebuilds
      // synchronously against the warmed memo. Refined tokens instead warm
      // the DocHandle slice cache (the main-side materialized view) — that
      // road's fetches go through the client when DocHandle itself
      // migrates (W4 prep).
      prepareRefined: () => {
        const handle = slot.handle;
        if (!handle) return undefined;
        // W5c: fetch the replica's refined slices (manifest + changed
        // segments only) into the worker plane; the sync rebuild then
        // assembles from it — no main-thread analysis on this road.
        return handle.refreshRefined(
          this.project.docClient(),
          this.project.getSession().configEpoch?.() ?? 0,
        );
      },
      prepareProjection: () => this.prepareQuery(slot, "getHirSpansDoc", "overlay", []),
      prepareHints: (start, end) =>
        this.prepareQuery(slot, "getInlayHintsDoc", "hints", [start, end]),
      prepareWidgets: (start, end) =>
        this.prepareQuery(slot, "getArgumentWidgetsDoc", "widgets", [start, end]),
      prepareFoldRanges: () => this.prepareQuery(slot, "getFoldingRangesDoc", "folds", []),
    };
  }

  /** One interactive query through the async facade (W2c): interactive
   *  priority — runs after queued mutations but before background pulls,
   *  never coalesced or dropped. `undefined` (no live handle) tells the
   *  caller to use its empty sync fallback. */
  private interactiveQuery<T>(
    slot: ViewSlot,
    method: string,
    args: readonly unknown[],
  ): Promise<T> | undefined {
    const id = slot.handle?.id;
    if (id === undefined) return undefined;
    return this.project
      .docClient()
      .query<T>(method, [id, ...args], { priority: "interactive", doc: id })
      .promise.then((r) => r.value);
  }

  /** One deferred-refresh warm-up query (W2b): background priority, a
   *  per-surface-per-doc coalesce key, the doc id as the leading arg.
   *  `undefined` (no live handle) tells the caller to skip the warm-up
   *  and dispatch directly. */
  private prepareQuery(
    slot: ViewSlot,
    method: string,
    surface: "overlay" | "hints" | "widgets" | "folds",
    args: readonly unknown[],
  ): Promise<unknown> | undefined {
    const id = slot.handle?.id;
    if (id === undefined) return undefined;
    return this.project
      .docClient()
      .query(method, [id, ...args], {
        priority: "background",
        doc: id,
        coalesceKey: `${surface}:${id}`,
      })
      .promise.then((r) => {
        // W5c: the warm-up's RESULT is the point — stash it so the
        // field's synchronous rebuild reads worker-fed data instead of
        // pulling analysis on the main thread.
        const handle = slot.handle;
        if (handle === null || handle.id !== id) return r;
        switch (surface) {
          case "overlay":
            handle.stashProjection(r.value as import("@brink/wasm-types").HirProjection);
            break;
          case "hints":
            handle.stashHints(r.value as import("@brink/wasm-types").InlayHint[]);
            break;
          case "widgets":
            handle.stashWidgets(r.value as import("@brink/wasm-types").CallWidgetSite[]);
            break;
          case "folds":
            handle.stashFolds(r.value as import("@brink/wasm-types").FoldRange[]);
            break;
        }
        return r;
      });
  }

  /**
   * The slot's update listener: cursor/line-info reporting for the focused
   * view, auto-pin signalling, and the cross-view mirror for user edits.
   */
  private slotListener(slot: ViewSlot): Extension {
    return EditorView.updateListener.of((update) => {
      const mirrored = update.transactions.some(
        (tr) => tr.annotation(syncAnnotation) === true,
      );

      if (update.docChanged && !mirrored) {
        this.callbacks.onDocEdited?.(slot.docKey, slot.groupId);
        perfTime("cm.slotListener.mirrorEdit", () =>
          this.mirrorEdit(slot, update.changes, docString(update.state)),
        );
      }

      if (
        (update.docChanged || update.selectionSet) &&
        this.focusedSlotId === slotId(slot.docKey, slot.groupId)
      ) {
        perfTime("cm.slotListener.reportCursor", () => this.reportCursor(update.view));
      }
    });
  }

  // ── Private: mirroring ───────────────────────────────────────────

  /**
   * Forward a user edit to sibling views (CM6 sync-dispatch) and mirror
   * across fragment⇄file views of the same path via the wasm change spec.
   */
  private mirrorEdit(source: ViewSlot, changes: ChangeSet, newSource: string): void {
    // Same-document siblings: identical docs, so the ChangeSet applies
    // verbatim. The annotation stops the sibling's own mirror from echoing.
    for (const sibling of this.slots.values()) {
      if (sibling === source || sibling.docKey !== source.docKey) continue;
      if (sibling.view === null) continue;
      // Fragment duplicates: rebase the sibling's handle to the source's
      // freshly-spliced range *before* dispatch, so the sibling's own push
      // (elementTypeField) compares equal and does not re-splice with a
      // stale range.
      if (
        sibling.handle !== null &&
        sibling.handle.isFragment &&
        source.handle !== null &&
        source.handle.isFragment
      ) {
        const range = source.handle.fragmentRange();
        if (range !== null) {
          sibling.handle = this.reopenFragment(sibling, range);
        }
      }
      sibling.view.dispatch({ changes, annotations: syncAnnotation.of(true) });
    }

    // Fragment⇄file overlap: views of the same file under different keys.
    const spec = source.handle?.takePendingChangeSpec() ?? null;
    if (spec !== null) {
      this.mirrorAcrossPath(source, spec, newSource);
    }
  }

  private mirrorAcrossPath(
    source: ViewSlot,
    spec: DocumentChangeSpec,
    newSource: string,
  ): void {
    const inserted = spec.text ?? newSource;
    for (const other of this.slots.values()) {
      if (other.docKey === source.docKey || other.path !== source.path) continue;
      if (other.view === null) continue;

      if (other.handle === null || !other.handle.isFragment) {
        if (source.handle !== null && source.handle.isFragment) {
          // Fragment edit → full-file view: apply the spec as a CM6 change
          // (UTF-16 file coordinates against the previous content).
          other.view.dispatch({
            changes: { from: spec.start, to: spec.end, insert: inserted },
            annotations: syncAnnotation.of(true),
          });
        } else {
          // File-handle source under a different key (degraded symbol tab):
          // refresh wholesale.
          this.refreshSlotFromFile(other);
        }
        continue;
      }

      // Sibling fragment of the same file (different symbol, or a file edit
      // overlapping a fragment): shift its range when the change lies
      // entirely outside, refresh from the file otherwise.
      const range = other.handle.fragmentRange();
      const delta = inserted.length - (spec.end - spec.start);
      if (range !== null && spec.end <= range.start) {
        other.handle = this.reopenFragment(other, {
          start: range.start + delta,
          end: range.end + delta,
        });
      } else if (range !== null && spec.start >= range.end) {
        // Entirely after: nothing shifts.
      } else {
        this.refreshSlotFromFile(other);
      }
    }
  }

  /** Close + reopen a fragment slot's handle at a new range. */
  private reopenFragment(
    slot: ViewSlot,
    range: { start: number; end: number },
  ): DocHandle | null {
    const session = this.project.getSession();
    slot.handle?.close();
    const id = session.openFragment(slot.path, range.start, range.end);
    if (id === null) return null;
    const handle = new DocHandle(session, id, slot.path, true);
    handle.setFragmentRange(range.start, range.end);
    this.symbolHints.set(slot.docKey, range);
    return handle;
  }

  /**
   * Reload a mounted view's content from the session: re-resolve symbol
   * ranges by name (full file when the symbol vanished), reopen the handle,
   * and replace the view's doc when the text differs.
   */
  private refreshSlotFromFile(slot: ViewSlot, opts?: { allowHint?: boolean }): void {
    if (slot.view === null) return;
    slot.handle?.close();
    slot.handle = this.openHandle(slot, { allowHint: opts?.allowHint ?? false });
    if (slot.handle !== null && !slot.handle.isFragment) {
      // The symbol vanished: the stale hint must not resurrect the old range
      // on a later remount either.
      this.symbolHints.delete(slot.docKey);
    }
    const content = slot.handle?.viewSource() ?? this.fileContent(slot.path);
    if (slot.view.state.doc.toString() !== content) {
      slot.view.dispatch({
        changes: { from: 0, to: slot.view.state.doc.length, insert: content },
        annotations: syncAnnotation.of(true),
      });
    }
  }

  // ── Private: focus / reveal / compile plumbing ───────────────────

  private applyFocusSideEffects(slot: ViewSlot): void {
    if (slot.view === null) return;
    this.callbacks.onFocusedViewChange?.(slot.view);
    this.reportCursor(slot.view);
  }

  private applyPendingReveal(slot: ViewSlot): void {
    // Slot-targeted entries (split-view restores) take precedence over
    // docKey-wide ones; consume whichever matched.
    const ownKey = slotId(slot.docKey, slot.groupId);
    const key = this.pendingReveals.has(ownKey) ? ownKey : slot.docKey;
    const pending = this.pendingReveals.get(key);
    if (pending === undefined || slot.view === null) return;
    this.pendingReveals.delete(key);
    const view = slot.view;
    const docLength = view.state.doc.length;
    // Clamp both ends: host-persisted snapshots can be stale (doc shrank) or
    // corrupted (negative offsets) — a bad snapshot must degrade, not throw
    // inside mountView.
    const clamp = (n: number) => Math.max(0, Math.min(n, docLength));
    if (pending.kind === "reveal") {
      const offset = clamp(pending.offset);
      view.dispatch({
        selection: { anchor: offset },
        effects: EditorView.scrollIntoView(offset, { y: "center" }),
      });
      view.focus();
      return;
    }
    // restore (#347): full selection + pixel scroll, no focus steal.
    view.dispatch({ selection: { anchor: clamp(pending.anchor), head: clamp(pending.head) } });
    setScrollTop(view, Math.max(0, pending.scrollTop));
  }

  private deliverCompile(result: CompileResult): void {
    // Several views compile on their own debounce; the project-level cache
    // makes repeats cheap and reference-equal, so identical deliveries are
    // collapsed here.
    if (result === this.lastCompileDelivered) return;
    this.lastCompileDelivered = result;
    // A name added in ONE file must reach the prose checker in another, so
    // the dictionary's cache key is the project's analysis rather than any
    // file's text (#3210).
    this.project.invalidateProseDictionary();
    // A compile/analysis completing is not a CM transaction, so the HIR
    // overlay's StateField would keep its (possibly empty) seed until the
    // next doc change (#494). Re-read the projection in every mounted view —
    // the initial debounced compile after a passive load lands here, which
    // is what makes the overlay paint without the user typing. Slots without
    // a view are handled by the mount-time refresh in mountView (#518):
    // `lastCompileDelivered` set above is what tells a later mount that it
    // missed this loop.
    for (const slot of this.slots.values()) {
      if (slot.view !== null) {
        this.refreshOverlayPrepared(slot);
        // Prose too, and for a reason the overlay's comment does not cover:
        // a compile is also how a `brink.toml` edit lands, so `[prose]
        // enable`/`dialect` changing has no other signal to re-check on. The
        // dictionary (invalidated above) is the other input that moves here.
        // Compile squiggles need the same wake-up, and for the same reason
        // (#3260): they are published by the diagnostics ViewPlugin on
        // `docChanged`, and a `brink.toml` edit is a compile with no
        // document change in THIS view. Suppressing a code project-wide and
        // watching the squiggle stay is what that gap looked like.
        //
        // Safe against recursion: the re-compile returns the project cache's
        // reference-equal result, which `lastCompileDelivered` above drops.
        slot.view.dispatch({
          effects: [refreshProseEffect.of(), refreshDiagnosticsEffect.of()],
        });
      }
    }
    this.callbacks.onCompileResult?.(result);
  }

  /**
   * Overlay refresh with the projection fetched FIRST (W5c): the stash is
   * filled from the doc client (worker replica, or the in-process client
   * in fallback environments) and the refresh effect dispatches on
   * landing, so the field's synchronous rebuild never pulls analysis on
   * the main thread. A failed/dropped fetch still refreshes — the
   * getter's session fallback covers it (mocks, small docs).
   */
  private refreshOverlayPrepared(slot: ViewSlot): void {
    const view = slot.view;
    if (view === null) return;
    const id = slot.handle?.id;
    if (id === undefined) {
      refreshHirOverlay(view);
      return;
    }
    void this.project
      .docClient()
      .query<import("@brink/wasm-types").HirProjection>("getHirSpansDoc", [id], {
        priority: "background",
        doc: id,
        coalesceKey: `overlay:${id}`,
      })
      .promise.then(
        (r) => {
          if (slot.handle?.id === id) slot.handle.stashProjection(r.value);
          if (slot.view === view && view.dom.isConnected) refreshHirOverlay(view);
        },
        () => {
          if (slot.view === view && view.dom.isConnected) refreshHirOverlay(view);
        },
      );
  }

  private reportCursor(view: EditorView): void {
    const state = view.state;
    const pos = state.selection.main.head;
    const line = state.doc.lineAt(pos);
    const col = pos - line.from;
    this.callbacks.onCursorChange?.(line.number, col + 1);

    const infos = state.field(elementTypeField, false) ?? [];
    const info = infos[line.number - 1] ?? null;
    let hints: KeyHint[] = [];
    if (info) {
      const hasContent = lineHasContent(line.text, info);
      const lineCtx = buildContext(infos, line.number - 1);
      hints = getHintsForElement(info, hasContent, lineCtx);
    }
    this.callbacks.onLineInfoChange?.(info, hints);
  }
}

// ── Symbol lookup ────────────────────────────────────────────────────

/** Depth-first search by name over a file's symbols (knots, then stitches). */
function findSymbolByName(
  symbols: readonly DocumentSymbol[],
  name: string,
): DocumentSymbol | null {
  for (const symbol of symbols) {
    if (symbol.name === name) return symbol;
    const child = findSymbolByName(symbol.children, name);
    if (child !== null) return child;
  }
  return null;
}
