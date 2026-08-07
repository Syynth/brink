import { describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
const listen = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: (...args: unknown[]) => listen(...args) }));

const { runCli } = await import("../cli.js");

describe("runCli", () => {
  it("invokes run_cli with root/rel/subcommand/rest and returns the exit code", async () => {
    invoke.mockResolvedValueOnce(0);
    const code = await runCli({
      root: "/proj",
      rel: "story.brink",
      subcommand: "export-xliff",
      rest: ["--output", "out.xlf"],
    });
    expect(code).toBe(0);
    expect(invoke).toHaveBeenCalledWith("run_cli", {
      root: "/proj",
      rel: "story.brink",
      subcommand: "export-xliff",
      rest: ["--output", "out.xlf"],
    });
  });

  it("defaults rest to an empty array when omitted", async () => {
    invoke.mockResolvedValueOnce(0);
    await runCli({ root: "/proj", rel: "story.brink", subcommand: "compile" });
    expect(invoke).toHaveBeenCalledWith("run_cli", {
      root: "/proj",
      rel: "story.brink",
      subcommand: "compile",
      rest: [],
    });
  });

  it("propagates a non-zero exit code without throwing", async () => {
    invoke.mockResolvedValueOnce(1);
    await expect(
      runCli({ root: "/proj", rel: "story.brink", subcommand: "compile" }),
    ).resolves.toBe(1);
  });

  it("rejects when the shell rejects the invoke (e.g. disallowed subcommand)", async () => {
    invoke.mockRejectedValueOnce(new Error("subcommand not in the sidecar allowlist: play"));
    await expect(
      runCli({ root: "/proj", rel: "story.brink", subcommand: "play" }),
    ).rejects.toThrow("sidecar allowlist");
  });

  it("does not subscribe to cli:output when no onOutput callback is given", async () => {
    invoke.mockResolvedValueOnce(0);
    listen.mockClear();
    await runCli({ root: "/proj", rel: "story.brink", subcommand: "compile" });
    expect(listen).not.toHaveBeenCalled();
  });

  it("streams cli:output events to onOutput and unsubscribes once the run resolves", async () => {
    const unlisten = vi.fn();
    let handler: ((event: { payload: unknown }) => void) | undefined;
    listen.mockImplementationOnce((_name: string, cb: (event: { payload: unknown }) => void) => {
      handler = cb;
      return Promise.resolve(unlisten);
    });
    invoke.mockImplementationOnce(async () => {
      handler?.({ payload: { stream: "stdout", line: "compiling…" } });
      return 0;
    });

    const onOutput = vi.fn();
    const code = await runCli(
      { root: "/proj", rel: "story.brink", subcommand: "compile" },
      onOutput,
    );

    expect(code).toBe(0);
    expect(onOutput).toHaveBeenCalledWith({ stream: "stdout", line: "compiling…" });
    expect(unlisten).toHaveBeenCalledTimes(1);
  });
});
