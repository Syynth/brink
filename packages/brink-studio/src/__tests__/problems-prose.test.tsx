/**
 * Prose findings in the Problems panel (#3256).
 *
 * This is unimplemented RULED scope, not a new feature: the original
 * spellcheck ruling said results "render as squiggles and are listable, but
 * the Problems panel FILTERS THEM OUT BY DEFAULT; the author opts in to
 * seeing them in the list". Only the squiggles half shipped, so a spelling
 * mistake was visible in the buffer and findable nowhere else.
 *
 * Two properties carry the whole design and both fail silently:
 *
 * 1. **Off by default, and off for EXISTING authors too.** A stored
 *    preferences record written before this bucket existed has no `prose`
 *    key. Reading a missing key the way every severity reads it — "only an
 *    explicit false hides it" — would switch spelling rows on for everyone
 *    at once, which is the exact outcome the ruling forbids.
 * 2. **Two producers, one list.** Compile diagnostics and prose findings
 *    have different lifetimes, so they are stored apart and joined for
 *    display. Merging them into one slot would have each producer erase the
 *    other's rows.
 */
import { describe, it, expect, afterEach } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import {
  CommandRegistry,
  KeymapOverridesService,
  ShellProvider,
  ThemeService,
} from "@brink/studio-shell";
import {
  ProblemsView,
  StoreProvider,
  countBySeverity,
  filterProblemRows,
  severityBucket,
  summarizeCounts,
} from "@brink/studio-ui";
import {
  PROSE_CODE_PREFIX,
  createStudioStore,
  isProseDiagnostic,
  loadProblemsPrefs,
  toProseDiagnostics,
  type StudioStore,
} from "@brink/studio-store";
import type { Diagnostic, FileOutline } from "@brink/wasm-types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const OUTLINE: FileOutline[] = [{ path: "main.ink", symbols: [], mounted: false }];

const compileWarning: Diagnostic = {
  start: 0,
  end: 4,
  message: "unreachable code after divert",
  severity: "Warning",
  code: "E033",
  file: "main.ink",
};

const todoNote: Diagnostic = {
  start: 10,
  end: 14,
  message: "TODO: fix the ending",
  severity: "Info",
  code: "E189",
  file: "main.ink",
};

const spelling: Diagnostic = {
  start: 20,
  end: 28,
  message: "Did you mean to spell `Griswold` this way?",
  severity: "Info",
  code: `${PROSE_CODE_PREFIX}Spelling`,
  file: "main.ink",
};

function memoryStorage(): Storage {
  const map = new Map<string, string>();
  return {
    getItem: (k: string) => map.get(k) ?? null,
    setItem: (k: string, v: string) => void map.set(k, v),
  } as unknown as Storage;
}

let root: Root | null = null;
let container: HTMLDivElement | null = null;

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  root = null;
  container = null;
});

function mount(store: StudioStore) {
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
        createElement(StoreProvider, { store, children: createElement(ProblemsView) }),
      ),
    );
  });
}

function shownMessages(): string[] {
  return [...container!.querySelectorAll(".problems-message")].map(
    (el) => el.textContent ?? "",
  );
}

describe("bucketing", () => {
  it("puts a prose finding in its own bucket, not with info", () => {
    // Severity alone would say "info" — the finding IS Info-severity. The
    // bucket keys off the source instead, which is the only way "off by
    // default" can be expressed while info stays on.
    expect(severityBucket(spelling)).toBe("prose");
    expect(severityBucket(todoNote)).toBe("info");
  });

  it("recognises a prose diagnostic by its code prefix", () => {
    expect(isProseDiagnostic(spelling)).toBe(true);
    expect(isProseDiagnostic(todoNote)).toBe(false);
    expect(isProseDiagnostic({ code: undefined })).toBe(false);
  });

  it("counts prose separately and names it 'spelling' in the summary", () => {
    const rows = [compileWarning, todoNote, spelling].map((diagnostic) => ({
      diagnostic,
      location: "",
    }));
    const counts = countBySeverity(rows);
    expect(counts).toEqual({ error: 0, warning: 1, info: 1, prose: 1 });
    expect(summarizeCounts(counts)).toBe("1 warning · 1 info · 1 spelling");
  });

  it("hides prose rows under the default toggles, keeping the rest", () => {
    const rows = [compileWarning, todoNote, spelling].map((diagnostic) => ({
      diagnostic,
      location: "",
    }));
    const defaults = { error: true, warning: true, info: true, prose: false };
    expect(filterProblemRows(rows, defaults, "").map((r) => r.diagnostic.code)).toEqual([
      "E033",
      "E189",
    ]);
    const opted = { ...defaults, prose: true };
    expect(filterProblemRows(rows, opted, "")).toHaveLength(3);
  });
});

describe("mapping the editor's findings to panel rows", () => {
  // `mountStudio` is the only caller and no unit test constructs one, so
  // the mapping is asserted here rather than being wired-and-hoped.
  const lint = {
    start: 20,
    end: 28,
    kind: "Spelling",
    message: "Did you mean to spell `Griswold` this way?",
    suggestions: [],
  };

  it("carries the file, so the row lands in the right group", () => {
    expect(toProseDiagnostics("chapters/two.ink", [lint])[0]?.file).toBe("chapters/two.ink");
  });

  it("marks it with the prose prefix, keeping the checker's rule name", () => {
    const [d] = toProseDiagnostics("main.ink", [lint]);
    expect(d?.code).toBe(`${PROSE_CODE_PREFIX}Spelling`);
    expect(severityBucket(d!)).toBe("prose");
  });

  it("passes offsets through unconverted", () => {
    // Both sides count UTF-16 code units; a conversion here would move
    // every row off its word.
    const [d] = toProseDiagnostics("main.ink", [lint]);
    expect([d?.start, d?.end]).toEqual([20, 28]);
  });

  it("maps an empty set to an empty set", () => {
    expect(toProseDiagnostics("main.ink", [])).toEqual([]);
  });
});

describe("the default is off, including for existing authors", () => {
  it("defaults prose off with nothing stored", () => {
    expect(loadProblemsPrefs(memoryStorage()).severities.prose).toBe(false);
  });

  it("keeps prose off for a record written before the bucket existed", () => {
    // The failure this rules out: reading a missing key the way severities
    // read theirs ("only an explicit false hides it") would switch spelling
    // rows on for every existing author on upgrade.
    const storage = memoryStorage();
    storage.setItem(
      "brink-studio.problems.v1",
      JSON.stringify({ severities: { error: true, warning: true, info: true }, grouped: true }),
    );
    expect(loadProblemsPrefs(storage).severities.prose).toBe(false);
  });

  it("honours an explicit opt-in", () => {
    const storage = memoryStorage();
    storage.setItem(
      "brink-studio.problems.v1",
      JSON.stringify({ severities: { prose: true }, grouped: true }),
    );
    expect(loadProblemsPrefs(storage).severities.prose).toBe(true);
  });
});

describe("the store's two producers", () => {
  function seeded(): StudioStore {
    const store = createStudioStore();
    store.getState().setCompileResult(OUTLINE, { errors: 0, warnings: 1 }, [compileWarning], null);
    store.getState().setProseDiagnostics("main.ink", [spelling]);
    return store;
  }

  it("keeps compile diagnostics when prose findings arrive", () => {
    const store = seeded();
    expect(store.getState().diagnosticsList).toEqual([compileWarning]);
    expect(store.getState().proseDiagnostics["main.ink"]).toEqual([spelling]);
  });

  it("keeps prose findings across a recompile", () => {
    // The lifetimes really are independent: a compile must not clear a
    // file's spelling rows, since the prose checker did not re-run.
    const store = seeded();
    store.getState().setCompileResult(OUTLINE, { errors: 0, warnings: 0 }, [], null);
    expect(store.getState().proseDiagnostics["main.ink"]).toEqual([spelling]);
  });

  it("clears a file's findings when it reports none", () => {
    const store = seeded();
    store.getState().setProseDiagnostics("main.ink", []);
    expect(store.getState().proseDiagnostics["main.ink"]).toBeUndefined();
  });

  it("does not churn state when a debounce reports the same findings", () => {
    // The prose extension republishes on every debounce, usually with
    // nothing new; an unconditional set would re-render the panel on every
    // typing pause.
    const store = seeded();
    const before = store.getState().proseDiagnostics;
    store.getState().setProseDiagnostics("main.ink", [{ ...spelling }]);
    expect(store.getState().proseDiagnostics).toBe(before);
  });

  it("does not create an entry for a file that reports nothing", () => {
    const store = createStudioStore();
    const before = store.getState().proseDiagnostics;
    store.getState().setProseDiagnostics("clean.ink", []);
    expect(store.getState().proseDiagnostics).toBe(before);
  });
});

describe("the panel", () => {
  function seeded(): StudioStore {
    const store = createStudioStore();
    store.getState().setCompileResult(OUTLINE, { errors: 0, warnings: 1 }, [compileWarning], null);
    store.getState().setProseDiagnostics("main.ink", [spelling]);
    return store;
  }

  it("does not list a spelling finding by default", () => {
    const store = seeded();
    mount(store);
    const shown = shownMessages().join(" ");
    expect(shown).toContain("unreachable code");
    expect(shown).not.toContain("Griswold");
  });

  it("lists it once the author opts in", () => {
    // The whole point of the issue: a spelling mistake was findable only by
    // spotting the squiggle.
    const store = seeded();
    store.getState().toggleProblemSeverity("prose");
    mount(store);
    expect(shownMessages().join(" ")).toContain("Griswold");
  });
});
