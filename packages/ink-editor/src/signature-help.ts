import { type Extension, StateEffect, StateField } from "@codemirror/state";
import { EditorView, showTooltip, type Tooltip } from "@codemirror/view";
import type { SignatureInfo } from "@brink/wasm-types";

export interface SignatureHelpOptions {
  /** Sync or async (W2c of `docs/editor-worker-spec.md`). An async
   *  result lands only if it is the LATEST query and the document has
   *  not changed since it was issued — an out-of-order or stale landing
   *  is discarded (the change that staled it re-queried, or will on the
   *  next trigger). A rejected pull reads as "no signature". */
  getSignatureHelp: (
    source: string,
    offset: number,
  ) => SignatureInfo | null | Promise<SignatureInfo | null>;
}

const setSignatureTooltip = StateEffect.define<Tooltip | null>();

const signatureTooltipField = StateField.define<Tooltip | null>({
  create() {
    return null;
  },
  update(value, tr) {
    for (const e of tr.effects) {
      if (e.is(setSignatureTooltip)) return e.value;
    }
    return value;
  },
  provide: (f) => showTooltip.from(f),
});

export function signatureHelpExtension(options: SignatureHelpOptions): Extension {
  // Monotonic query counter (W2c): an async landing applies only if no
  // newer query was issued meanwhile (e.g. typing "((", or advancing the
  // active parameter while the tooltip is open).
  let querySeq = 0;
  return [
    signatureTooltipField,
    EditorView.updateListener.of((update) => {
      if (!update.docChanged) return;

      const { state } = update.view;
      const open = state.field(signatureTooltipField) !== null;
      // #3064: LSP-standard triggering — query only when a trigger
      // character was typed ("(" or ",") or the tooltip is already open
      // (to advance the active parameter or dismiss on leaving the
      // call). Plain typing with no tooltip never queries: the previous
      // unconditional probe ran a whole-document wasm query on EVERY
      // keystroke (228 calls per typing burst in the perf trace) for a
      // popup that standard editors don't show without a trigger.
      if (!open) {
        let triggered = false;
        update.changes.iterChanges((_fromA, _toA, _fromB, _toB, inserted) => {
          const text = inserted.toString();
          if (text.includes("(") || text.includes(",")) triggered = true;
        });
        if (!triggered) return;
      }

      const pos = state.selection.main.head;
      const source = state.doc.toString();
      const seq = ++querySeq;
      const doc = state.doc;
      const view = update.view;

      const land = (info: SignatureInfo | null): void => {
        if (seq !== querySeq) return; // a newer query landed or is landing
        if (!view.dom.isConnected) return;
        if (view.state.doc !== doc) return; // stale — the change re-queries
        if (!info) {
          if (view.state.field(signatureTooltipField) !== null) {
            view.dispatch({ effects: setSignatureTooltip.of(null) });
          }
          return;
        }
        view.dispatch({ effects: setSignatureTooltip.of(buildTooltip(info, pos)) });
      };

      let produced: SignatureInfo | null | Promise<SignatureInfo | null>;
      try {
        produced = options.getSignatureHelp(source, pos);
      } catch {
        produced = null;
      }
      if (produced instanceof Promise) {
        void produced.then(land, () => land(null));
        return;
      }
      land(produced);
    }),
  ];
}

function buildTooltip(info: SignatureInfo, pos: number): Tooltip {
  const tooltip: Tooltip = {
        pos,
        above: true,
        create() {
          const dom = document.createElement("div");
          dom.className = "brink-signature-help";

          const label = document.createElement("div");
          label.className = "brink-sig-label";

          const sigText = info.label;
          const params = info.parameters;
          const activeIdx = info.active_parameter;

          if (params.length > 0) {
            let remaining = sigText;
            for (let i = 0; i < params.length; i++) {
              const paramLabel = params[i].label;
              const idx = remaining.indexOf(paramLabel);
              if (idx >= 0) {
                if (idx > 0) {
                  label.appendChild(document.createTextNode(remaining.slice(0, idx)));
                }
                const span = document.createElement("span");
                span.textContent = paramLabel;
                if (i === activeIdx) {
                  span.className = "brink-sig-active-param";
                }
                label.appendChild(span);
                remaining = remaining.slice(idx + paramLabel.length);
              }
            }
            if (remaining) {
              label.appendChild(document.createTextNode(remaining));
            }
          } else {
            label.textContent = sigText;
          }

          dom.appendChild(label);

          if (info.documentation) {
            const doc = document.createElement("div");
            doc.className = "brink-sig-doc";
            doc.textContent = info.documentation;
            dom.appendChild(doc);
          }

          return { dom };
        },
      };
  return tooltip;
}
