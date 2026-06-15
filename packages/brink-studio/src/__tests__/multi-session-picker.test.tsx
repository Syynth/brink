/**
 * Multi-session picker rendering (docs/multi-session-spec.md §5, #182).
 *
 * The picker is hidden until there's more than one session (no picker noise in
 * the single-session studio), then lists them and switches the active session.
 */

import { describe, it, expect, afterEach } from "vitest";
import { createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { act } from "react";
import {
  createStudioStore,
  LocalSessionProvider,
  DEFAULT_SESSION_ID,
} from "@brink/studio-store";
import { SessionPicker, StoreProvider } from "@brink/studio-ui";

function entry(id: string, label: string, status: "running" | "ended") {
  return {
    id,
    label,
    provider: new LocalSessionProvider({
      runner: { continueSingle: () => ({ type: "end", text: "", tags: [] }) } as never,
      status: status as never,
      transcript: [],
    }),
  };
}

describe("SessionPicker", () => {
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
        createElement(StoreProvider, { store, children: createElement(SessionPicker) }),
      );
    });
  }

  it("is hidden with ≤1 session", () => {
    const store = createStudioStore();
    mount(store);
    expect(container!.querySelector(".brink-status-sessions")).toBeNull();

    act(() => store.setState({ sessions: [entry(DEFAULT_SESSION_ID, "Main", "running")] }));
    expect(container!.querySelector(".brink-status-sessions")).toBeNull();
  });

  it("lists sessions and switches the active one once there are ≥2", () => {
    const store = createStudioStore();
    mount(store);
    act(() =>
      store.setState({
        sessions: [
          entry(DEFAULT_SESSION_ID, "Main", "running"),
          entry("local:1", "Branch", "ended"),
        ],
        activeSessionId: DEFAULT_SESSION_ID,
      }),
    );

    const select = container!.querySelector<HTMLSelectElement>(".brink-session-select");
    expect(select).not.toBeNull();
    expect([...select!.options].map((o) => o.textContent)).toEqual(["Main", "Branch"]);
    expect(select!.value).toBe(DEFAULT_SESSION_ID);

    // Selecting another option repoints the active session.
    act(() => {
      select!.value = "local:1";
      select!.dispatchEvent(new Event("change", { bubbles: true }));
    });
    expect(store.getState().activeSessionId).toBe("local:1");

    // The close button shows for the non-primary active session.
    expect(container!.querySelector(".brink-session-close")).not.toBeNull();
  });

  it("hides the close button while the primary session is active", () => {
    const store = createStudioStore();
    mount(store);
    act(() =>
      store.setState({
        sessions: [
          entry(DEFAULT_SESSION_ID, "Main", "running"),
          entry("local:1", "Branch", "ended"),
        ],
        activeSessionId: DEFAULT_SESSION_ID,
      }),
    );
    expect(container!.querySelector(".brink-session-close")).toBeNull();
  });
});
