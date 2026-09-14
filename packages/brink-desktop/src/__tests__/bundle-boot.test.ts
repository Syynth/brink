import { describe, expect, it, vi } from "vitest";
import { confirmBundleBoot, rollbackMessage } from "../bundle-boot.js";
import type { BundleLaunchInfo } from "../tauri-provider.js";

const embedded: BundleLaunchInfo = { version: null, rolledBackFrom: null };

describe("rollbackMessage", () => {
  it("is null for an ordinary launch, bundle or embedded", () => {
    expect(rollbackMessage(embedded)).toBeNull();
    expect(rollbackMessage({ version: "0.7.1", rolledBackFrom: null })).toBeNull();
  });

  /** The author needs BOTH facts: the update is gone, and what replaced it. */
  it("names the failed version and what is running instead", () => {
    const message = rollbackMessage({ version: "0.7.1", rolledBackFrom: "0.7.2" });
    expect(message).toContain("0.7.2");
    expect(message).toContain("0.7.1");
  });

  /** Falling back to the floor has no version to name, and "running null"
   * would be worse than useless. */
  it("describes the embedded floor in words, not as a missing version", () => {
    const message = rollbackMessage({ version: null, rolledBackFrom: "0.7.2" });
    expect(message).toContain("0.7.2");
    expect(message).toContain("built into the app");
    expect(message).not.toContain("null");
  });
});

describe("confirmBundleBoot", () => {
  it("returns what the shell reports", async () => {
    const info: BundleLaunchInfo = { version: "0.7.1", rolledBackFrom: null };
    await expect(confirmBundleBoot(async () => info)).resolves.toEqual(info);
  });

  /**
   * A throw must not propagate — an unhandled rejection at module scope
   * could take the boot down, and the sentinel surviving already handles
   * the failure safely by reverting on next launch. It is logged, not
   * swallowed, because a persistently-throwing confirm is indistinguishable
   * from a persistently-broken bundle otherwise.
   */
  it("resolves null and logs when the call throws", async () => {
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    const ready = vi.fn().mockRejectedValue(new Error("ipc down"));

    await expect(confirmBundleBoot(ready)).resolves.toBeNull();
    expect(ready).toHaveBeenCalledTimes(1);
    expect(error).toHaveBeenCalled();
    error.mockRestore();
  });

  /**
   * The confirm is unconditional by construction: it takes no inputs it
   * could branch on. This pins that — a future signature growing a "should
   * we?" argument is exactly the change that silently rolls back working
   * bundles, so it should have to break a test to happen.
   */
  it("takes no condition to skip on", () => {
    expect(confirmBundleBoot.length).toBe(1);
  });
});
