import { type Extension } from "@codemirror/state";
import { hoverTooltip, EditorView, type Tooltip } from "@codemirror/view";
import type { HoverInfo, CallWidgetSite, Location } from "@brink/wasm-types";
import { openCallForm, FORM_GLYPH_ICON } from "./argument-widgets.js";

export interface HoverOptions {
  /** Sync or async (W2c of `docs/editor-worker-spec.md`). CM6's
   *  `hoverTooltip` natively awaits a promise-returning source and drops
   *  the result if the pointer moved on, so no extra staleness handling
   *  is needed here. */
  getHover: (source: string, offset: number) => HoverInfo | null | Promise<HoverInfo | null>;
  /** When provided, the hover card over a call name gains an always-on "edit
   *  arguments" action (zero in-text chrome — independent of the inline glyph). */
  getArgumentWidgets?: (source: string, start: number, end: number) => CallWidgetSite[];
  /**
   * Navigate to a hover card's link target (#3255, decision 5).
   *
   * Absent means references render as plain code rather than as links —
   * the same rule "Add to dictionary" follows: an embedder that cannot
   * perform the action is not offered a control that silently does
   * nothing.
   */
  onNavigate?: (target: Location) => void;
}

export function hoverExtension(options: HoverOptions): Extension {
  return hoverTooltip((view, pos): Tooltip | null | Promise<Tooltip | null> => {
    // No hover cards while an inline rename/name-input row is open: the row
    // owns the space under the line, and a card that flips below (viewport
    // top) lands exactly on the "⚠ breaks N" badge and intercepts its
    // clicks — symbol-rename e2e caught it the moment the #3060-era row
    // moved the badge beneath the token. DOM probe rather than state: the
    // rename StateField is closure-local to renameExtension, and the row's
    // input existing IS the active condition.
    if (view.dom.querySelector(".brink-inline-rename-input")) return null;
    const source = view.state.doc.toString();

    const finish = (info: HoverInfo | null): Tooltip | null => {
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
          const dom = renderHoverContent(info, options.onNavigate);
          if (site) dom.appendChild(buildEditAction(site, view));
          return { dom };
        },
      };
    };

    let produced: HoverInfo | null | Promise<HoverInfo | null>;
    try {
      produced = options.getHover(source, pos);
    } catch {
      return null;
    }
    if (produced instanceof Promise) {
      // A rejected pull (superseded/teardown) reads as "no hover".
      return produced.then(finish, () => null);
    }
    return finish(produced);
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
/**
 * Build a hover card's body from its markdown content and link targets.
 *
 * Extracted from the tooltip's `create()` so the rendering — which is where
 * the link indices are resolved, and where a target that goes nowhere has
 * to degrade to plain text — is reachable from a test without driving
 * CodeMirror's hover machinery.
 */
export function renderHoverContent(
  info: HoverInfo,
  onNavigate?: (target: Location) => void,
): HTMLElement {
  const dom = document.createElement("div");
  dom.className = "brink-hover-tooltip";

  const navigate =
    onNavigate === undefined
      ? undefined
      : (index: number) => {
          const target = info.links?.[index];
          // An empty `file` is a target the compiler could not resolve; it
          // renders as plain text and never reaches here.
          if (target === undefined || target.file === "") return;
          onNavigate(target);
        };

  for (const line of info.content.split("\n")) {
    if (line.startsWith("```") || line.trim() === "") continue;
    const p = document.createElement("div");
    renderInline(line, p, { links: info.links, navigate });
    dom.appendChild(p);
  }
  return dom;
}

interface InlineCtx {
  links?: readonly Location[];
  navigate?: (index: number) => void;
}

/**
 * Render one line of the hover card's restricted markdown.
 *
 * Supports `` `code` ``, `**strong**`, `*em*` and `[text](#N)` links, where
 * `N` indexes the card's link targets. The link form is matched FIRST in the
 * alternation so a link's own `` `code` `` label is not consumed as a bare
 * code span before the link is seen.
 */
function renderInline(line: string, into: HTMLElement, ctx: InlineCtx = {}): void {
  const re = /(\[[^\]]*\]\(#\d+\))|(`[^`]+`)|(\*\*[^*]+\*\*)|(\*[^*]+\*)/g;
  let last = 0;
  for (const m of line.matchAll(re)) {
    const idx = m.index ?? 0;
    if (idx > last) {
      into.appendChild(document.createTextNode(line.slice(last, idx)));
    }
    const tok = m[0];
    if (tok.startsWith("[")) {
      renderLink(tok, into, ctx);
    } else if (tok.startsWith("`")) {
      const code = document.createElement("code");
      code.textContent = tok.slice(1, -1);
      into.appendChild(code);
    } else if (tok.startsWith("**")) {
      const strong = document.createElement("strong");
      renderInline(tok.slice(2, -2), strong, ctx);
      into.appendChild(strong);
    } else {
      const em = document.createElement("em");
      renderInline(tok.slice(1, -1), em, ctx);
      into.appendChild(em);
    }
    last = idx + tok.length;
  }
  if (last < line.length) {
    into.appendChild(document.createTextNode(line.slice(last)));
  }
}

/**
 * `[text](#N)` — a reference into the card's link targets.
 *
 * Falls back to rendering the label as ordinary markdown when there is
 * nowhere to go: no navigate hook, no target at that index, or a target the
 * compiler could not resolve to a project file. A link that navigates
 * nowhere is worse than plain text.
 */
function renderLink(token: string, into: HTMLElement, ctx: InlineCtx): void {
  const split = token.indexOf("](#");
  const label = token.slice(1, split);
  const index = Number(token.slice(split + 3, -1));
  const target = ctx.links?.[index];

  if (ctx.navigate === undefined || target === undefined || target.file === "") {
    renderInline(label, into, ctx);
    return;
  }

  const a = document.createElement("a");
  a.className = "brink-hover-link";
  a.setAttribute("role", "link");
  a.tabIndex = 0;
  a.title = `Go to ${target.file}`;
  renderInline(label, a, ctx);
  const go = (event: Event) => {
    event.preventDefault();
    event.stopPropagation();
    ctx.navigate?.(index);
  };
  a.addEventListener("click", go);
  a.addEventListener("keydown", (event) => {
    if (event.key === "Enter" || event.key === " ") go(event);
  });
  into.appendChild(a);
}
