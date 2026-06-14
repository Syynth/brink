/**
 * Program identity + degraded mode (docs/live-inspector-spec.md §5, #181).
 *
 * Source mapping — graph current-location highlight, visit-count badges — is
 * valid only when the running program IS the studio's latest compile. The
 * studio compares two checksums:
 *   - the running program's identity (`programChecksum`, mirrored from the
 *     provider snapshot), and
 *   - its latest successful compile (`compiledChecksum`, computed by the
 *     compile slice via the `program_checksum(bytes)` wasm util).
 * `sessionDegraded(running, compiled)` is true only when both are known and
 * differ. Locally this never fires (a recompile hot-reloads the session, so
 * the two always match); it is reached by a remote provider whose game runs an
 * older program than the studio's source.
 */

import { describe, it, expect } from "vitest";
import { createStudioStore, sessionDegraded } from "@brink/studio-store";

describe("sessionDegraded", () => {
  it("is false unless both checksums are known and differ", () => {
    // Unknown identity (no session, or a failed compile) is absent, not degraded.
    expect(sessionDegraded(null, null)).toBe(false);
    expect(sessionDegraded("0x1", null)).toBe(false);
    expect(sessionDegraded(null, "0x1")).toBe(false);
    // Match → full fidelity.
    expect(sessionDegraded("0xabc", "0xabc")).toBe(false);
    // Both known and different → degraded.
    expect(sessionDegraded("0xabc", "0xdef")).toBe(true);
  });
});

describe("compiledChecksum (compile slice)", () => {
  it("captures the latest compile's identity from its bytes", () => {
    const store = createStudioStore();
    // The mock `program_checksum` is a stable hash of the bytes (sum), so a
    // known input yields a known checksum: 1+2+3 = 6.
    store.getState().setCompileResult([], { errors: 0, warnings: 0 }, [], new Uint8Array([1, 2, 3]));
    expect(store.getState().compiledChecksum).toBe("0x00000006");

    // Distinct bytes → distinct identity.
    store.getState().setCompileResult([], { errors: 0, warnings: 0 }, [], new Uint8Array([4, 5]));
    expect(store.getState().compiledChecksum).toBe("0x00000009");
  });

  it("clears the checksum when the compile fails (null bytes)", () => {
    const store = createStudioStore();
    store.getState().setCompileResult([], { errors: 0, warnings: 0 }, [], new Uint8Array([1]));
    expect(store.getState().compiledChecksum).toBe("0x00000001");

    store.getState().setCompileResult([], { errors: 1, warnings: 0 }, [], null);
    expect(store.getState().compiledChecksum).toBeNull();
  });
});

describe("degraded derivation from store state", () => {
  it("flags degraded when the running program differs from the latest compile", () => {
    const store = createStudioStore();
    // Author edited + recompiled (new identity) while a session keeps running
    // the old program — the remote-provider / edit-while-playing case.
    store.setState({ programChecksum: "0xrunning0", compiledChecksum: "0xcompiled" });
    const s = store.getState();
    expect(sessionDegraded(s.programChecksum, s.compiledChecksum)).toBe(true);

    // A restart/reload realigns identity → full fidelity returns live.
    store.setState({ programChecksum: "0xcompiled" });
    const s2 = store.getState();
    expect(sessionDegraded(s2.programChecksum, s2.compiledChecksum)).toBe(false);
  });
});
