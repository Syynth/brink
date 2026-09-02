/**
 * Headless-ready editor (#363):
 *  - theme opt-out — `brinkStudio({ theme: false })` omits `brinkTheme`;
 *    the default keeps it; a custom Extension substitutes it;
 *  - inline-style sweep — editor-owned popups/widgets carry no presentational
 *    inline styles; dynamic values ride on CSS custom properties consumed by
 *    the injected zero-specificity structural stylesheet.
 */

import { describe, it, expect, afterEach } from "vitest";
import { EditorState } from "@codemirror/state";
import type { Extension } from "@codemirror/state";
import { EditorView, runScopeHandlers } from "@codemirror/view";
import type { CodeAction, InlayHint } from "@brink/wasm-types";
import { AT_CUE_DIALECT, brinkStudio, brinkTheme, ensureStructuralStyles } from "@brink-lang/editor";
import { colorWidget } from "../../../ink-editor/src/color-widget.js";
import { mountColorPicker } from "../../../ink-editor/src/color-picker-ui.js";

const STYLE_ID = "brink-editor-structural-styles";

const minimal = {
  // The at-cue preset, opted into (no dialect by default, RULED 2026-08-30).
  dialect: AT_CUE_DIALECT,
  compile: () => ({ ok: true, diagnostics: [] }) as never,
  getSemanticTokens: () => [],
  getTokenTypeNames: () => [],
};

/** Depth-first identity search through a (nested-array) Extension tree. */
function containsExtension(tree: Extension, needle: Extension): boolean {
  if (tree === needle) return true;
  if (Array.isArray(tree)) return tree.some((child) => containsExtension(child, needle));
  return false;
}

afterEach(() => {
  document.body.replaceChildren();
});

// ── Theme opt-out ───────────────────────────────────────────────────

describe("theme opt-out", () => {
  it("includes brinkTheme by default", () => {
    expect(containsExtension(brinkStudio(minimal), brinkTheme)).toBe(true);
  });

  it("theme: false yields a headless (theme-less) bundle", () => {
    expect(containsExtension(brinkStudio({ ...minimal, theme: false }), brinkTheme)).toBe(false);
  });

  it("theme: <Extension> substitutes the host's theme for brinkTheme", () => {
    const custom = EditorView.theme({});
    const ext = brinkStudio({ ...minimal, theme: custom });
    expect(containsExtension(ext, custom)).toBe(true);
    expect(containsExtension(ext, brinkTheme)).toBe(false);
  });

  it("a headless bundle still mounts and renders", () => {
    const view = new EditorView({
      state: EditorState.create({
        doc: "Hello world\n",
        extensions: [brinkStudio({ ...minimal, theme: false })],
      }),
      parent: document.body,
    });
    expect(view.contentDOM.textContent).toContain("Hello world");
    view.destroy();
  });
});

// ── Structural stylesheet ───────────────────────────────────────────

describe("structural stylesheet", () => {
  it("injects once, idempotently, with zero-specificity selectors", () => {
    ensureStructuralStyles();
    ensureStructuralStyles();
    const styles = document.querySelectorAll(`#${STYLE_ID}`);
    expect(styles).toHaveLength(1);
    const css = styles[0].textContent ?? "";
    // Load-bearing placement is class-based (not inline) and overridable.
    expect(css).toContain(":where(.brink-code-actions-menu)");
    expect(css).toContain(":where(.brink-widget-popover)");
    expect(css).toContain(":where(.brink-inline-picker)");
    expect(css).toContain("var(--brink-popup-left");
    expect(css).toContain(":where(.brink-color-swatch)");
    expect(css).toContain("var(--brink-swatch-color");
  });
});

// ── Inline-style sweep ──────────────────────────────────────────────

describe("inline-style sweep", () => {
  it("code-actions menu carries no inline position styles", () => {
    const action: CodeAction = { title: "Do the thing", kind: "refactor", data: { action: "noop" } };
    const view = new EditorView({
      state: EditorState.create({
        doc: "content\n",
        extensions: [brinkStudio({ ...minimal, getCodeActions: () => [action] })],
      }),
      parent: document.body,
    });
    const handled = runScopeHandlers(
      view,
      new KeyboardEvent("keydown", { key: ".", ctrlKey: true }),
      "editor",
    );
    expect(handled).toBe(true);
    const menu = document.querySelector<HTMLElement>(".brink-code-actions-menu");
    expect(menu).not.toBeNull();
    // Placement comes from the structural stylesheet + custom properties.
    expect(menu?.style.position).toBe("");
    expect(menu?.style.left).toBe("");
    expect(menu?.style.top).toBe("");
    expect(document.getElementById(STYLE_ID)).not.toBeNull();
    view.destroy();
  });

  it("inlay hints request padding via the -pad class, not an inline margin", () => {
    const hints: InlayHint[] = [
      { offset: 0, label: "lead:", kind: "parameter", padding_right: true },
      { offset: 8, label: "tail:", kind: "parameter", padding_right: false },
    ];
    const view = new EditorView({
      state: EditorState.create({
        doc: "content here\n",
        extensions: [brinkStudio({ ...minimal, getInlayHints: () => hints })],
      }),
      parent: document.body,
    });
    const rendered = [...view.dom.querySelectorAll<HTMLElement>(".brink-inlay-hint")];
    expect(rendered).toHaveLength(2);
    const [padded, plain] = rendered;
    expect(padded.classList.contains("brink-inlay-hint-pad")).toBe(true);
    expect(padded.style.marginRight).toBe("");
    expect(plain.classList.contains("brink-inlay-hint-pad")).toBe(false);
    view.destroy();
  });

  it("color swatch carries its color as a custom property, not a background", () => {
    const swatch = colorWidget.renderInline("#ff0000");
    expect(swatch.className).toBe("brink-color-swatch");
    expect(swatch.style.getPropertyValue("--brink-swatch-color")).toBe("#FF0000");
    expect(swatch.style.background).toBe("");
  });

  it("color picker drives sv/thumb/presets through custom properties", () => {
    const host = document.createElement("div");
    document.body.appendChild(host);
    const picker = mountColorPicker(host, "#00FF00", () => {});
    const sv = host.querySelector<HTMLElement>(".brink-cp-sv");
    const thumb = host.querySelector<HTMLElement>(".brink-cp-sv-thumb");
    const preset = host.querySelector<HTMLElement>(".brink-cp-preset");
    expect(sv?.style.getPropertyValue("--brink-cp-hue")).toBe("120");
    expect(sv?.style.background).toBe("");
    expect(thumb?.style.getPropertyValue("--brink-cp-x")).toBe("100%");
    expect(thumb?.style.getPropertyValue("--brink-cp-color")).toBe("#00FF00");
    expect(thumb?.style.left).toBe("");
    expect(preset?.style.getPropertyValue("--brink-cp-color")).not.toBe("");
    expect(preset?.style.background).toBe("");
    picker.destroy();
  });
});

// ── Replaced-range widget audit (#427) ───────────────────────────────
//
// #421's regression test (structural-decoration-attrs.test.ts) proved
// `.cm-line` never carries a `style` attribute under `theme: false` — but
// that only covers LINE decorations. `screenplay.ts` also has a
// replaced-RANGE widget: the sigil-hiding widget used for dialect hidden
// geometry — the `@`/`:<>` cue affixes and `(`/`)<>` parenthetical glue
// (rendered as `.brink-hidden-sigil`). It carries only a `className` (no
// `.style.*` writes in `toDOM`) — this audit proves it holds under both
// the default theme and headless (`theme: false`), guarding the #363 "no
// inline styles" contract the way #421 does for line decorations.
// (`DepthSigilWidget`, the second widget this audit used to sweep, was
// REMOVED by the 2026-08-23 "literal whitespace" ruling — nested sigil
// runs now render as typed; the companion test below pins that removal.)
describe("replaced-range widgets (#427)", () => {
  const mount = (doc: string, theme: false | undefined = undefined): EditorView =>
    new EditorView({
      state: EditorState.create({
        doc,
        extensions: [brinkStudio(theme === false ? { ...minimal, theme: false } : minimal)],
      }),
      parent: document.body,
    });

  it("nested sigil runs render as typed — no depth-sigil widget (ruled 2026-08-23)", () => {
    const view = mount("* Option A\n    * * Nested A1\n        A1 body.\n");
    // The literal `* *` survives into the DOM; nothing replaces it.
    expect(view.dom.querySelector(".brink-depth-sigil")).toBeNull();
    expect(view.dom.textContent).toContain("* * Nested A1");
    // The machine-readable depth contract is untouched.
    const nested = [...view.dom.querySelectorAll<HTMLElement>(".cm-line")].find((l) =>
      l.textContent?.includes("Nested A1"),
    );
    expect(nested?.getAttribute("data-depth")).toBe("2");
    view.destroy();
  });

  it.each([
    ["default theme", undefined],
    ["headless (theme: false)", false],
  ] as const)(
    "hidden-sigil widget (character/parenthetical dialect glyphs) carries no inline style — %s",
    (_label, theme) => {
      const view = mount("@Alice:<>\n(quietly)<>\nHello there.\n", theme);
      const sigils = [...view.dom.querySelectorAll<HTMLElement>(".brink-hidden-sigil")];
      expect(sigils.length).toBeGreaterThan(0);
      for (const sigil of sigils) {
        expect(sigil.hasAttribute("style")).toBe(false);
      }
      view.destroy();
    },
  );
});
