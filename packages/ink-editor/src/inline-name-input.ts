/**
 * InlineNameInput (#315 H) — the shared in-editor name-prompt primitive.
 *
 * A CM6 replace-widget chip hosting a styled name input, a "⚠ breaks N" badge,
 * and (expanded) an inline breakage report — the affected-reference list plus
 * [Cancel] / [<forceLabel>]. It is the factored core behind both:
 *
 *  - inline **rename** (#323/#324) — seeded with the symbol's current name, a
 *    live debounced badge as you type, safe commit on Enter (see rename.ts); and
 *  - **extract to knot/function** (#315 H) — an empty prompt; on Enter the name
 *    is run through the extract op and the same safe-by-default gate applies.
 *
 * SAFE-BY-DEFAULT: a `query(name)` returning a safe {@link StructuralResult}
 * commits immediately on Enter; an unsafe result expands the inline breakage
 * report and commits only on the explicit force button.
 *
 * The controller owns the input, badge, report DOM, the debounce timer, and the
 * key listener, and tears them all down in `dispose()` (the code-actions.ts
 * teardown pattern) — an open prompt must never leak when the editor unmounts.
 *
 * OFF THE PAINT PATH (#722): `query` is a synchronous wasm call and can run
 * long (collision/breakage analysis) under load. The debounce timer no longer
 * calls it inline — it flips the badge into a "pending" state synchronously
 * (so that paints before the heavy call runs) and defers the actual call to
 * the next idle slot via {@link scheduleIdleWork}. An Enter (or force click)
 * that lands while a query is in flight for the current name does not force
 * a synchronous call either; it sets a "commit requested" flag and the commit
 * completes once the deferred query resolves — the apply/force path is
 * effectively disabled until then. `query` itself stays a plain synchronous
 * function (unchanged signature) so callers — including tests — keep the
 * existing synchronous API; only the *scheduling* of the call changed.
 *

 * The DOM reuses the `brink-inline-rename-*` classes so both flows share one
 * stylesheet (rename-prompt.css); the styling is name-prompt-generic.
 */

import type { StructuralResult } from "@brink/wasm-types";
import { isSafeRename, breakageCount, breakageEntries } from "./breakage.js";
import { scheduleIdleWork, cancelIdleWork, type IdleHandle } from "./idle-schedule.js";

/** Context handed to a host `onBreakage` override. */
export interface InlineNameBreakageContext {
  /** The name currently in the input. */
  name: string;
}

export interface InlineNameInputOptions {
  /** Seed value for the input (rename: current name; extract: ""). */
  initialValue: string;
  /** Placeholder shown when the input is empty (extract: "new name…"). */
  placeholder?: string;
  /** aria-label for the input (e.g. "Rename greeting", "Extract to knot"). */
  ariaLabel: string;
  /** Text for the "commit anyway" button in the unsafe report. */
  forceLabel: string;
  /**
   * Head line of the breakage report, built from the entered name + entry
   * count. Defaults to "<name> breaks N references:".
   */
  reportHead?: (name: string, count: number) => string;
  /**
   * Compute the {@link StructuralResult} for `name`. Side-effect-free — it
   * returns the new sources + breakage report without applying anything. A
   * `null` return (or a throw) is treated as "no result" (badge hidden, Enter
   * cancels). The commit applies `result` through {@link InlineNameInputOptions.onCommit}.
   */
  query: (name: string) => StructuralResult | null;
  /**
   * Commit an already-computed result — apply its edits. Called for a safe
   * Enter or an explicit force.
   */
  onCommit: (result: StructuralResult, name: string) => void;
  /** Called on cancel (Esc / empty / unchanged), before teardown. */
  onCancel?: () => void;
  /**
   * When true, re-run `query` on a ~250ms debounce as the user types and
   * refresh the "⚠ breaks N" badge live (rename). When false the query runs
   * only on Enter (extract — the op is heavier and the intent is deliberate).
   */
  liveBadge?: boolean;
  /**
   * Optional host override for the breakage surface. Return `true` to suppress
   * the default inline report (the host renders its own).
   */
  onBreakage?: (result: StructuralResult, ctx: InlineNameBreakageContext) => boolean;
}

/** Debounce window for the live breakage query (#324). */
const QUERY_DEBOUNCE_MS = 250;

/**
 * A live inline name-prompt session. Owns the input, badge, report DOM, the
 * debounce timer, and the document listeners — all torn down in `dispose()`.
 */
export class InlineNameInput {
  private root: HTMLElement | null = null;
  private input: HTMLInputElement | null = null;
  private badge: HTMLButtonElement | null = null;
  private report: HTMLElement | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private lastResult: StructuralResult | null = null;
  /** Handle for the deferred (off-paint-path) query call, if one is in
   *  flight; see the OFF THE PAINT PATH note above. */
  private idleHandle: IdleHandle | null = null;
  /** The name a deferred query is currently in flight for, or null when idle. */
  private pendingName: string | null = null;
  /** True while `pendingName`'s query is in flight — surfaced as the badge's
   *  pending state, and gates the apply/force path. */
  private pending = false;
  /** Set by `confirm()` (Enter) or a force click when it lands mid-flight;
   *  consumed once the in-flight query resolves to finish the commit. */
  private commitRequested = false;
  private reportOpen = false;
  private disposed = false;
  private readonly cache = new Map<string, StructuralResult>();
  private readonly keyHandler: (e: KeyboardEvent) => void;

  constructor(
    private readonly options: InlineNameInputOptions,
    /** Invoked once the prompt closes (commit or cancel) so the host can drop
     *  the widget decoration and refocus the editor. */
    private readonly onClose: () => void,
  ) {
    this.keyHandler = (e) => this.onKeyDown(e);
  }

  /** Build the widget DOM (input + badge + report container) and wire it. */
  render(): HTMLElement {
    const root = document.createElement("span");
    root.className = "brink-inline-rename";

    const row = document.createElement("span");
    row.className = "brink-inline-rename-row";

    const input = document.createElement("input");
    input.type = "text";
    input.className = "brink-inline-rename-input";
    input.value = this.options.initialValue;
    if (this.options.placeholder !== undefined) input.placeholder = this.options.placeholder;
    input.spellcheck = false;
    input.setAttribute("aria-label", this.options.ariaLabel);
    if (this.options.liveBadge === true) {
      input.addEventListener("input", () => this.scheduleQuery());
    }
    input.addEventListener("keydown", this.keyHandler);

    const badge = document.createElement("button");
    badge.type = "button";
    badge.className = "brink-inline-rename-badge";
    badge.hidden = true;
    badge.setAttribute("aria-label", "Show breakage report");
    badge.addEventListener("click", () => this.toggleReport());
    // Belt-and-suspenders: `disabled` already blocks native clicks while
    // pending, but a synthetic dispatchEvent (tests, some assistive tech
    // shims) can bypass that — toggleReport() itself also gates on pending.

    const report = document.createElement("div");
    report.className = "brink-inline-rename-report";
    report.hidden = true;
    report.setAttribute("role", "group");
    report.setAttribute("aria-label", "Breakage report");

    row.append(input, badge);
    root.append(row, report);

    this.root = root;
    this.input = input;
    this.badge = badge;
    this.report = report;
    // Focus + select after CM mounts the widget. The deferral is load-bearing
    // for `focus()`: `render()` is called from `WidgetType.toDOM()`, which
    // returns the element *before* CM inserts it, and `focus()` on a detached
    // element is a no-op. CM6's `WidgetType` offers no post-mount hook —
    // `toDOM`/`updateDOM`/`coordsAt`/`destroy` are the whole lifecycle, and
    // `destroy` fires on removal — so there is nothing to replace the timer
    // with (#2535).
    //
    // `select()` must not be deferred unconditionally, though
    // (docs/studio-shell-spec.md §7.7.1 rule 2): a user who types during the
    // deferred window would have their text selected, and the next keystroke
    // would replace it. Unlike #2511's value-clobber — which made the rename a
    // no-op — this one lets the rename *succeed with the wrong string*. Only
    // select a field that still holds what we seeded it with, i.e. one nobody
    // has touched; this mirrors the guard PR #2523 landed in
    // `packages/studio-ui/src/SymbolRenamePrompt.tsx`. Pinned by
    // `packages/brink-studio/src/__tests__/inline-name-input-seed.test.ts`.
    setTimeout(() => {
      input.focus();
      if (input.value === this.options.initialValue) input.select();
    }, 0);
    return root;
  }

  private scheduleQuery(): void {
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = setTimeout(() => {
      this.timer = null;
      this.startLiveQuery();
    }, QUERY_DEBOUNCE_MS);
  }

  /** Debounce settle for a live (typing-driven) query: resolves from cache
   *  synchronously, else kicks off a deferred query via {@link beginQuery}. */
  private startLiveQuery(): void {
    const input = this.input;
    if (input === null) return;
    const name = input.value.trim();
    if (name === "" || name === this.options.initialValue) {
      this.abandonPending();
      this.lastResult = null;
      this.updateBadge(null);
      return;
    }
    this.beginQuery(name);
  }

  /**
   * Resolve `name`'s breakage result — from cache synchronously, or by
   * kicking off (or joining an already in-flight) deferred query. Marks the
   * UI pending *before* the heavy call runs so a paint can land first; see
   * the OFF THE PAINT PATH note on the class doc comment.
   */
  private beginQuery(name: string): void {
    const cached = this.cache.get(name);
    if (cached !== undefined) {
      this.resolveQuery(name, cached);
      return;
    }
    if (this.pending && this.pendingName === name) return; // already in flight
    if (this.idleHandle !== null) {
      cancelIdleWork(this.idleHandle);
      this.idleHandle = null;
    }
    this.pending = true;
    this.pendingName = name;
    this.updateBadge(this.lastResult);
    this.idleHandle = scheduleIdleWork(() => {
      this.idleHandle = null;
      let queried: StructuralResult | null;
      try {
        queried = this.options.query(name);
      } catch {
        queried = null;
      }
      this.resolveQuery(name, queried);
    });
  }

  /** Cancel any in-flight deferred query without resolving it (input reverted
   *  to empty/unchanged while it was pending). */
  private abandonPending(): void {
    if (this.idleHandle !== null) {
      cancelIdleWork(this.idleHandle);
      this.idleHandle = null;
    }
    this.pending = false;
    this.pendingName = null;
  }

  /** Land a resolved (possibly deferred, possibly cached) query result for
   *  `name`, then finish any commit that was requested while it was pending. */
  private resolveQuery(name: string, result: StructuralResult | null): void {
    this.pending = false;
    this.pendingName = null;
    if (result === null) {
      this.lastResult = null;
      this.updateBadge(null);
    } else {
      this.cache.set(name, result);
      this.lastResult = result;
      this.updateBadge(result);
    }
    if (!this.commitRequested) return;
    this.commitRequested = false;
    // Drop a stale commit request: the user kept typing before this query
    // resolved, so `name` no longer matches the live input. A fresh Enter
    // re-issues against whatever is current.
    const input = this.input;
    if (input === null || input.value.trim() !== name) return;
    if (result === null) {
      // No result (query error, e.g. name collision/header crossing/illegal
      // body): restore the pre-#722 semantics — Enter on a null result
      // cancels the prompt instead of leaving it open in a dead pending
      // state (see #722 review).
      this.cancel();
      return;
    }
    this.settleCommit(result, name);
  }

  /** Refresh the "⚠ breaks N" badge (hidden when safe) or its pending state. */
  private updateBadge(result: StructuralResult | null): void {
    const badge = this.badge;
    if (badge === null) return;
    if (this.pending) {
      badge.hidden = false;
      badge.disabled = true;
      badge.setAttribute("aria-busy", "true");
      badge.classList.add("brink-inline-rename-badge--pending");
      badge.textContent = "…";
      // Disable (don't tear down) an already-open report's force button —
      // it reflects a name that's now stale — until the fresh result lands.
      const force = this.report?.querySelector<HTMLButtonElement>(".brink-inline-rename-force");
      if (force) {
        force.disabled = true;
        force.setAttribute("aria-busy", "true");
      }
      return;
    }
    badge.disabled = false;
    badge.removeAttribute("aria-busy");
    badge.classList.remove("brink-inline-rename-badge--pending");
    if (result === null || isSafeRename(result)) {
      badge.hidden = true;
      this.closeReport();
      return;
    }
    const n = breakageCount(result);
    badge.hidden = false;
    badge.textContent = `⚠ breaks ${n}`;
    if (this.reportOpen) this.renderReport(result);
  }

  private toggleReport(): void {
    if (this.pending) return;
    if (this.reportOpen) this.closeReport();
    else if (this.lastResult !== null && !isSafeRename(this.lastResult)) {
      this.renderReport(this.lastResult);
    }
  }

  private closeReport(): void {
    this.reportOpen = false;
    if (this.report !== null) {
      this.report.hidden = true;
      this.report.replaceChildren();
    }
  }

  /** Render the inline breakage report: affected-reference list + actions. */
  private renderReport(result: StructuralResult): void {
    const report = this.report;
    const input = this.input;
    if (report === null || input === null) return;
    const name = input.value.trim();

    // Host override: let a host render its own surface and suppress the default.
    if (this.options.onBreakage?.(result, { name }) === true) {
      this.closeReport();
      return;
    }

    this.reportOpen = true;
    report.hidden = false;
    report.replaceChildren();

    const entries = breakageEntries(result);
    const head = document.createElement("p");
    head.className = "brink-inline-rename-report-head";
    head.textContent =
      this.options.reportHead?.(name, entries.length) ??
      `${name} breaks ${entries.length} ${
        entries.length === 1 ? "reference" : "references"
      }:`;

    const list = document.createElement("ul");
    list.className = "brink-inline-rename-report-list";
    for (const entry of entries) {
      const li = document.createElement("li");
      li.className = "brink-inline-rename-report-item";
      const loc = document.createElement("span");
      loc.className = "brink-inline-rename-report-loc";
      loc.textContent = entry.line !== undefined ? `${entry.file}:${entry.line}` : entry.file;
      const msg = document.createElement("span");
      msg.className = "brink-inline-rename-report-msg";
      msg.textContent = entry.message;
      li.append(loc, msg);
      list.appendChild(li);
    }

    const actions = document.createElement("div");
    actions.className = "brink-inline-rename-report-actions";
    const cancel = document.createElement("button");
    cancel.type = "button";
    cancel.className = "brink-inline-rename-cancel";
    cancel.textContent = "Cancel";
    cancel.addEventListener("click", () => this.cancel());
    const force = document.createElement("button");
    force.type = "button";
    force.className = "brink-inline-rename-force";
    force.textContent = this.options.forceLabel;
    force.addEventListener("click", () => this.commit(result, name));

    actions.append(cancel, force);
    report.append(head, list, actions);
    // Focus the override so an unsafe Enter lands on a deliberate confirmation.
    setTimeout(() => force.focus(), 0);
  }

  private onKeyDown(e: KeyboardEvent): void {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      this.cancel();
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      e.stopPropagation();
      this.confirm();
    }
  }

  /**
   * Enter: commit a safe result; surface the report (focus force) when
   * unsafe. When no cached result exists yet, this does NOT force a
   * synchronous query — it requests a commit and defers to
   * {@link beginQuery}/{@link resolveQuery}, same as a live debounce settle,
   * so Enter never blocks the keystroke on the analysis. The apply path is
   * effectively disabled (nothing commits) until that resolves.
   */
  private confirm(): void {
    const input = this.input;
    if (input === null) return;
    const name = input.value.trim();
    if (name === "" || name === this.options.initialValue) {
      this.cancel();
      return;
    }
    // Resolve any pending debounce immediately so Enter acts on fresh data.
    if (this.timer !== null) {
      clearTimeout(this.timer);
      this.timer = null;
    }
    const cached = this.cache.get(name);
    if (cached !== undefined) {
      this.settleCommit(cached, name);
      return;
    }
    this.commitRequested = true;
    this.beginQuery(name);
  }

  /** Finish a confirm once a result is available: commit a safe result, or
   *  surface the report (focusing the explicit override) when unsafe. */
  private settleCommit(result: StructuralResult, name: string): void {
    if (isSafeRename(result)) {
      this.commit(result, name);
      return;
    }
    if (!this.reportOpen) this.renderReport(result);
    else {
      const force = this.report?.querySelector<HTMLButtonElement>(".brink-inline-rename-force");
      force?.focus();
    }
  }

  private commit(result: StructuralResult, name: string): void {
    this.options.onCommit(result, name);
    this.close();
  }

  private cancel(): void {
    this.options.onCancel?.();
    this.close();
  }

  private close(): void {
    if (this.disposed) return;
    this.onClose();
  }

  /** Tear down everything this controller owns: the debounce timer, the report,
   *  and the widget DOM with its listeners (the input listeners die with the
   *  removed node; we null our refs so a late callback is inert). */
  dispose(): void {
    this.disposed = true;
    if (this.timer !== null) {
      clearTimeout(this.timer);
      this.timer = null;
    }
    if (this.idleHandle !== null) {
      cancelIdleWork(this.idleHandle);
      this.idleHandle = null;
    }
    this.pending = false;
    this.pendingName = null;
    this.commitRequested = false;
    this.input?.removeEventListener("keydown", this.keyHandler);
    this.root?.remove();
    this.root = null;
    this.input = null;
    this.badge = null;
    this.report = null;
    this.cache.clear();
    this.lastResult = null;
    this.reportOpen = false;
  }
}
