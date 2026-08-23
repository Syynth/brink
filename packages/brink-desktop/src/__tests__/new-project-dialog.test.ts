// @vitest-environment jsdom
/**
 * The New Project dialog (#3012 — compare
 * `docs/design/project-open-flow/NewProject.dc.html`): Create stays
 * disabled until a folder is chosen and the entry name is valid; the
 * "Will create" panel tracks the entry name; a create failure surfaces
 * in the dialog instead of closing it; success closes and opens the
 * created brink.toml on the toml door.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import { showNewProjectDialog, type NewProjectApi } from "../new-project-dialog.js";

function api(overrides: Partial<NewProjectApi> = {}): NewProjectApi {
  return {
    chooseFolder: vi.fn(() => Promise.resolve("/stories/nightjar")),
    create: vi.fn(() => Promise.resolve("/stories/nightjar/brink.toml")),
    open: vi.fn(() => Promise.resolve()),
    ...overrides,
  };
}

const flush = async (): Promise<void> => {
  await Promise.resolve();
  await Promise.resolve();
};

describe("showNewProjectDialog", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
  });

  it("disables Create until a folder is chosen, then enables it", async () => {
    const a = api();
    const overlay = showNewProjectDialog(a);
    const createBtn = overlay.querySelector<HTMLButtonElement>(".np-create");
    expect(createBtn?.disabled).toBe(true);
    overlay.querySelector<HTMLButtonElement>(".np-choose")?.click();
    await flush();
    expect(createBtn?.disabled).toBe(false);
    expect(overlay.textContent).toContain("/stories/nightjar");
  });

  it("the Will-create panel tracks the entry name, and an invalid name disables Create with a reason", async () => {
    const overlay = showNewProjectDialog(api());
    overlay.querySelector<HTMLButtonElement>(".np-choose")?.click();
    await flush();
    const input = overlay.querySelector<HTMLInputElement>(".np-entry");
    expect(overlay.querySelector(".np-create-ink")?.textContent).toBe("main.ink");
    expect(overlay.querySelector(".np-toml-note")?.textContent).toBe('entry = "main.ink"');
    if (input === null) return;
    input.value = "harbour.ink";
    input.dispatchEvent(new Event("input"));
    expect(overlay.querySelector(".np-create-ink")?.textContent).toBe("harbour.ink");
    input.value = "nope.txt";
    input.dispatchEvent(new Event("input"));
    expect(overlay.querySelector<HTMLButtonElement>(".np-create")?.disabled).toBe(true);
    expect(overlay.textContent).toContain("must end in .ink");
  });

  it("creates then opens the returned brink.toml and closes", async () => {
    const a = api();
    const overlay = showNewProjectDialog(a);
    overlay.querySelector<HTMLButtonElement>(".np-choose")?.click();
    await flush();
    overlay.querySelector<HTMLButtonElement>(".np-create")?.click();
    await flush();
    expect(a.create).toHaveBeenCalledWith("/stories/nightjar", "main.ink");
    expect(a.open).toHaveBeenCalledWith("/stories/nightjar/brink.toml");
    expect(document.getElementById("new-project-overlay")).toBeNull();
  });

  it("a create failure surfaces in the dialog and keeps it open", async () => {
    const a = api({
      create: vi.fn(() =>
        Promise.reject(new Error("cannot create project: /x already has a brink.toml")),
      ),
    });
    const overlay = showNewProjectDialog(a);
    overlay.querySelector<HTMLButtonElement>(".np-choose")?.click();
    await flush();
    overlay.querySelector<HTMLButtonElement>(".np-create")?.click();
    await flush();
    expect(document.getElementById("new-project-overlay")).not.toBeNull();
    expect(overlay.textContent).toContain("already has a brink.toml");
    expect(a.open).not.toHaveBeenCalled();
  });

  it("Cancel and Escape both close; a second call focuses the existing dialog", () => {
    const overlay = showNewProjectDialog(api());
    expect(showNewProjectDialog(api())).toBe(overlay);
    overlay.querySelector<HTMLButtonElement>(".np-cancel")?.click();
    expect(document.getElementById("new-project-overlay")).toBeNull();
    const second = showNewProjectDialog(api());
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect(document.getElementById("new-project-overlay")).toBeNull();
    expect(second).not.toBe(overlay);
  });
});
