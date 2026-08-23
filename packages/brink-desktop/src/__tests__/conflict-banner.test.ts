// @vitest-environment jsdom
/**
 * The shell-owned conflict banner (#3010/#3021 — compare
 * `docs/design/project-open-flow/Conflict.dc.html`): renders the
 * governing-config warning, offers the one-click switch ONLY when the
 * opened file is the config's declared entry, and dismisses cleanly.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import { clearConflictBanner, renderConflictBanner } from "../conflict-banner.js";
import type { ConflictModel } from "../project-open.js";

function model(overrides: Partial<ConflictModel> = {}): ConflictModel {
  return {
    configPath: "/repo/brink.toml",
    relConfig: "../brink.toml",
    entry: "chapter3.ink",
    openedIsEntry: true,
    trace: [
      { step: 1, path: "chapter3.ink", note: "opened", found: false },
      { step: 2, path: "../brink.toml", note: "governs — entry = chapter3.ink", found: true },
    ],
    warnings: [],
    ...overrides,
  };
}

describe("renderConflictBanner", () => {
  let host: HTMLElement;
  beforeEach(() => {
    document.body.innerHTML = "<div id='host'></div>";
    host = document.getElementById("host") as HTMLElement;
  });

  it("renders the message with the relative config path and both actions when the file is the entry", () => {
    const actions = { switchToProject: vi.fn(), keepStandalone: vi.fn() };
    renderConflictBanner(host, model(), actions);
    expect(host.textContent).toContain("A project config governs this file");
    expect(host.textContent).toContain("../brink.toml");
    expect(host.textContent).toContain("names it as its entry");
    const switchBtn = host.querySelector<HTMLButtonElement>(".conflict-switch");
    expect(switchBtn).not.toBeNull();
    switchBtn?.click();
    expect(actions.switchToProject).toHaveBeenCalledTimes(1);
  });

  it("omits the switch when the opened file is NOT the declared entry (the ruling's condition)", () => {
    renderConflictBanner(host, model({ openedIsEntry: false }), {
      switchToProject: vi.fn(),
      keepStandalone: vi.fn(),
    });
    expect(host.querySelector(".conflict-switch")).toBeNull();
    expect(host.querySelector(".conflict-keep")).not.toBeNull();
    expect(host.textContent).not.toContain("names it as its entry");
  });

  it("renders the walk-up trace with the found row emphasized", () => {
    renderConflictBanner(host, model(), { switchToProject: vi.fn(), keepStandalone: vi.fn() });
    const rows = host.querySelectorAll(".trace-row");
    expect(rows).toHaveLength(2);
    expect(rows[1]?.classList.contains("trace-found")).toBe(true);
    expect(host.textContent).toContain("How the config was found");
  });

  it("surfaces discovery warnings (a malformed governing config must not be silent)", () => {
    renderConflictBanner(
      host,
      model({ entry: null, openedIsEntry: false, warnings: ["brink.toml: parse error"] }),
      { switchToProject: vi.fn(), keepStandalone: vi.fn() },
    );
    expect(host.textContent).toContain("brink.toml: parse error");
  });

  it("keepStandalone fires and clearConflictBanner empties the host", () => {
    const actions = { switchToProject: vi.fn(), keepStandalone: vi.fn() };
    renderConflictBanner(host, model(), actions);
    host.querySelector<HTMLButtonElement>(".conflict-keep")?.click();
    expect(actions.keepStandalone).toHaveBeenCalledTimes(1);
    clearConflictBanner(host);
    expect(host.childElementCount).toBe(0);
  });

  it("re-rendering replaces rather than stacks", () => {
    const actions = { switchToProject: vi.fn(), keepStandalone: vi.fn() };
    renderConflictBanner(host, model(), actions);
    renderConflictBanner(host, model(), actions);
    expect(host.querySelectorAll(".conflict-banner")).toHaveLength(1);
  });
});
