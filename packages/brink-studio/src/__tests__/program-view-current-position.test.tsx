/**
 * ProgramView current-knot/current-instruction highlight (D9, #3187 review
 * finding): `.pv-current-knot`/`.pv-current-instruction` paint when the live
 * `debugState.position` matches a knot's `container_idx`/disasm offset and
 * `programChecksum` === `compiledChecksum`, and both suppress the moment
 * `sessionDegraded` goes true — never showing a stale highlight
 * (docs/live-inspector-spec.md §5).
 */

import { describe, expect, it, afterEach } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { CommandRegistry, KeymapOverridesService, ShellProvider, ThemeService } from "@brink/studio-shell";
import { createStudioStore, type StudioStore } from "@brink/studio-store";
import { ProgramView, StoreProvider } from "@brink/studio-ui";
import type { ProgramModel } from "@brink/wasm-types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement | null = null;
let root: Root | null = null;

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  root = null;
  container = null;
});

const PROGRAM_MODEL: ProgramModel = {
  checksum: "chk-1",
  globals: [],
  lists: [],
  externals: [],
  knots: [
    {
      path: "intro",
      name: "intro",
      kind: "knot",
      flags: [],
      path_hash: 0,
      container_idx: 2,
      byte_size: 64,
      container_count: 1,
      disasm: [
        { offset: 0, text: "Const 1" },
        { offset: 4, text: "SetTemp x" },
      ],
      children: [],
    },
  ],
};

function mount(store: StudioStore): void {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  const commands = new CommandRegistry();
  const themes = new ThemeService();
  const overrides = new KeymapOverridesService();
  act(() => {
    root!.render(
      createElement(
        ShellProvider,
        { commands, themes, keymapOverrides: overrides, isMac: true } as never,
        createElement(StoreProvider, { store } as never, createElement(ProgramView)),
      ),
    );
  });
}

/** The disasm/current markers only render once the knot row is expanded. */
function expandFirstKnot(): void {
  act(() => {
    container!.querySelector<HTMLButtonElement>(".pv-knot-header")?.click();
  });
}

describe("ProgramView current-position highlight (D9, #3187)", () => {
  it("paints .pv-current-knot / .pv-current-instruction on a live position match", () => {
    const store = createStudioStore();
    store.setState({
      programModel: PROGRAM_MODEL,
      programChecksum: "chk-1",
      compiledChecksum: "chk-1",
      debugState: {
        status: "active",
        position: { container_idx: 2, offset: 4 },
        turn_index: 0,
        globals: [],
        call_stack: [],
        visit_counts: [],
        pending_choices: [],
        rng: { seed: 0, previous: 0 },
      } as never,
    });
    mount(store);
    expandFirstKnot();

    expect(container!.querySelector(".pv-current-knot")).not.toBeNull();
    const currentInstr = container!.querySelector(".pv-current-instruction");
    expect(currentInstr).not.toBeNull();
    expect(currentInstr?.textContent).toContain("SetTemp x");
    // The other instruction, at a different offset, must NOT be marked.
    const lines = [...container!.querySelectorAll(".pv-disasm-line")];
    expect(lines).toHaveLength(2);
    expect(lines.filter((l) => l.classList.contains("pv-current-instruction"))).toHaveLength(1);
  });

  it("suppresses both highlights once the session is degraded (checksum divergence)", () => {
    const store = createStudioStore();
    store.setState({
      programModel: PROGRAM_MODEL,
      // programChecksum (what's actually running) no longer matches the
      // studio's latest compile (compiledChecksum) — sessionDegraded(...) is
      // true, so the position must be suppressed, not shown stale.
      programChecksum: "chk-1",
      compiledChecksum: "chk-2",
      debugState: {
        status: "active",
        position: { container_idx: 2, offset: 4 },
        turn_index: 0,
        globals: [],
        call_stack: [],
        visit_counts: [],
        pending_choices: [],
        rng: { seed: 0, previous: 0 },
      } as never,
    });
    mount(store);
    expandFirstKnot();

    expect(container!.querySelector(".pv-current-knot")).toBeNull();
    expect(container!.querySelector(".pv-current-instruction")).toBeNull();
  });
});
