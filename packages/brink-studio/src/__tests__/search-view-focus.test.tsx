/**
 * `SearchView` query focus/select (#2527) — the properties that make the
 * view's unguarded `select()` correct.
 *
 * #2511 (`SymbolRenamePrompt`) established the studio-wide rule now written
 * down in `docs/studio-shell-spec.md` §7.7.1: seed a text input synchronously
 * at mount, and never `select()` text the user typed, because `select()`
 * primes the next keystroke to replace the whole value. `SearchView.tsx`
 * calls `focus()` + `select()` on every `focusSeq` change including mount,
 * with no dirty guard, which looks like the same defect — and is not one.
 * The query field is *controlled*, so there is no deferred seed to clobber
 * anything, and the effect fires only on mount or on an explicit
 * `search.focus`, where selecting the old query is the point (VS Code
 * precedent). Re-invoking Find in Files with the previous query still in the
 * box is exactly when the author wants to type straight over it.
 *
 * So these are not tests of a fix — no behaviour changed. They are the guard
 * over the two properties that keep the unguarded `select()` honest, plus the
 * intended behaviour itself so nobody "fixes" it into a regression:
 *
 *   - `selects the whole query …` fails if the select-on-invoke is removed or
 *     put behind a dirty guard borrowed from `SymbolRenamePrompt`.
 *   - `leaves the caret alone …` fails if the effect's `[focusSeq]` list is
 *     widened or dropped, so that unrelated re-renders re-select the query
 *     while the author is typing into it.
 *   - `advances the focus sequence only …` fails if a focus request is ever
 *     raised from a path the user did not initiate — the one change that
 *     would turn this `select()` into a live input-loss bug.
 *
 * The shell-level half of the command (opening the tool window, the Mod-6
 * numbering) is covered by `shell-search.test.ts`; this file covers what the
 * mounted view does with the focus sequence.
 */

import { afterEach, describe, expect, it } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import {
  CommandRegistry,
  ToolWindowRegistry,
  createShellLayoutStore,
  type ToolWindowDescriptor,
} from "@brink/studio-shell";
import { createStudioStore, type StudioStore } from "@brink/studio-store";
import {
  SEARCH_FOCUS_COMMAND_ID,
  SEARCH_TOOL_WINDOW_ID,
  SearchView,
  StoreProvider,
  registerSearchFocusCommand,
} from "@brink/studio-ui";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let container: HTMLDivElement | null = null;

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  root = null;
  container = null;
});

function descriptor(id: string): ToolWindowDescriptor {
  return {
    id,
    title: id,
    icon: null,
    defaultPlacement: { dock: "left", section: "start" },
    defaultOpen: false,
    component: () => null,
  };
}

interface Harness {
  store: StudioStore;
  /** Invoke `search.focus` exactly as Mod-Shift-F and the palette do. */
  invokeFindInFiles(): void;
}

/**
 * Mount `SearchView` over a store whose query slice already holds `query` —
 * the state an author is in whenever they search twice in a session — and
 * register the real `search.focus` command against it.
 *
 * The view is mounted directly rather than through `ShellFrame`: the command
 * still reaches it by the production route (command → `requestSearchFocus()`
 * → `searchFocusSeq` → the view's effect), and the shell's own open path is
 * already covered by `shell-search.test.ts`.
 */
function mountSearchView(query: string): Harness {
  const store = createStudioStore();
  act(() => {
    store.getState().setSearchQuery(query);
  });

  const commands = new CommandRegistry();
  const layout = createShellLayoutStore();
  const registry = new ToolWindowRegistry();
  registry.register(descriptor(SEARCH_TOOL_WINDOW_ID));
  layout.getState().syncFromRegistry(registry.list());
  registerSearchFocusCommand(commands, layout, store);

  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  act(() => {
    root!.render(createElement(StoreProvider, { store, children: createElement(SearchView) }));
  });

  return {
    store,
    invokeFindInFiles() {
      act(() => {
        commands.dispatch(SEARCH_FOCUS_COMMAND_ID);
      });
    },
  };
}

function queryInput(): HTMLInputElement {
  const el = container?.querySelector(".search-input");
  expect(el, "the search view mounted its query input").toBeInstanceOf(HTMLInputElement);
  return el as HTMLInputElement;
}

describe("SearchView query focus (#2527)", () => {
  it("selects the whole query when the user re-invokes Find in Files", () => {
    const { invokeFindInFiles } = mountSearchView("barter");

    // Park the caret mid-query, as leaving the field mid-edit would.
    const input = queryInput();
    input.setSelectionRange(3, 3);

    invokeFindInFiles();

    // Mod-Shift-F on a box that still holds the last query means "replace
    // it" — the query must come back selected so the next keystroke does.
    // This is the behaviour #2511's dirty guard would wrongly suppress here.
    expect(document.activeElement).toBe(input);
    expect(input.selectionStart).toBe(0);
    expect(input.selectionEnd).toBe("barter".length);
  });

  it("leaves the caret alone on a re-render the user did not ask for", () => {
    const { store } = mountSearchView("barter");

    // The author is typing in the query box, caret mid-word.
    const input = queryInput();
    input.focus();
    input.setSelectionRange(3, 3);

    // Something unrelated updates the store and re-renders the view. The
    // replace field is the cheapest such neighbour; a search result landing
    // has the same shape. Neither is a focus request, so the focus effect
    // must not run — if it does, the query is selected out from under the
    // caret and the next keystroke replaces the whole thing.
    act(() => {
      store.getState().setSearchReplace("haggle");
    });

    expect(input.selectionStart).toBe(3);
    expect(input.selectionEnd).toBe(3);
  });

  it("advances the focus sequence only when the user invokes the command", () => {
    const { store, invokeFindInFiles } = mountSearchView("bar");

    // A whole typing session: refining the query, flipping the option
    // buttons, filling in a replacement, and letting the debounced search
    // run. None of it is a request to focus, so none of it may select the
    // query — the property the unguarded `select()` above rests on.
    act(() => {
      store.getState().setSearchQuery("bart");
      store.getState().setSearchQuery("barte");
      store.getState().setSearchQuery("barter");
      store.getState().toggleSearchOption("caseSensitive");
      store.getState().setSearchReplace("haggle");
      store.getState().runSearch();
    });

    expect(store.getState().searchFocusSeq).toBe(0);

    // …and the command still moves it, so the guard above is measuring a
    // sequence that is genuinely live rather than one nothing ever writes.
    invokeFindInFiles();
    expect(store.getState().searchFocusSeq).toBe(1);
  });
});
