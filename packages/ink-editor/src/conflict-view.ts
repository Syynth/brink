/**
 * conflict-view — the external-conflict merge surface (issue #320, Track V).
 *
 * When the host rewrites a file the studio has an unsaved, divergent buffer
 * for, the B1 hook ({@link FileChangeHub.detectExternalConflict}) keeps the
 * buffer and flags the path conflicted instead of clobbering it. This module
 * is the *visual* half: given a {@link FileConflict}, it mounts
 *
 *  - a banner — "Changed on disk while you had unsaved edits" with
 *    [Keep mine] / [Use disk] actions, and
 *  - a side-by-side, 2-way `@codemirror/merge` MergeView (YOURS vs ON DISK,
 *    no baseline column) over the conflicted document.
 *
 * It is a self-contained, framework-agnostic component (like `find-panel`):
 * the studio renders a host container and hands it a conflict + resolution
 * callbacks; the React layer stays thin. Following the CodeActionsMenu
 * teardown contract, {@link ConflictView.destroy} removes every listener and
 * DOM node and destroys the MergeView — nothing leaks when the editor (or the
 * conflict) goes away.
 *
 * Resolution is reported through callbacks, never applied here — the
 * FileChangeHub baseline/dirty seam (via ProjectSession) owns the mutation:
 *
 *  - "Use disk"     → `onUseDisk()`            (re-baseline to disk, clear)
 *  - "Keep mine"    → `onKeepMine()`           (keep buffer dirty, clear)
 *  - merged edit    → `onMerge(mergedText)`    (apply merged buffer, clear)
 *
 * "Apply merge" is enabled only once the YOURS pane diverges from the kept
 * buffer (the user actually edited the merge); otherwise the two explicit
 * keep/use actions are the resolution.
 */

import { EditorState, type Extension } from "@codemirror/state";
import { EditorView, lineNumbers } from "@codemirror/view";
import { MergeView } from "@codemirror/merge";
import type { FileConflict } from "./file-change-hub.js";
import { brinkTheme } from "./theme.js";

/** Resolution callbacks for a single conflicted document. */
export interface ConflictViewOptions {
  /** "Use disk": discard the studio buffer, take the host's on-disk text. */
  onUseDisk(): void;
  /** "Keep mine": keep the studio buffer (stays dirty), drop the flag. */
  onKeepMine(): void;
  /** "Apply merge": adopt the user's hand-merged text as the buffer. */
  onMerge(merged: string): void;
}

function baseExtensions(): Extension[] {
  return [lineNumbers(), brinkTheme, EditorView.lineWrapping];
}

/** Read-only base extensions for the ON DISK (right) pane. */
function diskExtensions(): Extension[] {
  return [...baseExtensions(), EditorState.readOnly.of(true), EditorView.editable.of(false)];
}

export class ConflictView {
  private readonly root: HTMLElement;
  private readonly options: ConflictViewOptions;
  private conflict: FileConflict;

  private merge: MergeView | null = null;
  private banner: HTMLElement | null = null;
  private applyBtn: HTMLButtonElement | null = null;

  constructor(root: HTMLElement, conflict: FileConflict, options: ConflictViewOptions) {
    this.root = root;
    this.conflict = conflict;
    this.options = options;
    this.render();
  }

  /** Swap to a new conflict (e.g. the active document changed) without
   *  re-creating the host wiring. Tears the old view down first. */
  update(conflict: FileConflict): void {
    this.conflict = conflict;
    this.teardownDom();
    this.render();
  }

  /** The current ON DISK text (right pane). */
  diskText(): string {
    return this.conflict.disk;
  }

  /** The live YOURS (left) editor text — the candidate merged result. */
  minedText(): string {
    return this.merge?.a.state.doc.toString() ?? this.conflict.buffer;
  }

  /**
   * Tear down: destroy the MergeView (which removes its two editors, their
   * DOM, and the update listener registered as one of their extensions) and
   * remove the banner + captions. Mirrors the CodeActionsMenu teardown
   * contract — leaks are bugs.
   */
  destroy(): void {
    this.teardownDom();
  }

  // ── Internal ─────────────────────────────────────────────────────

  private teardownDom(): void {
    if (this.merge) {
      this.merge.destroy();
      this.merge = null;
    }
    if (this.banner) {
      this.banner.remove();
      this.banner = null;
    }
    this.applyBtn = null;
    // The MergeView appends its own DOM under root; clear any stragglers.
    this.root.replaceChildren();
  }

  private render(): void {
    const { path, buffer, disk } = this.conflict;

    // ── Banner ───────────────────────────────────────────────────
    const banner = document.createElement("div");
    banner.className = "brink-conflict-banner";
    banner.setAttribute("role", "alert");

    const message = document.createElement("span");
    message.className = "brink-conflict-message";
    message.textContent = `“${path}” changed on disk while you had unsaved edits.`;
    banner.appendChild(message);

    const actions = document.createElement("div");
    actions.className = "brink-conflict-actions";

    const keepMine = document.createElement("button");
    keepMine.type = "button";
    keepMine.className = "brink-conflict-btn brink-conflict-keep-mine";
    keepMine.textContent = "Keep mine";
    keepMine.setAttribute("aria-label", `Keep my unsaved edits to ${path}`);
    keepMine.addEventListener("click", this.handleKeepMine);

    const useDisk = document.createElement("button");
    useDisk.type = "button";
    useDisk.className = "brink-conflict-btn brink-conflict-use-disk";
    useDisk.textContent = "Use disk";
    useDisk.setAttribute("aria-label", `Discard my edits and use the on-disk version of ${path}`);
    useDisk.addEventListener("click", this.handleUseDisk);

    const apply = document.createElement("button");
    apply.type = "button";
    apply.className = "brink-conflict-btn brink-conflict-apply-merge";
    apply.textContent = "Apply merge";
    apply.setAttribute("aria-label", `Apply the merged result for ${path}`);
    apply.disabled = true; // enabled once the YOURS pane is edited
    apply.addEventListener("click", this.handleApplyMerge);
    this.applyBtn = apply;

    actions.append(keepMine, useDisk, apply);
    banner.appendChild(actions);
    this.root.appendChild(banner);
    this.banner = banner;

    // ── Pane captions (YOURS | ON DISK) ──────────────────────────
    const captions = document.createElement("div");
    captions.className = "brink-conflict-captions";
    const yoursCap = document.createElement("span");
    yoursCap.className = "brink-conflict-caption brink-conflict-caption-yours";
    yoursCap.textContent = "Yours (unsaved)";
    const diskCap = document.createElement("span");
    diskCap.className = "brink-conflict-caption brink-conflict-caption-disk";
    diskCap.textContent = "On disk";
    captions.append(yoursCap, diskCap);
    banner.insertAdjacentElement("afterend", captions);

    // ── 2-way merge (YOURS vs ON DISK) ───────────────────────────
    //
    // `a` = YOURS (the kept, editable buffer); `b` = ON DISK (read-only).
    // No baseline column — this is a plain 2-way diff. The user can edit the
    // YOURS pane to hand-merge, then "Apply merge".
    const mergeHost = document.createElement("div");
    mergeHost.className = "brink-conflict-merge";
    this.root.appendChild(mergeHost);

    const mineChanged = EditorView.updateListener.of((u) => {
      if (u.docChanged) this.refreshApplyEnabled();
    });

    this.merge = new MergeView({
      a: {
        doc: buffer,
        extensions: [...baseExtensions(), mineChanged],
      },
      b: {
        doc: disk,
        extensions: diskExtensions(),
      },
      parent: mergeHost,
      orientation: "a-b",
      // No revert arrows: resolution is via the banner actions, not chunk
      // reverts, so the surface reads as "yours vs disk" not "merge into".
      gutter: true,
      highlightChanges: true,
    });

    // Label the two panes for accessibility / clarity.
    this.labelPane(this.merge.a, "Yours (unsaved)");
    this.labelPane(this.merge.b, "On disk");

    this.refreshApplyEnabled();
  }

  /** Mark the "Apply merge" button enabled iff the YOURS pane diverges from
   *  the originally-kept buffer (the user actually merged something). */
  private refreshApplyEnabled(): void {
    if (!this.applyBtn) return;
    this.applyBtn.disabled = this.minedText() === this.conflict.buffer;
  }

  private labelPane(view: EditorView, label: string): void {
    view.dom.setAttribute("aria-label", label);
    view.dom.dataset.brinkConflictPane = label;
  }

  private readonly handleKeepMine = (): void => {
    this.options.onKeepMine();
  };

  private readonly handleUseDisk = (): void => {
    this.options.onUseDisk();
  };

  private readonly handleApplyMerge = (): void => {
    this.options.onMerge(this.minedText());
  };
}
