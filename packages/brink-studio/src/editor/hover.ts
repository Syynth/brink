import { type Extension } from "@codemirror/state";
import { hoverTooltip, type Tooltip } from "@codemirror/view";
import type { HoverInfo } from "../wasm.js";

export interface HoverOptions {
  getHover: (source: string, offset: number) => HoverInfo | null;
}

export function hoverExtension(options: HoverOptions): Extension {
  return hoverTooltip((view, pos): Tooltip | null => {
    const source = view.state.doc.toString();

    let info: HoverInfo | null;
    try {
      info = options.getHover(source, pos);
    } catch {
      return null;
    }

    if (!info) return null;

    return {
      pos: info.start ?? pos,
      end: info.end ?? pos,
      above: true,
      create() {
        const dom = document.createElement("div");
        dom.className = "brink-hover-tooltip";

        // Render content as simple text with line breaks
        const lines = info!.content.split("\n");
        for (const line of lines) {
          const p = document.createElement("div");
          if (line.startsWith("```")) {
            // Skip code fence markers
            continue;
          }
          if (line.startsWith("**") && line.endsWith("**")) {
            const strong = document.createElement("strong");
            strong.textContent = line.slice(2, -2);
            p.appendChild(strong);
          } else {
            // Render inline code
            const parts = line.split(/`([^`]+)`/);
            for (let i = 0; i < parts.length; i++) {
              if (i % 2 === 1) {
                const code = document.createElement("code");
                code.textContent = parts[i];
                p.appendChild(code);
              } else if (parts[i]) {
                p.appendChild(document.createTextNode(parts[i]));
              }
            }
          }
          dom.appendChild(p);
        }

        return { dom };
      },
    };
  });
}
