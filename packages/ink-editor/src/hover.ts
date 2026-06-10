import { type Extension } from "@codemirror/state";
import { hoverTooltip, type Tooltip } from "@codemirror/view";
import type { HoverInfo } from "@brink/wasm-types";

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

        // Render content line by line with inline markdown spans
        const lines = info!.content.split("\n");
        for (const line of lines) {
          if (line.startsWith("```")) {
            // Skip code fence markers
            continue;
          }
          const p = document.createElement("div");
          renderInline(line, p);
          dom.appendChild(p);
        }

        return { dom };
      },
    };
  });
}

/**
 * Render a line of hover markdown (inline `code`, **bold**, *italic*) into
 * `into`. Bold/italic contents are rendered recursively, so one level of
 * nesting like *Defined in `path`* works.
 */
function renderInline(line: string, into: HTMLElement): void {
  const re = /(`[^`]+`)|(\*\*[^*]+\*\*)|(\*[^*]+\*)/g;
  let last = 0;
  for (const m of line.matchAll(re)) {
    const idx = m.index ?? 0;
    if (idx > last) {
      into.appendChild(document.createTextNode(line.slice(last, idx)));
    }
    const tok = m[0];
    if (tok.startsWith("`")) {
      const code = document.createElement("code");
      code.textContent = tok.slice(1, -1);
      into.appendChild(code);
    } else if (tok.startsWith("**")) {
      const strong = document.createElement("strong");
      renderInline(tok.slice(2, -2), strong);
      into.appendChild(strong);
    } else {
      const em = document.createElement("em");
      renderInline(tok.slice(1, -1), em);
      into.appendChild(em);
    }
    last = idx + tok.length;
  }
  if (last < line.length) {
    into.appendChild(document.createTextNode(line.slice(last)));
  }
}
