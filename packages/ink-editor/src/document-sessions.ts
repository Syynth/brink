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

import { EditorState, type ChangeSet, type Extension } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { basicSetup } from "codemirror";
import { defaultKeymap } from "@codemirror/commands";
import type {
  CodeAction,
  CompileResult,
  DialogueDialect,
  DocumentChangeSpec,
  DocumentSymbol,
  Location,
  StructuralResult,
} from "@brink/wasm-types";
import type { ExtractKind } from "./extract-actions.js";
import { getTokenTypeNames } from "@brink-lang/web";
import { brinkStudio, type BrinkStudioOptions } from "./extensions.js";
import { startInlineRename, type BreakageContext } from "./rename.js";
import {
  setFormGlyphMode,
  setFormAutoOpen,
  DEFAULT_FORM_GLYPH_MODE,
  type FormGlyphMode,
} from "./argument-widgets.js";
import { DocHandle, syncAnnotation } from "./document-handle.js";
import { refreshHirOverlay } from "./hir-overlay.js";
import { elementTypeField, type LineInfo } from "./element-type.js";
import { getHintsForElement, lineHasContent, buildContext } from "./transitions.js";
import { convertLineToType as cmConvertLineToType } from "./convert.js";
import type { ProjectSession } from "./project-session.js";

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
  /** Goto-definition targets a different file. */
  onNavigateToFile?(location: Location): void;
  /** "Play from here" (#186): start a session entered at a knot/stitch path. */
  onPlayFrom?(inkPath: string, label?: string): void;
  /** Right-click a knot/stitch declaration → open the shared symbol context
   *  menu (`path` is injected from the focused view's file). */
  onSymbolContextMenu?(
    info: { path: string; knot: string; stitch?: string },
    x: number,
    y: number,
  ): void;
  /** Inline rename (#323/#324): commit a safe (or forced) rename. `result` is
   *  the already-computed safe-rename payload; the host applies its edits and
   *  re-keys any open symbol tab. `path` is the file the rename ran in.
   *
   *  ⚠ `result` CAN carry `ok: false` with `safe: true` — a REFUSED rename
   *  (the op declined, e.g. renaming a symbol that can't be renamed), not a
   *  clean one. `result.safe` describes whether the (possibly empty) computed
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
   *  (the pre-#315 dismiss-only behavior). */
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

    const view = new EditorView({ state, parent });
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
      refreshHirOverlay(view);
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
   */
  triggerCompile(): void {
    const result = this.project.compileProject();
    this.deliverCompile(result);
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
  startInlineRenameAt(path: string, offset: number): boolean {
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
      startInlineRename(slot.view, viewOffset);
      return true;
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
      return id === null ? null : new DocHandle(session, id, slot.path, false);
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
    const found = findSymbolByName(this.project.getSession().getFileSymbols(path), symbol);
    if (found !== null) {
      return { start: found.full_start, end: found.full_end };
    }
    return allowHint ? (this.symbolHints.get(docKey) ?? null) : null;
  }

  private fileContent(path: string): string {
    return this.project.getSession().getFileSource(path) ?? "";
  }

  private slotExtensions(slot: ViewSlot): Extension[] {
    if (slot.extensions === null) {
      slot.extensions = [
        brinkStudio(this.slotOptions(slot)),
        basicSetup,
        keymap.of(defaultKeymap),
        EditorView.lineWrapping,
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
      dialect: this.options.dialect,
      compile: (source) => {
        slot.handle?.pushSource(source);
        project.notifyFileChanged(slot.path);
        // Kick off async INCLUDE resolution — next compile picks up new files.
        void project.refreshIncludes();
        return project.compileProject();
      },
      onCompile: (result) => this.deliverCompile(result),
      // No re-push (#14): the source for this transaction was already pushed by
      // the elementTypeField StateField (CM runs StateFields before decoration
      // facets), so every per-keystroke query just reads by DocId.
      getSemanticTokens: (_source) => slot.handle?.semanticTokens() ?? [],
      getHirProjection: () =>
        slot.handle?.hirProjection() ?? { spans: [], lines: [] },
      getTokenTypeNames,
      handleSlot: slot,
      getActiveFile: () => slot.path,
      onNavigateToFile: (location) => this.callbacks.onNavigateToFile?.(location),
      // Only expose the editor affordance when a handler is wired, so the
      // hover ▶ / right-click menu never appears as a dead control.
      onPlayFrom: this.callbacks.onPlayFrom
        ? (inkPath, label) => this.callbacks.onPlayFrom?.(inkPath, label)
        : undefined,
      // Inject the focused view's file path so the host can resolve the symbol.
      onSymbolContextMenu: this.callbacks.onSymbolContextMenu
        ? (info, x, y) =>
            this.callbacks.onSymbolContextMenu?.({ path: slot.path, ...info }, x, y)
        : undefined,
      getCompletions: (_source, offset) => slot.handle?.completions(offset) ?? [],
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
      getHover: (_source, offset) => slot.handle?.hover(offset) ?? null,
      gotoDefinition: (_source, offset) => slot.handle?.gotoDefinition(offset) ?? null,
      findReferences: (_source, offset) => slot.handle?.findReferences(offset) ?? [],
      prepareRename: (_source, offset) => slot.handle?.prepareRename(offset) ?? null,
      // Inline rename (#323/#324): the badge's live breakage query. Fold any
      // fragment-view origin into a whole-file UTF-16 offset, then compute the
      // safe-rename result (side-effect-free). Only wired alongside a commit
      // handler, so the F2 inline widget never appears as a dead control.
      renameSymbolAt: this.callbacks.onRenameCommit
        ? (offset, newName) => {
            const base = slot.handle?.fragmentRange()?.start ?? 0;
            return (
              slot.handle?.renameSymbolAt(base + offset, newName) ??
              emptyRenameResult(slot.path)
            );
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
      getCodeActions: (_source, offset) => slot.handle?.codeActions(offset) ?? [],
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
      getSignatureHelp: (_source, offset) => slot.handle?.signatureHelp(offset) ?? null,
      getFoldingRanges: () => slot.handle?.foldingRanges() ?? [],
    };
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
        this.mirrorEdit(slot, update.changes, update.state.doc.toString());
      }

      if (
        (update.docChanged || update.selectionSet) &&
        this.focusedSlotId === slotId(slot.docKey, slot.groupId)
      ) {
        this.reportCursor(update.view);
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
  private refreshSlotFromFile(slot: ViewSlot): void {
    if (slot.view === null) return;
    slot.handle?.close();
    slot.handle = this.openHandle(slot, { allowHint: false });
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
    // A compile/analysis completing is not a CM transaction, so the HIR
    // overlay's StateField would keep its (possibly empty) seed until the
    // next doc change (#494). Re-read the projection in every mounted view —
    // the initial debounced compile after a passive load lands here, which
    // is what makes the overlay paint without the user typing. Slots without
    // a view are handled by the mount-time refresh in mountView (#518):
    // `lastCompileDelivered` set above is what tells a later mount that it
    // missed this loop.
    for (const slot of this.slots.values()) {
      if (slot.view !== null) refreshHirOverlay(slot.view);
    }
    this.callbacks.onCompileResult?.(result);
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
