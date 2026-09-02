/**
 * The Diagnostics section of Settings (#3148).
 *
 * Two lists, and **which list a code is in IS whether it is in
 * `brink.toml`** — so the assertions here are mostly about which list a
 * row landed in, not about its styling.
 *
 * The registry is the compiler's (#3169), so this drives a fake project
 * that returns real-shaped rows: overridable and not, native-only and not,
 * with and without an explanation. Those are the four axes the section
 * branches on.
 */
import { describe, it, expect, afterEach, vi } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { LintSettings, StoreProvider } from "@brink/studio-ui";
import { createStudioStore } from "@brink/studio-store";
import type { FileOutline } from "@brink/wasm-types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const INK_PROJECT: FileOutline[] = [
  { path: "brink.toml", symbols: [], mounted: false },
  { path: "main.ink", symbols: [], mounted: false },
];

const NATIVE_PROJECT: FileOutline[] = [
  { path: "brink.toml", symbols: [], mounted: false },
  { path: "story.brink", symbols: [], mounted: false },
];

/** Real-shaped registry rows covering the axes the section branches on. */
const REGISTRY = [
  {
    code: "E014",
    title: "logic line has no effect",
    default_severity: "warning" as const,
    overridable: true,
    category: "Logic",
    surfaces: ["ink", "native"],
    explanation: "A `~` line whose expression is neither an assignment nor a call.",
  },
  {
    code: "E033",
    title: "unreachable code after divert",
    default_severity: "warning" as const,
    overridable: true,
    category: "Flow",
    surfaces: ["ink", "native"],
  },
  {
    code: "E164",
    title: "markup span tag is not in the host manifest",
    default_severity: "warning" as const,
    overridable: true,
    category: "Host markup",
    surfaces: ["native"],
  },
  {
    code: "E001",
    title: "knot is missing a name",
    default_severity: "error" as const,
    overridable: false,
    surfaces: ["ink", "native"],
  },
];

const CONFIG = `[project]
entry = "main.ink"

# keep me
[lints]
E014 = "deny"
`;

let root: Root | null = null;
let container: HTMLDivElement | null = null;

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  root = null;
  container = null;
});

function fakeProject(initial: string) {
  let source = initial;
  const applied: string[] = [];
  return {
    applied,
    getSource: () => source,
    project: {
      getDiagnosticRegistry: () => REGISTRY,
      getSession: () => ({
        getFileSource: (p: string) => (p === "brink.toml" ? source : null),
      }),
      applyEdit: (_path: string, next: string) => {
        source = next;
        applied.push(next);
        return true;
      },
    },
  };
}

function mount(outline: FileOutline[], initial = CONFIG) {
  const fake = fakeProject(initial);
  const store = createStudioStore();
  store.getState().setCompileResult(outline, { errors: 0, warnings: 0 }, [], null);
  store.setState({
    _project: fake.project as never,
    _documents: {
      refreshExternal: vi.fn(),
      triggerCompile: vi.fn(),
    } as never,
  });
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  act(() => {
    root!.render(createElement(StoreProvider, { store, children: createElement(LintSettings) }));
  });
  return fake;
}

/** The codes in each list, top list first. */
function lists(): string[][] {
  return [...container!.querySelectorAll(".lint-list")].map((list) =>
    [...list.querySelectorAll(".lint-code")].map((c) => c.textContent ?? ""),
  );
}

function rowFor(code: string): HTMLElement | null {
  for (const row of container!.querySelectorAll(".lint-row")) {
    if (row.querySelector(".lint-code")?.textContent === code) return row as HTMLElement;
  }
  return null;
}

describe("the two lists (#3148)", () => {
  it("puts a code in brink.toml in the configured list, and the rest below", () => {
    mount(INK_PROJECT);
    const [configured, unconfigured] = lists();
    expect(configured).toEqual(["E014"]);
    expect(unconfigured).toEqual(["E033"]);
  });

  it("shows the configured code's level, read from the file", () => {
    mount(INK_PROJECT);
    expect(rowFor("E014")?.querySelector(".lint-level.on")?.textContent).toBe("deny");
  });

  it("never lists a non-overridable code", () => {
    // `[lints]` cannot set it, so offering a picker would build the silent
    // no-op the section exists to prevent.
    mount(INK_PROJECT);
    expect(lists().flat()).not.toContain("E001");
  });

  it("hides a native-only code from an ink-only project", () => {
    mount(INK_PROJECT);
    expect(lists().flat()).not.toContain("E164");
  });

  it("shows it to a project that has .brink files", () => {
    // The control: without this, a filter that hid everything would look
    // like a pass above.
    mount(NATIVE_PROJECT);
    expect(lists().flat()).toContain("E164");
  });
});

describe("moving codes between the lists", () => {
  it("Configure writes the key at the code's current default", () => {
    // The first click must not change what the build does — it brings the
    // code under the project's control, nothing more.
    const fake = mount(INK_PROJECT);
    act(() => {
      rowFor("E033")?.querySelector<HTMLButtonElement>(".lint-configure")?.click();
    });
    expect(fake.getSource()).toContain('E033 = "warn"');
    expect(fake.applied).toHaveLength(1);
  });

  it("the down arrow removes the key entirely", () => {
    const fake = mount(INK_PROJECT);
    act(() => {
      rowFor("E014")?.querySelector<HTMLButtonElement>(".lint-move")?.click();
    });
    expect(fake.getSource()).not.toContain("E014");
    // The table and its comment survive — these are line edits, not a
    // parse-and-reserialize.
    expect(fake.getSource()).toContain("# keep me");
    expect(fake.getSource()).toContain("[lints]");
  });

  it("picking a level rewrites just that key", () => {
    const fake = mount(INK_PROJECT);
    const allow = [...(rowFor("E014")?.querySelectorAll<HTMLButtonElement>(".lint-level") ?? [])].find(
      (b) => b.textContent === "allow",
    );
    act(() => allow?.click());
    expect(fake.getSource()).toContain('E014 = "allow"');
    expect(fake.getSource()).toContain('entry = "main.ink"');
  });
});

describe("codes this compiler does not know", () => {
  const WITH_UNKNOWN = `[project]\nentry = "main.ink"\n\n[lints]\nE999 = "deny"\n`;

  it("keeps them rather than dropping them", () => {
    // It may belong to a newer compiler, and the file is the author's.
    mount(INK_PROJECT, WITH_UNKNOWN);
    expect(rowFor("E999")).not.toBeNull();
    expect(
      [...container!.querySelectorAll(".lint-group-head")].map((g) => g.textContent),
    ).toContain("Unknown to this compiler");
  });

  it("offers no level picker for one", () => {
    // Nothing is known about it, so any level shown would be invented.
    mount(INK_PROJECT, WITH_UNKNOWN);
    expect(rowFor("E999")?.querySelector(".lint-level")).toBeNull();
  });
});

describe("the extended description", () => {
  it("is collapsed until asked for, and only offered when one exists", () => {
    mount(INK_PROJECT);
    expect(container!.querySelector(".lint-explanation")).toBeNull();

    const disclose = rowFor("E014")?.querySelector<HTMLButtonElement>("button.lint-disclose");
    expect(disclose, "E014 has an explanation, so it gets a disclosure").not.toBeNull();
    act(() => disclose?.click());
    expect(container!.querySelector(".lint-explanation")?.textContent).toContain(
      "neither an assignment nor a call",
    );

    // E033 has none, so there is nothing to open — the slot stays for
    // alignment but is not a button.
    expect(rowFor("E033")?.querySelector("button.lint-disclose")).toBeNull();
  });
});

describe("deny-warnings", () => {
  it("reflects and writes the policy key", () => {
    const fake = mount(INK_PROJECT);
    const box = container!.querySelector<HTMLInputElement>(".lint-policy input");
    expect(box?.checked).toBe(false);
    act(() => box?.click());
    expect(fake.getSource()).toContain("deny-warnings = true");
    // Unchecking removes the key rather than writing `false`: absent and
    // false mean the same thing, and the smaller file is the honest one.
    act(() => container!.querySelector<HTMLInputElement>(".lint-policy input")?.click());
    expect(fake.getSource()).not.toContain("deny-warnings");
  });
});

describe("a project with no brink.toml", () => {
  it("says so instead of rendering empty lists", () => {
    mount([{ path: "main.ink", symbols: [], mounted: false }], CONFIG);
    expect(container!.textContent).toContain("no");
    expect(container!.querySelector(".lint-list")).toBeNull();
  });
});

// ── the Fix column (#3419, docs/autofix-spec.md §6.1) ─────────────────────
//
// `[fix]` and `[lints]` are independent tables keyed by the same code, so
// these assertions exercise the Fix column across both the `[lints]`-
// configured and unconfigured rows, and confirm it never touches `[lints]`.

const CONFIG_WITH_FIX = `[project]
entry = "main.ink"

[lints]
E014 = "deny"

[fix]
E014 = "off"
E033 = "auto"
`;

describe("the Fix column beside severity", () => {
  it("reads a configured code's fix policy from [fix]", () => {
    mount(INK_PROJECT, CONFIG_WITH_FIX);
    expect(rowFor("E014")?.querySelector(".fix-level.on")?.textContent).toBe("off");
  });

  it("reads an unconfigured (in [lints]) code's fix policy from [fix] too", () => {
    // E033 has no [lints] entry (it's in the "Not configured" list) but does
    // have a [fix] entry — the two tables are independent.
    mount(INK_PROJECT, CONFIG_WITH_FIX);
    expect(rowFor("E033")?.querySelector(".fix-level.on")?.textContent).toBe("auto");
  });

  it("shows no fix level selected when [fix] doesn't mention the code", () => {
    mount(INK_PROJECT, CONFIG);
    expect(rowFor("E014")?.querySelector(".fix-level.on")).toBeNull();
  });

  it("picking a fix level writes [fix], not [lints]", () => {
    const fake = mount(INK_PROJECT, CONFIG);
    const autoButton = [
      ...(rowFor("E014")?.querySelectorAll<HTMLButtonElement>(".fix-level") ?? []),
    ].find((b) => b.textContent === "auto");
    act(() => autoButton?.click());
    expect(fake.getSource()).toContain('[fix]\nE014 = "auto"');
    // The existing [lints] entry is untouched.
    expect(fake.getSource()).toContain('E014 = "deny"');
  });

  it("picking a fix level for an unconfigured code does not add it to [lints]", () => {
    const fake = mount(INK_PROJECT, CONFIG);
    const offButton = [
      ...(rowFor("E033")?.querySelectorAll<HTMLButtonElement>(".fix-level") ?? []),
    ].find((b) => b.textContent === "off");
    act(() => offButton?.click());
    expect(fake.getSource()).toContain('[fix]\nE033 = "off"');
    // E033 must still be in the unconfigured list, not the [lints]-configured one.
    const [configured] = lists();
    expect(configured).not.toContain("E033");
  });

  it("keeps an unknown-to-the-compiler code's fix policy visible", () => {
    const WITH_UNKNOWN_FIX = `[project]\nentry = "main.ink"\n\n[fix]\nE999 = "auto"\n`;
    mount(INK_PROJECT, WITH_UNKNOWN_FIX);
    expect(rowFor("E999")).not.toBeNull();
    expect(rowFor("E999")?.textContent).toContain("auto");
  });

  it("removing an unknown code clears both [lints] and [fix] in one edit", () => {
    const WITH_BOTH = `[project]\nentry = "main.ink"\n\n[lints]\nE999 = "deny"\n\n[fix]\nE999 = "auto"\n`;
    const fake = mount(INK_PROJECT, WITH_BOTH);
    act(() => {
      rowFor("E999")?.querySelector<HTMLButtonElement>(".lint-move")?.click();
    });
    expect(fake.getSource()).not.toContain("E999");
    expect(fake.applied).toHaveLength(1);
  });

  // A code known to the compiler, not in [lints], but non-overridable or
  // off-surface — the row selection cannot key on the [lints]-only gates
  // (`overridable`, `surfaces`) alone or these entries render nowhere:
  // not `configured` (no [lints] key), not `unknown` (the compiler DOES
  // know the code). Adversarial review on #3419/PR #3443.

  it("shows a [fix] entry for a known, non-overridable code even though [lints] can never set it", () => {
    // E001 is overridable: false — pins the overridable gate specifically.
    mount(INK_PROJECT, `[project]\nentry = "main.ink"\n\n[fix]\nE001 = "off"\n`);
    const row = rowFor("E001");
    expect(row).not.toBeNull();
    expect(row?.querySelector(".fix-level.on")?.textContent).toBe("off");
  });

  it("shows a [fix] entry for a code whose surface this project doesn't produce", () => {
    // E164 is surfaces: ["native"]; INK_PROJECT has no .brink file — pins
    // the surface gate independently of the overridable gate above.
    mount(INK_PROJECT, `[project]\nentry = "main.ink"\n\n[fix]\nE164 = "auto"\n`);
    const row = rowFor("E164");
    expect(row).not.toBeNull();
    expect(row?.querySelector(".fix-level.on")?.textContent).toBe("auto");
  });

  it("offers no [lints] Configure affordance for a [fix]-only non-overridable code", () => {
    // The row is visible (previous test), but [lints] genuinely cannot act
    // on a non-overridable code — Configure must not be offered for it.
    mount(INK_PROJECT, `[project]\nentry = "main.ink"\n\n[fix]\nE001 = "off"\n`);
    expect(rowFor("E001")?.querySelector(".lint-configure")).toBeNull();
  });

  it("still offers [lints] Configure for an ordinary unconfigured code", () => {
    // Control for the case above: an overridable, on-surface code with no
    // [fix] entry keeps its Configure button.
    mount(INK_PROJECT, CONFIG);
    expect(rowFor("E033")?.querySelector(".lint-configure")).not.toBeNull();
  });

  it("clicking the active fix level again clears it", () => {
    const fake = mount(INK_PROJECT, CONFIG_WITH_FIX);
    const offButton = [
      ...(rowFor("E014")?.querySelectorAll<HTMLButtonElement>(".fix-level") ?? []),
    ].find((b) => b.textContent === "off");
    expect(offButton?.classList.contains("on")).toBe(true);
    act(() => offButton?.click());
    expect(fake.getSource()).not.toContain('[fix]\nE014 = "off"');
    expect(rowFor("E014")?.querySelector(".fix-level.on")).toBeNull();
    // [lints] is untouched by clearing [fix].
    expect(fake.getSource()).toContain('E014 = "deny"');
  });
});
