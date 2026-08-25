/**
 * W4 worker road (docs/editor-worker-spec.md §8/§12): the WorkerTransport
 * over a loopback fake worker running the REAL SessionHostCore — the same
 * core LocalTransport runs, so these tests pin that the message boundary
 * preserves the protocol semantics — plus the ProjectSession worker-mode
 * plumbing: full-then-incremental file flush, config-log replay, ordering
 * (mutations before the query), crash/boot-failure fallback to the
 * in-process road.
 */

import { describe, expect, it, vi } from "vitest";
import type { EditorSessionHandle } from "@brink-lang/web";
import type { SessionRequest, SessionResponse } from "@brink/wasm-types";
import { ProjectSession } from "../project-session.js";
import { InMemoryFileProvider } from "../provider.js";
import { SessionHostCore, type SessionServerLike } from "../worker/session-host.js";
import { SessionClient } from "../worker/session-client.js";
import { WorkerTransport, type WorkerLike } from "../worker/worker-transport.js";

/** A loopback "worker": requests dispatch into a real SessionHostCore on a
 *  microtask (the worker entry's exact scheduling), responses come back
 *  through the message listener. */
function makeLoopbackWorker(server: SessionServerLike & Record<string, unknown>): {
  worker: WorkerLike;
  crash: (message: string) => void;
} {
  let onMessage: ((ev: { data: unknown }) => void) | null = null;
  let onError: ((ev: { message?: string }) => void) | null = null;
  const core = new SessionHostCore(server, (response: SessionResponse) => {
    onMessage?.({ data: response });
  });
  let drainScheduled = false;
  const worker: WorkerLike = {
    postMessage: (data) => {
      core.accept(data as SessionRequest);
      if (!drainScheduled) {
        drainScheduled = true;
        queueMicrotask(() => {
          drainScheduled = false;
          core.drain();
        });
      }
    },
    terminate: () => core.stop(),
    addEventListener: ((type: string, listener: unknown) => {
      if (type === "message") onMessage = listener as typeof onMessage;
      else onError = listener as typeof onError;
    }) as WorkerLike["addEventListener"],
  };
  return { worker, crash: (message) => onError?.({ message }) };
}

function makeServer() {
  const calls: string[] = [];
  const files = new Map<string, string>();
  const server = {
    calls,
    updateDocument: () => null,
    configEpoch: () => 0,
    updateFile(path: string, source: string) {
      calls.push(`updateFile:${path}`);
      files.set(path, source);
    },
    removeFile(path: string) {
      calls.push(`removeFile:${path}`);
      files.delete(path);
    },
    setExternalCheck(level: string) {
      calls.push(`setExternalCheck:${level}`);
    },
    compileProject(entry: string) {
      calls.push(`compile:${entry}`);
      return { ok: true, files: [...files.keys()].sort(), entry };
    },
  };
  return { server: server as SessionServerLike & Record<string, unknown>, calls };
}

describe("WorkerTransport over a loopback SessionHostCore", () => {
  it("round-trips queries with the shared host semantics", async () => {
    const { server, calls } = makeServer();
    const { worker } = makeLoopbackWorker(server);
    const client = new SessionClient(new WorkerTransport(worker));
    client.files("updateFile", "a.ink", "-> DONE\n");
    const result = await client.query<{ files: string[] }>("compileProject", ["a.ink"]).promise;
    expect(result.value.files).toEqual(["a.ink"]);
    // The mutation applied BEFORE the query despite both crossing the wire.
    expect(calls).toEqual(["updateFile:a.ink", "compile:a.ink"]);
  });

  it("close() terminates the worker and stops delivery", async () => {
    const { server } = makeServer();
    const { worker } = makeLoopbackWorker(server);
    const transport = new WorkerTransport(worker);
    const client = new SessionClient(transport);
    const pending = client.query("compileProject", ["a.ink"]);
    client.close();
    await expect(pending.promise).rejects.toMatchObject({ reason: "cancelled" });
    expect(() => transport.post({ kind: "cancel", id: 1 })).toThrow();
  });
});

describe("ProjectSession worker road", () => {
  function makeMainStub() {
    const files = new Map<string, string>([["main.ink", "-> DONE\n"]]);
    const stub = {
      generation: 0,
      listFiles: () => [...files.keys()].sort().map((path) => ({ path })),
      getFileSource: (path: string) => files.get(path) ?? null,
      updateFile: (path: string, source: string) => {
        files.set(path, source);
      },
      removeFile: (path: string) => files.delete(path),
      setExternalCheck: () => {},
      compileProject: () => {
        throw new Error("main-thread compile must not run in worker mode");
      },
      free: vi.fn(),
    };
    return { stub, files };
  }

  function makeProject(overrides?: { factory?: () => WorkerLike | null }) {
    const { stub, files } = makeMainStub();
    const { server, calls } = makeServer();
    const { worker, crash } = makeLoopbackWorker(server);
    const project = new ProjectSession({
      provider: new InMemoryFileProvider({}),
      entryFile: "main.ink",
      session: stub as unknown as EditorSessionHandle,
      workerSession: true,
      workerFactory: overrides?.factory ?? (() => worker),
    });
    return { project, stub, files, workerCalls: calls, crash };
  }

  it("streams the whole project on first query, then only changed files", async () => {
    const { project, workerCalls } = makeProject();
    const first = await project.projectQuery<{ files: string[] }>("compileProject", [
      "main.ink",
    ]);
    expect(first.files).toEqual(["main.ink"]);
    expect(workerCalls).toEqual(["updateFile:main.ink", "compile:main.ink"]);

    workerCalls.length = 0;
    // An edit through the mirrored session choke point marks the path dirty.
    project.getSession().updateFile("main.ink", "changed\n-> DONE\n");
    project.getSession().updateFile("other.ink", "-> DONE\n");
    await project.projectQuery("compileProject", ["main.ink"]);
    expect(workerCalls.sort()).toEqual([
      "compile:main.ink",
      "updateFile:main.ink",
      "updateFile:other.ink",
    ]);

    workerCalls.length = 0;
    // Nothing dirty: no file traffic at all.
    await project.projectQuery("compileProject", ["main.ink"]);
    expect(workerCalls).toEqual(["compile:main.ink"]);
  });

  it("replays config mutations to the worker in order", async () => {
    const { project, workerCalls } = makeProject();
    project.getSession().setExternalCheck("off");
    await project.projectQuery("compileProject", ["main.ink"]);
    expect(workerCalls).toEqual([
      "updateFile:main.ink",
      "setExternalCheck:off",
      "compile:main.ink",
    ]);
    workerCalls.length = 0;
    // Already replayed — not re-sent.
    await project.projectQuery("compileProject", ["main.ink"]);
    expect(workerCalls).toEqual(["compile:main.ink"]);
  });

  it("falls back to the in-process road when no worker can be created", async () => {
    const { stub } = makeMainStub();
    stub.compileProject = (() => ({ ok: true, files: ["local"] })) as never;
    const project = new ProjectSession({
      provider: new InMemoryFileProvider({}),
      entryFile: "main.ink",
      session: stub as unknown as EditorSessionHandle,
      workerSession: true,
      workerFactory: () => null,
    });
    const result = await project.projectQuery<{ files: string[] }>("compileProject", [
      "main.ink",
    ]);
    expect(result.files).toEqual(["local"]);
  });

  it("a worker crash rejects in-flight work and later queries run in-process", async () => {
    const { project, crash, stub } = makeProject();
    (stub as Record<string, unknown>).compileProject = () => ({ ok: true, files: ["local"] });
    const inFlight = project.projectQuery("compileProject", ["main.ink"]);
    crash("boom");
    await expect(inFlight).rejects.toMatchObject({ reason: "cancelled" });
    const retried = await project.projectQuery<{ files: string[] }>("compileProject", [
      "main.ink",
    ]);
    expect(retried.files).toEqual(["local"]);
  });

  it("destroy() closes the worker client", async () => {
    const { project } = makeProject();
    const pending = project.projectQuery("compileProject", ["main.ink"]);
    project.destroy();
    await expect(pending).rejects.toMatchObject({ reason: "cancelled" });
  });
});
