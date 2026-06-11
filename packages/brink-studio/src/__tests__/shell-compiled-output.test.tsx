/**
 * Compiled Output document tests (issue #91, spec §4 / §7.8).
 *
 * Covers: command registration + dispatch opening/focusing the singleton tab
 * (never duplicating), real read-only enforcement (a dispatched change
 * transaction does not alter the doc; only the component's own annotated
 * replace does), the .inkt fold service, and the component's placeholder vs
 * content states (compile-bound: programInkt drives both).
 */

import { describe, expect, it, afterEach } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { foldable } from "@codemirror/language";
import {
  CommandRegistry,
  createEditorGroupsStore,
  documentKey,
  findTab,
} from "@brink/studio-shell";
import { createStudioStore } from "@brink/studio-store";
import {
  COMPILED_OUTPUT_TYPE_ID,
  CompiledOutputDocument,
  OPEN_COMPILED_OUTPUT_COMMAND_ID,
  StoreProvider,
  compiledOutputExtensions,
  compiledOutputRef,
  registerCompiledOutputCommand,
  replaceCompiledOutput,
} from "@brink/studio-ui";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const SAMPLE_INKT = `(story
  (name_table
    0 ""
  )
  (container $01_406ea523c53def
    (name 0)
    (lines
      0 "Hello, world!" @626e7681b4e2e7bc
    )
    (code
      enter_container $01_6647014fb554e5
      done
    )
  )
)`;

// ── Command wiring ──────────────────────────────────────────────────

describe("program.openCompiledOutput", () => {
  it("opens the singleton pinned in the focused group", () => {
    const commands = new CommandRegistry();
    const groups = createEditorGroupsStore();
    registerCompiledOutputCommand(commands, groups);

    expect(commands.dispatch(OPEN_COMPILED_OUTPUT_COMMAND_ID)).toBe(true);

    const key = documentKey(compiledOutputRef());
    const found = findTab(groups.getState().groups, key);
    expect(found).not.toBeNull();
    expect(found!.tab.pinned).toBe(true);
    expect(found!.tab.ref.typeId).toBe(COMPILED_OUTPUT_TYPE_ID);
    expect(found!.tab.ref.title).toBe("Compiled Output");
    expect(found!.group.activeKey).toBe(key);
  });

  it("re-dispatch focuses the existing tab instead of duplicating", () => {
    const commands = new CommandRegistry();
    const groups = createEditorGroupsStore();
    registerCompiledOutputCommand(commands, groups);

    // Open the document in group 1, then move focus to a new right group
    // holding an ink file.
    commands.dispatch(OPEN_COMPILED_OUTPUT_COMMAND_ID);
    const homeGroupId = groups.getState().focusedGroupId;
    groups.getState().openDocument(
      { typeId: "ink-file", docId: "main.ink", title: "main.ink" },
      { group: "split-right" },
    );
    expect(groups.getState().focusedGroupId).not.toBe(homeGroupId);

    // Reopen via the command: reveal policy focuses the tab where it lives.
    commands.dispatch(OPEN_COMPILED_OUTPUT_COMMAND_ID);
    const s = groups.getState();
    expect(s.focusedGroupId).toBe(homeGroupId);

    const key = documentKey(compiledOutputRef());
    const instances = s.groups.flatMap((g) =>
      g.tabs.filter((t) => documentKey(t.ref) === key),
    );
    expect(instances).toHaveLength(1);
  });

  it("reopens after the tab is closed", () => {
    const commands = new CommandRegistry();
    const groups = createEditorGroupsStore();
    registerCompiledOutputCommand(commands, groups);

    const key = documentKey(compiledOutputRef());
    commands.dispatch(OPEN_COMPILED_OUTPUT_COMMAND_ID);
    groups.getState().closeTab(groups.getState().focusedGroupId, key);
    expect(findTab(groups.getState().groups, key)).toBeNull();

    commands.dispatch(OPEN_COMPILED_OUTPUT_COMMAND_ID);
    expect(findTab(groups.getState().groups, key)).not.toBeNull();
  });
});

// ── Read-only enforcement ───────────────────────────────────────────

describe("compiled output read-only", () => {
  it("drops dispatched change transactions that are not its own replace", () => {
    const view = new EditorView({
      state: EditorState.create({ doc: SAMPLE_INKT, extensions: compiledOutputExtensions() }),
    });

    // A plain change transaction (what typing/paste/drop would produce, and
    // what any stray programmatic dispatch produces) must not alter the doc.
    view.dispatch({ changes: { from: 0, to: 0, insert: "INJECTED" } });
    expect(view.state.doc.toString()).toBe(SAMPLE_INKT);

    // The component's own annotated content replace goes through.
    replaceCompiledOutput(view, "(story\n)");
    expect(view.state.doc.toString()).toBe("(story\n)");

    view.destroy();
  });

  it("is readOnly and non-editable at the state/DOM level", () => {
    const view = new EditorView({
      state: EditorState.create({ doc: SAMPLE_INKT, extensions: compiledOutputExtensions() }),
    });
    expect(view.state.readOnly).toBe(true);
    expect(view.contentDOM.getAttribute("contenteditable")).toBe("false");
    // Focusable so the CM6 search keymap (Mod-F) works inside the view.
    expect(view.contentDOM.getAttribute("tabindex")).toBe("0");
    view.destroy();
  });
});

// ── .inkt folding ───────────────────────────────────────────────────

describe("inkt fold service", () => {
  it("folds a form over its more-indented body, keeping the close paren", () => {
    const state = EditorState.create({
      doc: SAMPLE_INKT,
      extensions: compiledOutputExtensions(),
    });

    // `(container …` (line 5) folds over its indented body but not the
    // sibling-level closer.
    const container = state.doc.line(5);
    const range = foldable(state, container.from, container.to);
    expect(range).not.toBeNull();
    expect(range!.from).toBe(container.to);
    expect(range!.to).toBe(state.doc.line(13).to); // last body line: `    )` of (code

    // A leaf line has nothing to fold.
    const leaf = state.doc.line(8); // `      0 "Hello, world!" …`
    expect(foldable(state, leaf.from, leaf.to)).toBeNull();
  });
});

// ── Placeholder vs content (compile-bound) ──────────────────────────

describe("CompiledOutputDocument component", () => {
  let root: Root | null = null;
  let container: HTMLDivElement | null = null;

  afterEach(() => {
    act(() => root?.unmount());
    container?.remove();
    root = null;
    container = null;
  });

  function mount(store: ReturnType<typeof createStudioStore>) {
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
    act(() => {
      root!.render(
        createElement(StoreProvider, {
          store,
          children: createElement(CompiledOutputDocument, {
            doc: compiledOutputRef(),
            groupId: "group-1",
            active: true,
          }),
        }),
      );
    });
  }

  it("renders the placeholder before the first successful compile", () => {
    const store = createStudioStore();
    mount(store);
    expect(container!.querySelector(".compiled-output-empty")).not.toBeNull();
    expect(container!.textContent).toContain("No compiled program yet");
    expect(container!.querySelector(".cm-content")).toBeNull();
  });

  it("renders the dump once programInkt is set, and live-updates", () => {
    const store = createStudioStore();
    mount(store);

    act(() => store.setState({ programInkt: SAMPLE_INKT }));
    expect(container!.querySelector(".compiled-output-empty")).toBeNull();
    const content = container!.querySelector(".cm-content");
    expect(content).not.toBeNull();
    expect(content!.textContent).toContain("Hello, world!");

    // A recompile swaps programInkt; the view follows.
    act(() =>
      store.setState({ programInkt: SAMPLE_INKT.replace("Hello, world!", "Goodbye!") }),
    );
    expect(container!.querySelector(".cm-content")!.textContent).toContain("Goodbye!");
    expect(container!.querySelector(".cm-content")!.textContent).not.toContain(
      "Hello, world!",
    );
  });
});
