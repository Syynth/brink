/**
 * The Problems panel's auto-fix surfaces (#3420, `docs/autofix-spec.md` §7).
 *
 * Three properties, each of which fails silently if it drifts:
 *
 * 1. **A row shows a Fix button only when the diagnostic HAS a fix**, and
 *    the button names the tier. A permanently-present button, or one whose
 *    label does not distinguish `safe` from `suggested`, is worse than none:
 *    the whole point of §3's tiers is that the author knows how far a click
 *    goes before making it.
 * 2. **The header's `N` is the batch's own count**, not a tally of the
 *    offered fixes. `countFixes` applies the policy's batching gate and
 *    collapses identical fixes; a header counting `getFixOffers` entries
 *    would promise more than pressing it delivers.
 * 3. **Clicking routes through the file the DIAGNOSTIC names**, via
 *    `applyFixInFile`, and lands in the undoable apply seam. A Problems row
 *    can name a file that is not open at all, so an `applyFix` keyed to the
 *    active file would rewrite the wrong buffer.
 *
 * The wasm mock models no diagnostics (it has no analyzer), so the session
 * is stubbed here; the real fix road is pinned in Rust over a real
 * `EditorSession` (`crates/brink-web/src/editor/fix_batch.rs`).
 */
import { describe, it, expect, afterEach, vi } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import {
  CommandRegistry,
  KeymapOverridesService,
  ShellProvider,
  ThemeService,
} from "@brink/studio-shell";
import {
  ProblemsActions,
  ProblemsContextMenu,
  ProblemsView,
  StoreProvider,
} from "@brink/studio-ui";
import { createStudioStore, type StudioStore } from "@brink/studio-store";
import type {
  Diagnostic,
  FixOffer,
  FixSelect,
  FileOutline,
  StructuralResult,
} from "@brink/wasm-types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const OUTLINE: FileOutline[] = [{ path: "main.brink", symbols: [], mounted: false }];

const SOURCE = "flow start() {\n  -> haggle\n}\n";

/** The `E025` import-required diagnostic — the shape `ImportFixer` fixes. */
const IMPORT_REQUIRED: Diagnostic = {
  start: 20,
  end: 26,
  message: "`haggle` is not in scope",
  severity: "Error",
  code: "E025",
  file: "main.brink",
};

/** A diagnostic with no fixer — the row that must show no button. */
const NO_FIXER: Diagnostic = {
  start: 4,
  end: 9,
  message: "unreachable code after divert",
  severity: "Warning",
  code: "E033",
  file: "main.brink",
};

/** A Warning-severity diagnostic that BOTH has a fix and is suppressible —
 *  the shape that proves the fix entries land beside the suppress items
 *  rather than replacing them. */
const TRIM_WARNING: Diagnostic = {
  start: 30,
  end: 34,
  message: "`~` on its own line does nothing",
  severity: "Warning",
  code: "E014",
  file: "main.brink",
};

/** The compiler's severity registry, which the suppress items gate on: an
 *  Error-tier code is refused by every suppression channel, so a menu for
 *  one shows no suppress items at all. */
const REGISTRY = [
  { code: "E025", default_severity: "error" },
  { code: "E033", default_severity: "warning" },
  { code: "E014", default_severity: "warning" },
];

const IMPORT_OFFER: FixOffer = {
  code: "E025",
  path: "main.brink",
  start: IMPORT_REQUIRED.start,
  end: IMPORT_REQUIRED.end,
  batchable: false,
  fix: {
    code: "E025",
    title: "Import `haggle` from `story::market::barter`",
    applicability: "suggested",
    edits: [
      {
        path: "main.brink",
        start: 0,
        end: 0,
        new_text: "use story::market::barter::haggle;\n",
      },
    ],
  },
};

const TRIM_OFFER: FixOffer = {
  code: "E014",
  path: "main.brink",
  start: TRIM_WARNING.start,
  end: TRIM_WARNING.end,
  batchable: true,
  fix: {
    code: "E014",
    title: "Delete the empty `~` line",
    applicability: "safe",
    edits: [{ path: "main.brink", start: 30, end: 34, new_text: "" }],
  },
};

interface Harness {
  store: StudioStore;
  applied: { path: string; title: string }[];
  /** What reached the project's write seam, via `applyMoveResult`. */
  written: [string, string][];
  selects: FixSelect[];
}

function harness(
  diagnostics: Diagnostic[],
  offers: FixOffer[],
  opts: { safeCount?: number } = {},
): Harness {
  const applied: { path: string; title: string }[] = [];
  const written: [string, string][] = [];
  const selects: FixSelect[] = [];
  const session = {
    getFileSource: (p: string) => (p === "main.brink" ? SOURCE : null),
    getFixOffers: (select: FixSelect) => {
      selects.push(select);
      return offers;
    },
    countFixes: (select: FixSelect) => {
      selects.push(select);
      return opts.safeCount ?? 0;
    },
    applyFixInFile: (path: string, fix: { title: string }): StructuralResult => {
      applied.push({ path, title: fix.title });
      return {
        ok: true,
        path,
        new_source: `${IMPORT_OFFER.fix.edits[0]!.new_text}${SOURCE}`,
        cross_file_edits: [],
        introduced_diagnostics: [],
        safe: true,
      };
    },
  };
  const store = createStudioStore();
  store.getState().setCompileResult(OUTLINE, { errors: 1, warnings: 0 }, diagnostics, null);
  store.setState({
    _project: {
      getSession: () => session,
      getDiagnosticRegistry: () => REGISTRY,
      // `applyMoveResult`'s write seam — the road a chosen fix travels.
      applyEdit: (p: string, next: string) => {
        written.push([p, next]);
        return true;
      },
    } as never,
    _documents: {
      refreshExternal: vi.fn(),
      triggerCompile: vi.fn(),
      invalidateFile: vi.fn(),
      // #3496: `applyMoveResult` calls this instead of `invalidateFile` for
      // a touched path it has a precise edit list for (a single Fix's own
      // `edits`, as here).
      applyEditsToViews: vi.fn(),
    } as never,
  });
  return { store, applied, written, selects };
}

let root: Root | null = null;
let container: HTMLDivElement | null = null;

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  root = null;
  container = null;
});

function mount(store: StudioStore, element: React.ReactElement) {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  const commands = new CommandRegistry();
  act(() => {
    root!.render(
      createElement(
        ShellProvider,
        {
          commands,
          themes: new ThemeService(),
          keymapOverrides: new KeymapOverridesService(),
          isMac: true,
        } as never,
        createElement(StoreProvider, { store, children: element }),
      ),
    );
  });
  return commands;
}

function fixButtons(): HTMLButtonElement[] {
  return [...container!.querySelectorAll<HTMLButtonElement>("button.problems-fix")];
}

describe("the Problems row Fix button", () => {
  it("appears only on the row whose diagnostic has a fix, and names the tier", () => {
    const { store } = harness([IMPORT_REQUIRED, NO_FIXER], [IMPORT_OFFER]);
    mount(store, createElement(ProblemsView));

    const buttons = fixButtons();
    expect(buttons).toHaveLength(1);
    expect(buttons[0]!.dataset.tier).toBe("suggested");
    expect(buttons[0]!.textContent).toContain("Suggested");
    expect(buttons[0]!.title).toBe(
      "Import `haggle` from `story::market::barter` (suggested)",
    );

    // Two rows rendered — the button is per-row, not per-panel.
    expect(container!.querySelectorAll(".problems-row")).toHaveLength(2);
  });

  it("shows no button at all when nothing is offered", () => {
    const { store } = harness([IMPORT_REQUIRED, NO_FIXER], []);
    mount(store, createElement(ProblemsView));
    expect(fixButtons()).toHaveLength(0);
  });

  it("applies through the DIAGNOSTIC's file, not the active one", () => {
    const { store, applied } = harness([IMPORT_REQUIRED], [IMPORT_OFFER]);
    mount(store, createElement(ProblemsView));
    act(() => {
      fixButtons()[0]!.click();
    });
    expect(applied).toEqual([
      { path: "main.brink", title: "Import `haggle` from `story::market::barter`" },
    ]);
  });

  it("pulls the offers ONCE for the whole compilation, not once per row", () => {
    // Six rows, one pull: a per-row query is one wasm call per visible
    // diagnostic on every render, which is what the index exists to avoid.
    const many = Array.from({ length: 6 }, (_, i) => ({
      ...NO_FIXER,
      start: i * 10,
      end: i * 10 + 4,
    }));
    const { store, selects } = harness(many, []);
    mount(store, createElement(ProblemsView));
    expect(container!.querySelectorAll(".problems-row")).toHaveLength(6);
    expect(selects).toHaveLength(1);
  });
});

describe("the Fix all safe header button", () => {
  it("shows the batch's own count and runs the safe selection", () => {
    const { store, selects } = harness([IMPORT_REQUIRED], [IMPORT_OFFER], { safeCount: 3 });
    mount(store, createElement(ProblemsActions));
    const button = container!.querySelector<HTMLButtonElement>("button.problems-fix-all");
    expect(button, "the header must carry the batch button").not.toBeNull();
    expect(button!.textContent).toBe("Fix all safe (3)");
    // The count came from `countFixes` with the safe selection — NOT from
    // the offers list, which carries one non-batchable entry.
    expect(selects).toContainEqual({ tiers: ["safe"] });
  });

  it("is absent when the batch would take nothing", () => {
    const { store } = harness([IMPORT_REQUIRED], [IMPORT_OFFER], { safeCount: 0 });
    mount(store, createElement(ProblemsActions));
    expect(container!.querySelector("button.problems-fix-all")).toBeNull();
  });
});

describe("the Problems row context menu", () => {
  it("lists the offered fix, tier-labelled, ABOVE the suppress items", () => {
    const { store, applied, written } = harness([TRIM_WARNING], [TRIM_OFFER]);
    mount(
      store,
      createElement(ProblemsContextMenu, {
        target: { x: 0, y: 0, diagnostic: TRIM_WARNING },
        onClose: () => {},
      }),
    );
    const labels = [...container!.querySelectorAll(".brink-context-menu-item")].map(
      (el) => el.textContent ?? "",
    );
    expect(labels[0]).toBe("Delete the empty `~` line — Safe");
    // The suppress items are still there — the fix entries are added
    // BESIDE them (§7), never instead of them.
    expect(labels.some((l) => l.startsWith("Suppress E014"))).toBe(true);

    act(() => {
      (container!.querySelector(".brink-context-menu-item") as HTMLElement).click();
    });
    expect(applied).toEqual([{ path: "main.brink", title: "Delete the empty `~` line" }]);
    // …and it reached the project's write seam, not just the wasm call.
    expect(written.map(([p]) => p)).toEqual(["main.brink"]);
  });

  it("lists no fix entry for a diagnostic nothing fixes", () => {
    const { store } = harness([NO_FIXER], []);
    mount(
      store,
      createElement(ProblemsContextMenu, {
        target: { x: 0, y: 0, diagnostic: NO_FIXER },
        onClose: () => {},
      }),
    );
    const labels = [...container!.querySelectorAll(".brink-context-menu-item")].map(
      (el) => el.textContent ?? "",
    );
    expect(labels.every((l) => !l.includes("Delete the empty"))).toBe(true);
    expect(labels.some((l) => l.startsWith("Suppress E033"))).toBe(true);
  });

  it("still offers the fix for a code no suppression channel accepts", () => {
    // `E025` is Error-tier, so every suppress item is (correctly) withheld.
    // The fix must survive that gate — it is the ONLY thing this menu can
    // offer for such a row.
    const { store } = harness([IMPORT_REQUIRED], [IMPORT_OFFER]);
    mount(
      store,
      createElement(ProblemsContextMenu, {
        target: { x: 0, y: 0, diagnostic: IMPORT_REQUIRED },
        onClose: () => {},
      }),
    );
    const labels = [...container!.querySelectorAll(".brink-context-menu-item")].map(
      (el) => el.textContent ?? "",
    );
    expect(labels[0]).toBe(
      "Import `haggle` from `story::market::barter` — Suggested",
    );
    expect(labels.some((l) => l.startsWith("Suppress"))).toBe(false);
  });
});
