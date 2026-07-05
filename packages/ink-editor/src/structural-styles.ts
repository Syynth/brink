/**
 * Structural (non-skin) styles for editor-owned floating surfaces and
 * data-driven widget appearance (#363 headless-ready).
 *
 * The editor keeps NO presentational inline styles on its DOM — everything is
 * addressed by class so an embedding host can restyle the taxonomy directly
 * (see docs/editor-consumer-guide.md, "The class taxonomy"). Values that are
 * *data* (popup coordinates, the color a swatch shows) are carried on CSS
 * custom properties; the rules below consume them.
 *
 * These rules are load-bearing — a popup must be `position: fixed` to work at
 * all — so they are injected once per document, independent of the opt-in
 * `brinkTheme`. Every selector is wrapped in `:where(...)` (zero specificity),
 * so ANY host rule overrides them without specificity games.
 */

const STYLE_ID = "brink-editor-structural-styles";

const RULES = `
:where(.brink-code-actions-menu) { position: fixed; left: var(--brink-popup-left, 0px); top: var(--brink-popup-top, 0px); }
:where(.brink-inline-picker) { position: fixed; left: var(--brink-popup-left, 0px); top: var(--brink-popup-top, 0px); }
:where(.brink-widget-popover) { position: fixed; left: var(--brink-popup-left, 0px); top: var(--brink-popup-top, 0px); }
:where(.brink-form-anchor) { position: fixed; left: var(--brink-popup-left, 0px); top: var(--brink-popup-top, 0px); width: 1px; height: var(--brink-anchor-height, 1px); }
:where(.brink-inlay-hint-pad) { margin-right: 4px; }
:where(.brink-color-swatch) { background: var(--brink-swatch-color, transparent); }
:where(.brink-cp-preset) { background: var(--brink-cp-color, transparent); }
:where(.brink-cp-sv) { background: linear-gradient(to top, #000, rgba(0, 0, 0, 0)), linear-gradient(to right, #fff, hsl(var(--brink-cp-hue, 0) 100% 50%)), #fff; }
:where(.brink-cp-sv-thumb) { left: var(--brink-cp-x, 0%); top: var(--brink-cp-y, 0%); background: var(--brink-cp-color, #000); }
`;

/**
 * Inject the structural stylesheet into `doc` (default: the global document),
 * once — repeat calls are no-ops. Called automatically by every editor surface
 * that needs it; exported for hosts that mount editor popups into another
 * document (e.g. an iframe).
 */
export function ensureStructuralStyles(doc: Document = document): void {
  if (doc.getElementById(STYLE_ID)) return;
  const style = doc.createElement("style");
  style.id = STYLE_ID;
  style.textContent = RULES;
  doc.head.appendChild(style);
}
