import { type Extension } from "@codemirror/state";
import { hoverTooltip, EditorView, type Tooltip } from "@codemirror/view";
import type { HoverInfo, CallWidgetSite } from "@brink/wasm-types";
import { openCallForm, FORM_GLYPH_ICON } from "./argument-widgets.js";

export interface HoverOptions {
  getHover: (source: string, offset: number) => HoverInfo | null;
  /** When provided, the hover card over a call name gains an always-on "edit
   *  arguments" action (zero in-text chrome — independent of the inline glyph). */
  getArgumentWidgets?: (source: string, start: number, end: number) => CallWidgetSite[];
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

    // Surface the form action when `pos` is over a call name (always available).
    const site = options.getArgumentWidgets
      ? siteAt(options.getArgumentWidgets, source, pos)
      : null;

    return {
      pos: info.start ?? pos,
      end: info.end ?? pos,
      above: true,
      create() {
        const dom = document.createElement("div");
        dom.className = "brink-hover-tooltip";

        // Render content line by line with inline markdown spans.
        const lines = info!.content.split("\n");
        for (const line of lines) {
          if (line.startsWith("```") || line.trim() === "") continue;
          const p = document.createElement("div");
          renderInline(line, p);
          dom.appendChild(p);
        }

        if (site) dom.appendChild(buildEditAction(site, view));
        return { dom };
      },
    };
  });
}

/** The call whose name span contains `pos`, if any (for the hover-card action). */
function siteAt(
  getArgumentWidgets: (source: string, start: number, end: number) => CallWidgetSite[],
  source: string,
  pos: number,
): CallWidgetSite | null {
  let sites: CallWidgetSite[];
  try {
    sites = getArgumentWidgets(source, 0, source.length);
  } catch {
    return null;
  }
  return (
    sites.find(
      (s) => s.slots.length > 0 && pos >= s.name_start && pos <= s.name_end,
    ) ?? null
  );
}

/** The "edit arguments" button folded into the hover card. */
function buildEditAction(site: CallWidgetSite, view: EditorView): HTMLElement {
  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = "brink-hover-action";
  btn.innerHTML = `${FORM_GLYPH_ICON}<span>Edit arguments</span>`;
  btn.addEventListener("click", () => openCallForm(btn, site, view));
  return btn;
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
