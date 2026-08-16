/**
 * SymbolRenamePrompt — the knot/stitch rename surface (#305).
 *
 * Two-state, safe-by-default. State 1 is a name input (pre-filled + selected
 * with the current name). On confirm it runs `performSymbolRename`: if the
 * rename introduces no diagnostics it applies immediately and closes; if it
 * would break something it flips to State 2 — a breakage report listing the
 * introduced diagnostics — whose only override is an explicit **Force rename**.
 * Mirrors the CLI's safe-by-default + `--unsafe` gate.
 *
 * Mounted once near the studio root (next to `SymbolContextMenuHost`); driven
 * by the `renamePrompt` request the shared context menu raises.
 */

import { useEffect, useRef, useState } from "react";
import { Overlay, EDITOR_REVEAL_COMMAND_ID, useShell } from "@brink/studio-shell";
import type { RenameDiagnostic } from "@brink/wasm-types";
import { useStudioStore, useStudioStoreApi } from "./StoreContext.js";
import { performSymbolRename } from "./symbolMenuActions.js";

/** Byte offset of a 1-based line:col into `text` (clamped). Renames don't add
 *  or remove lines, so a hypothetical diagnostic's line:col maps cleanly onto
 *  the current document for navigation. */
function lineColToOffset(text: string, line: number, col: number): number {
  let offset = 0;
  let ln = 1;
  for (let i = 0; i < text.length && ln < line; i++) {
    if (text[i] === "\n") {
      ln++;
      offset = i + 1;
    }
  }
  return Math.min(offset + (col - 1), text.length);
}

export function SymbolRenamePrompt() {
  const req = useStudioStore((s) => s.renamePrompt);
  const close = useStudioStore((s) => s.closeRenamePrompt);
  const applyMoveResult = useStudioStore((s) => s.applyMoveResult);
  const storeApi = useStudioStoreApi();
  const { commands } = useShell();

  const inputRef = useRef<HTMLInputElement | null>(null);
  const [report, setReport] = useState<RenameDiagnostic[] | null>(null);
  const [pendingName, setPendingName] = useState("");
  const [busy, setBusy] = useState(false);

  const open = req != null;
  const currentName = req ? (req.currentName ?? req.stitch ?? req.knot ?? "") : "";

  // Reset transient state on each fresh open, then focus/select the input.
  //
  // The input's *value* is seeded synchronously by React (`defaultValue` +
  // `key`, below), never from this frame callback (#2511). Seeding it here
  // used to leave a window in which the field was mounted, visible and
  // editable but still empty — and anything typed during that window was
  // overwritten when the frame ran. Because the field is uncontrolled and
  // `confirmName()` reads `input.value`, a clobbered rename degrades to
  // `name === currentName`, which closes the prompt without renaming
  // anything. `focus()` is harmless to defer — a user who has already typed
  // is already focused — but `select()` is not: selecting the whole value on
  // a field the user has already typed into would replace their next
  // keystroke, the same defect class this fix closes. Only select when the
  // field still holds the seeded name, i.e. nobody has touched it yet; see
  // `packages/brink-studio/src/__tests__/symbol-rename-prompt-seed.test.tsx`
  // for the guard that keeps this branch honest.
  useEffect(() => {
    if (!open) return;
    setReport(null);
    setPendingName("");
    setBusy(false);
    const id = requestAnimationFrame(() => {
      const input = inputRef.current;
      if (input) {
        input.focus();
        // SELECT-INVARIANT SymbolRenamePrompt.select: guarded by the
        // value-equality check on the line below — select() only fires when
        // the field still holds the seeded name, i.e. nobody has typed into
        // it yet during this deferred frame; see the comment block above
        // this effect and symbol-rename-prompt-seed.test.tsx.
        if (input.value === currentName) input.select();
      }
    });
    return () => cancelAnimationFrame(id);
  }, [open, currentName]);

  if (!req) return null;

  const label = req.stitch ? `${req.knot}.${req.stitch}` : (req.knot ?? currentName);

  const run = async (newName: string, force: boolean): Promise<void> => {
    if (busy) return;
    setBusy(true);
    const outcome = await performSymbolRename(
      storeApi.getState(),
      applyMoveResult,
      req,
      newName,
      force,
    );
    setBusy(false);
    if (outcome.applied || outcome.error) {
      close();
      return;
    }
    // Unsafe — surface the breakage report and require an explicit force.
    setPendingName(newName);
    setReport(outcome.diagnostics);
  };

  const confirmName = (): void => {
    const input = inputRef.current;
    if (!input) return;
    const name = input.value.trim();
    if (name === "" || name === currentName) {
      close();
      return;
    }
    void run(name, false);
  };

  const revealDiag = (d: RenameDiagnostic): void => {
    const source = storeApi.getState()._project?.getSession().getFileSource(d.path);
    if (source == null) return;
    const offset = lineColToOffset(source, d.line, d.col);
    commands.dispatch(EDITOR_REVEAL_COMMAND_ID, {
      kind: "source",
      file: d.path,
      span: { start: offset, end: offset },
    });
  };

  return (
    <Overlay open={open} onClose={close} className="shell-palette brink-rename-prompt">
      {report == null ? (
        <div className="brink-rename-input-row">
          <label className="brink-rename-label" htmlFor="brink-rename-input">
            Rename {label} to
          </label>
          <input
            // `key` re-mounts the field when the prompt is re-pointed at a
            // different symbol without closing first, so `defaultValue` — which
            // React only applies at mount — always reflects the current target
            // (#2511).
            key={currentName}
            id="brink-rename-input"
            ref={inputRef}
            className="shell-palette-input"
            type="text"
            aria-label="New symbol name"
            defaultValue={currentName}
            disabled={busy}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                confirmName();
              }
            }}
          />
        </div>
      ) : (
        <div
          className="brink-rename-report"
          role="alertdialog"
          aria-label="Rename breakage report"
        >
          <p className="brink-rename-report-head">
            Renaming <strong>{currentName}</strong> → <strong>{pendingName}</strong> would break{" "}
            {report.length} {report.length === 1 ? "place" : "places"}:
          </p>
          <ul className="brink-rename-report-list">
            {report.map((d, i) => (
              <li key={i} className={`brink-rename-diag brink-rename-diag-${d.severity}`}>
                <button
                  type="button"
                  className="brink-rename-diag-loc"
                  onClick={() => revealDiag(d)}
                >
                  {d.path}:{d.line}:{d.col}
                </button>{" "}
                <span className="brink-rename-diag-msg">{d.message}</span>
              </li>
            ))}
          </ul>
          <div className="brink-rename-report-actions">
            <button type="button" onClick={close} disabled={busy}>
              Cancel
            </button>
            <button
              type="button"
              className="brink-rename-force"
              disabled={busy}
              onClick={() => void run(pendingName, true)}
            >
              Force rename
            </button>
          </div>
        </div>
      )}
    </Overlay>
  );
}
