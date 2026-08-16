/**
 * Does a refused `deleteSymbol` reach the author at all? (#2636)
 *
 * ## The question, and the answer
 *
 * #2528 established that a refused RENAME reaches the author as an error
 * toast: `performSymbolRename` / `applyComputedRename`
 * (`packages/studio-ui/src/symbolMenuActions.ts`) hand `result.error`
 * verbatim to `notifyRenameRefusal`, which raises an error-severity,
 * `binder`-sourced store notification. PR #2627 corrected `delete_symbol`'s
 * refusal wording on the strength of that same user path.
 *
 * It does not carry over. `notifyRenameRefusal` is called from exactly two
 * places, both inside the rename surfaces; `dispatchSymbolAction`'s switch has
 * no delete branch at all. Tracing the whole stack top-down:
 *
 * | layer | delete | verdict |
 * |---|---|---|
 * | `brink_ide::structural_delete::delete_symbol` (`crates/internal/brink-ide/src/structural_delete.rs:44`) | present | the op is real |
 * | `EditorSession::delete_symbol` (`crates/brink-web/src/editor/refactor.rs:412`) | present | exposed to wasm |
 * | `EditorSessionHandle.deleteSymbol` (`packages/wasm/src/index.ts:994`) | present | exposed to TS |
 * | any caller of `.deleteSymbol(` | **absent** | zero in the repo |
 * | `ContextMenuAction` union (`packages/studio-ui/src/BinderContextMenu.tsx:49`) | **absent** | `deleteFile`/`deleteFolder` only |
 * | knot/stitch menu items (`buildItems`, same file) | **absent** | Play / Rename / Move / Promote / Demote |
 * | `dispatchSymbolAction` switch (`packages/studio-ui/src/symbolMenuActions.ts:71`) | **absent** | falls to `default`, returns |
 *
 * So the answer to #2636 is stronger than "Delete has no refusal toast":
 * **Delete has no invocation at all.** A refused `delete_symbol` reaches the
 * author through nothing, because a *successful* `delete_symbol` reaches the
 * author through nothing either — the op stops dead at the `ProjectSession`
 * API boundary and no studio surface calls it.
 *
 * ## What that means for #2627
 *
 * #2627's corrected `source knot not found` wording is text no author reads
 * today. That does not make #2627 wrong — the mock genuinely was answering a
 * string production never emits, and `structural-refusal-shape.test.ts` pins
 * mock↔production parity, an invariant that holds whether or not the op has a
 * UI. But #2627's stated *reachability* argument, which routed through
 * `symbolMenuActions.ts` → `notifyRenameRefusal`, is a rename-only path and
 * does not cover `delete_symbol`. What the fix bought was fixture honesty,
 * not a corrected message on a screen.
 *
 * ## What this file does, and deliberately does not do
 *
 * It PINS the state above rather than repairing it. Whether Delete should be
 * *offered* on a knot/stitch is a maintainer call with no ruling behind it —
 * inventing one here is the failure mode #2636 names. The refusal-reporting
 * question is *not* unruled the same way: `docs/studio-shell-spec.md` §7.5
 * says "A rename/move/**delete** that the underlying op declines raises an
 * `error`-severity notification" — delete is named, and `delete_symbol` is a
 * `StructuralResult` op, so that clause already covers it. §7.5 is a
 * CONTRADICTED spec here, not silence: it prescribes behavior for an op that
 * has no invocation to carry it out. There is nothing to apply the clause
 * *to* until Delete is wired up, which is the part that stays a maintainer
 * call.
 *
 * The tests are a tripwire. The moment anyone wires a symbol Delete into the
 * menu or the dispatcher, the assertions below turn red and the author has to
 * answer the notification question on the way past, instead of shipping a
 * second silently-refusing op. They are the same class of pin as
 * `symbol-structural-ops.test.ts`'s refusal cases (#2544): observed behavior,
 * recorded, not asserted as correct.
 *
 * The last test is the control. It runs a refused RENAME through the same
 * store and the same notifier capture, so "delete raises nothing" is a real
 * absence in a harness that demonstrably reports refusals — not a harness that
 * cannot see notifications at all.
 */

import { describe, it, expect, afterEach, vi } from "vitest";
import { act, createElement, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { InMemoryFileProvider, ProjectSession } from "@brink-lang/editor";
import { initWasm } from "@brink-lang/web";
import {
  createStudioStore,
  type DocumentSessions as StoreDocs,
  type StoreNotification,
} from "@brink/studio-store";
import { BinderContextMenu, dispatchSymbolAction, performSymbolRename } from "@brink/studio-ui";
import type { DocumentSymbol, FileOutline } from "@brink/wasm-types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

/**
 * NOTE: this package's vitest suite aliases `brink-web` to
 * `src/__mocks__/brink-web.ts` (`studioTestWasmAliases`, `vitest.config.ts`),
 * so every `project.getSession().deleteSymbol(...)` call in this file lands
 * on the JSDOM MOCK's regex implementation, never `brink_ide::
 * structural_delete::delete_symbol`. The mock's refusal wording matches
 * production's own (verified against the Rust-driven fixture) in
 * `structural-refusal-shape.test.ts`, which owns that parity check — this
 * file does not repeat it; see the describe block below.
 */

const MAIN = [
  "=== one ===",
  "First.",
  "= alpha",
  "A.",
  "= beta",
  "B.",
  "",
  "=== two ===",
  "Second.",
  "",
].join("\n");

/** A `DocumentSymbol` with the offset fields filled in — the menu reads only
 *  `kind`/`name`/`children`, but the type demands the rest. */
function sym(kind: string, name: string, children: DocumentSymbol[] = []): DocumentSymbol {
  return { kind, name, start: 0, end: 0, full_start: 0, full_end: 0, children };
}

const OUTLINE: FileOutline[] = [
  {
    path: "main.ink",
    mounted: false,
    symbols: [
      sym("knot", "one", [sym("stitch", "alpha"), sym("stitch", "beta")]),
      sym("knot", "two"),
    ],
  },
];

function stubDocuments(): StoreDocs {
  return {
    invalidateFile: vi.fn(),
    triggerCompile: vi.fn(),
  } as unknown as StoreDocs;
}

/** A store wired the way `mount.tsx` wires it, plus a capture of every
 *  notification raised — the channel a refusal would report through. */
async function makeStore(files: Record<string, string>) {
  await initWasm();
  const provider = new InMemoryFileProvider(files);
  const project = new ProjectSession({ provider, entryFile: "main.ink" });
  await project.initialize();
  const store = createStudioStore();
  store.setState({ _project: project, _documents: stubDocuments() });
  const raised: StoreNotification[] = [];
  store.getState().setNotifier((n) => raised.push(n));
  return { store, project, raised };
}

// ── Context-menu rendering ──────────────────────────────────────────

let root: Root | null = null;
let container: HTMLDivElement | null = null;

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  root = null;
  container = null;
});

/** Render the real `BinderContextMenu` for a target and return the labels it
 *  offers plus the actions it emitted. Top-level items only: the sole submenu
 *  ("Move to") is generated from the file's knot NAMES, so it can never carry
 *  a lifecycle affordance — pinned by `submenuLabels` below. */
function renderMenu(target: ComponentProps<typeof BinderContextMenu>["target"]) {
  const emitted: Array<{ type: string }> = [];
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  act(() => {
    root!.render(
      createElement(BinderContextMenu, {
        x: 0,
        y: 0,
        target,
        outline: OUTLINE,
        onAction: (action: { type: string }) => emitted.push(action),
        onClose: () => {},
      }),
    );
  });
  const items = [...container.querySelectorAll(".brink-context-menu-item")] as HTMLElement[];
  return {
    emitted,
    items,
    labels: items.map((el) => el.textContent ?? ""),
    submenuLabels: items
      .filter((el) => el.classList.contains("brink-context-menu-has-submenu"))
      .map((el) => el.querySelector("span")?.textContent ?? ""),
    click(label: string) {
      const hit = items.find((el) => (el.textContent ?? "").startsWith(label));
      expect(hit, `no menu item labelled ${JSON.stringify(label)}`).toBeTruthy();
      act(() => hit!.click());
    },
  };
}

const KNOT_TARGET = {
  kind: "knot" as const,
  path: "main.ink",
  knot: "one",
  index: 0,
  siblingCount: 2,
};

const STITCH_TARGET = {
  kind: "stitch" as const,
  path: "main.ink",
  knot: "one",
  stitch: "alpha",
  index: 0,
  siblingCount: 2,
};

// ── The mock exists and refuses, matching production's wording ───────
//
// This describe block drives `src/__mocks__/brink-web.ts` (see the NOTE
// above `MAIN`), not `brink_ide::structural_delete::delete_symbol` — this
// package's vitest config aliases `brink-web` to that mock package-wide. The
// three refusal cases (missing knot, missing file, missing stitch-in-knot)
// already exist verbatim as data-table entries in
// `structural-refusal-shape.test.ts` (same mock, same three inputs, same
// three fixture keys) — that file is the mock↔production parity suite, so
// they are not repeated here. Only the success case is unique to this file:
// it is not a mock↔production parity check, only evidence that the refusals
// pinned elsewhere come from the mock's control flow deliberately, not from
// the mock itself being broken.

describe("the jsdom mock's delete_symbol answers the ProjectSession call (#2636)", () => {
  it("succeeds on a real knot, so a real call site (if one existed) would work", async () => {
    const { project } = await makeStore({ "main.ink": MAIN });

    const result = project.getSession().deleteSymbol("main.ink", "two");

    expect(result.ok).toBe(true);
  });
});

// ── …and no studio surface reaches it ───────────────────────────────

describe("no user path invokes delete_symbol (#2636)", () => {
  it("offers no Delete on a knot's context menu", () => {
    const menu = renderMenu(KNOT_TARGET);

    // Pin the whole menu, not just the absence: an assertion that only says
    // "no Delete" would keep passing if the menu stopped rendering entirely.
    expect(menu.labels.length).toBeGreaterThan(0);
    expect(menu.labels.filter((l) => /delete/i.test(l))).toEqual([]);
    // KNOT_TARGET ("one") has stitches, so `buildItems` skips the "Demote
    // into" submenu entirely (`hasStitches` guard, BinderContextMenu.tsx) —
    // this menu has NO submenu at all. Pin that explicitly: without it, a
    // Delete added one level down inside a future submenu would not turn the
    // top-level check above red.
    expect(menu.submenuLabels).toEqual([]);
  });

  it("offers no Delete on a stitch's context menu, submenu included", () => {
    const menu = renderMenu(STITCH_TARGET);

    expect(menu.labels.filter((l) => /delete/i.test(l))).toEqual([]);
    // The only submenu is the knot-name list behind "Move to"; nothing else
    // can be hiding a lifecycle affordance one level down.
    expect(menu.submenuLabels).toEqual(["Move to"]);
  });

  it("DOES offer Delete on a file, so the two assertions above are not vacuous", () => {
    const menu = renderMenu({
      kind: "file" as const,
      path: "main.ink",
      canDelete: true,
      canRename: true,
    });

    expect(menu.labels).toContain("Delete");
    menu.click("Delete");
    // Files delete; symbols do not. Same menu component, same harness.
    expect(menu.emitted).toEqual([{ type: "deleteFile", path: "main.ink" }]);
  });

  it("dispatchSymbolAction has no delete route: a delete-shaped action is a no-op", async () => {
    const { store, project, raised } = await makeStore({ "main.ink": MAIN });
    const state = store.getState();
    const before = project.getSession().getFileSource("main.ink");

    // The cast is the point: `deleteSymbol` is not a member of
    // `ContextMenuAction`, so this shape is unconstructible through the real
    // menu. It falls to the dispatcher's `default` arm and returns.
    await dispatchSymbolAction(state, state.applyMoveResult, {
      type: "deleteSymbol",
      path: "main.ink",
      knot: "nowhere",
    } as unknown as Parameters<typeof dispatchSymbolAction>[2]);

    expect(project.getSession().getFileSource("main.ink")).toBe(before);
    expect(raised).toEqual([]);
  });
});

// ── The control: the rename path DOES notify, in this same harness ───

describe("the rename refusal path, for contrast (#2528)", () => {
  it("raises an error notification the delete path has no equivalent of", async () => {
    const { store, raised } = await makeStore({ "main.ink": MAIN });
    const state = store.getState();

    // The same trigger the delete suite's missing-file case uses: the menu
    // captured a path, the file went away before the op ran.
    const outcome = await performSymbolRename(
      state,
      state.applyMoveResult,
      { path: "vanished.ink", knot: "one", currentName: "one" },
      "renamed",
      false,
    );

    expect(outcome.applied).toBe(false);
    // `notifyRenameRefusal` — #2528's invariant, `docs/studio-shell-spec.md`
    // §7.5. This is the toast #2636 asks whether Delete has. It does not:
    // deleting the same vanished file refuses identically one layer down, and
    // nothing above that layer is listening.
    expect(raised).toHaveLength(1);
    expect(raised[0]!.severity).toBe("error");
    expect(raised[0]!.source).toBe("binder");
    expect(raised[0]!.message).toContain("Rename one failed");
    expect(raised[0]!.message).toContain(outcome.error!);
  });
});
