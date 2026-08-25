import { type Extension, StateEffect, StateField } from "@codemirror/state";
import { EditorView, showTooltip, type Tooltip } from "@codemirror/view";
import type { SignatureInfo } from "@brink/wasm-types";

export interface SignatureHelpOptions {
  getSignatureHelp: (source: string, offset: number) => SignatureInfo | null;
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

      let info: SignatureInfo | null;
      try {
        info = options.getSignatureHelp(source, pos);
      } catch {
        info = null;
      }

      if (!info) {
        if (state.field(signatureTooltipField) !== null) {
          update.view.dispatch({ effects: setSignatureTooltip.of(null) });
        }
        return;
      }

      const tooltip: Tooltip = {
        pos,
        above: true,
        create() {
          const dom = document.createElement("div");
          dom.className = "brink-signature-help";

          const label = document.createElement("div");
          label.className = "brink-sig-label";

          const sigText = info!.label;
          const params = info!.parameters;
          const activeIdx = info!.active_parameter;

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

          if (info!.documentation) {
            const doc = document.createElement("div");
            doc.className = "brink-sig-doc";
            doc.textContent = info!.documentation;
            dom.appendChild(doc);
          }

          return { dom };
        },
      };

      update.view.dispatch({ effects: setSignatureTooltip.of(tooltip) });
    }),
  ];
}
