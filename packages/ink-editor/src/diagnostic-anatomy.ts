/**
 * The shape every diagnostic takes in a tooltip, whichever produced it.
 *
 * Two producers reach this surface — the compiler and the prose checker —
 * and before this they arrived looking like two different products: one
 * filed bug reports, the other asked questions, and neither had a
 * predictable place for its parts. The fix is not per-producer styling but
 * a shared ANATOMY: the same slots, in the same order, always.
 *
 * ```text
 *   ┌─────────────────────────────────────────┐
 *   │ spelling   `draught` is not in the …    │  label + title  (own row)
 *   │ [ drought ] [ draughts ] [ Add to … ]   │  fixes          (own row)
 *   │ prose:Spelling                          │  source tag     (own row)
 *   └─────────────────────────────────────────┘
 * ```
 *
 * The rows are produced by CodeMirror, not by us: it renders
 * `.cm-diagnosticText` (this function's output), then the action buttons,
 * then `.cm-diagnosticSource`, all as siblings. The stylesheet makes the
 * first and last full-width so the buttons flow between them — which is why
 * this returns only the label and title, and why the fixes are NOT wrapped
 * here. Building our own container for them would mean re-implementing
 * CodeMirror's action binding.
 *
 * **The label carries severity as a word, not only a colour.** The rail
 * colour alone fails a colourblind reader and fails a screenshot pasted
 * into an issue — which is how most of these get reported.
 */

/**
 * The `.cm-diagnosticText` content: a severity/kind label and the message.
 *
 * `label` is the short word that classifies the finding — `error`,
 * `warning` for the compiler, or the checker's own rule name (`spelling`)
 * for prose, which says more than the severity would.
 */
export function renderDiagnosticMessage(label: string, title: string): HTMLElement {
  const root = document.createElement("div");
  root.className = "cm-diag-body";

  const tag = document.createElement("span");
  // Kept lowercase deliberately: it reads as a category beside the message,
  // not as a shouted severity.
  tag.className = `cm-diag-label cm-diag-label-${label.toLowerCase()}`;
  tag.textContent = label.toLowerCase();
  root.append(tag);

  const text = document.createElement("span");
  text.className = "cm-diag-title";
  text.textContent = title;
  root.append(text);

  return root;
}
