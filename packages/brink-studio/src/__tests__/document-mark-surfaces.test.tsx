/**
 * The `documentMark` seam paints wherever the shell writes a document's
 * NAME (#3145).
 *
 * Ruled 2026-08-27: a file's name and its draft status never appear apart.
 * That is a rule, not a list of four surfaces — but a rule that only holds
 * because four call sites happen to be correct is a rule one refactor away
 * from being false. So this file renders each naming surface for real and
 * asserts the mark is beside the name in every one of them.
 *
 * `draft-status.test.tsx` covers what the studio's own mark SAYS; this
 * covers whether the shell shows a mark at all, which is why it uses a
 * trivial stand-in component rather than `DraftMark` — the two questions
 * fail for different reasons and should fail separately.
 */

import { afterEach, beforeAll, describe, expect, it } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import {
  CommandRegistry,
  ContinuousView,
  DocumentTypeRegistry,
  EditorArea,
  EditorTakeover,
  ShellProvider,
  SingleFileView,
  createEditorGroupsStore,
  type DocumentRef,
  type DocumentViewProps,
  type EditorGroupsStore,
} from "@brink/studio-shell";

beforeAll(() => {
  if (typeof globalThis.ResizeObserver === "undefined") {
    class ResizeObserverStub {
      observe(): void {}
      unobserve(): void {}
      disconnect(): void {}
    }
    (globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = ResizeObserverStub;
  }
  if (typeof Element.prototype.scrollIntoView === "undefined") {
    Element.prototype.scrollIntoView = () => {};
  }
});

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const DOC: DocumentRef = { typeId: "test-doc", docId: "scratch/cut.ink", title: "cut.ink" };

function TestDoc({ doc }: DocumentViewProps) {
  return <div data-testid="doc-body">{doc.docId}</div>;
}

/** A stand-in mark: says only "the shell rendered documentMark here". */
function Mark({ doc }: { doc: DocumentRef }) {
  return <span data-testid="mark">{`mark:${doc.docId}`}</span>;
}

function documents(): DocumentTypeRegistry {
  const registry = new DocumentTypeRegistry();
  registry.register({ id: "test-doc", component: TestDoc });
  return registry;
}

let root: Root | null = null;
let container: HTMLDivElement | null = null;

afterEach(() => {
  if (root) act(() => root!.unmount());
  container?.remove();
  root = null;
  container = null;
});

function mount(node: React.ReactNode, groups: EditorGroupsStore, withMark: boolean) {
  container = document.createElement("div");
  container.className = "brink-studio";
  document.body.appendChild(container);
  root = createRoot(container);
  act(() => {
    root!.render(
      <ShellProvider
        commands={new CommandRegistry()}
        documents={documents()}
        editorGroups={groups}
        {...(withMark ? { documentMark: Mark } : {})}
      >
        {node}
      </ShellProvider>,
    );
  });
}

function openOne(): EditorGroupsStore {
  const groups = createEditorGroupsStore();
  act(() => {
    groups.getState().openDocument(DOC, { pinned: true });
  });
  return groups;
}

const marks = (): string[] =>
  [...container!.querySelectorAll("[data-testid='mark']")].map((el) => el.textContent ?? "");

describe("documentMark renders at every surface that names a document", () => {
  it("the Code view's tab", () => {
    mount(<EditorArea />, openOne(), true);
    expect(marks()).toContain("mark:scratch/cut.ink");
  });

  it("the Single File header", () => {
    mount(<SingleFileView />, openOne(), true);
    expect(marks()).toContain("mark:scratch/cut.ink");
  });

  it("the Continuous section heading", () => {
    mount(<ContinuousView documents={[DOC]} />, openOne(), true);
    expect(marks()).toContain("mark:scratch/cut.ink");
  });

  it("the takeover header", () => {
    mount(<EditorTakeover doc={DOC} />, openOne(), true);
    expect(marks()).toContain("mark:scratch/cut.ink");
  });
});

describe("documentMark is optional", () => {
  it("every surface renders without one", () => {
    // The prop is optional by design (a shell with no host status concept
    // is the normal case in tests), so an unguarded call site would break
    // every one of those rather than degrading.
    for (const node of [
      <EditorArea key="a" />,
      <SingleFileView key="b" />,
      <ContinuousView key="c" documents={[DOC]} />,
      <EditorTakeover key="d" doc={DOC} />,
    ]) {
      mount(node, openOne(), false);
      expect(marks()).toEqual([]);
      act(() => root!.unmount());
      container!.remove();
      root = null;
    }
  });
});
