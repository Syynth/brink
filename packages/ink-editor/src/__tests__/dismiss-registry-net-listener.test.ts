/**
 * Behavioural backing (#2846 point 2, modelled on the `SAVE-PATH` precedent
 * `docs/studio-shell-spec.md` §7.7.1: "a marker's justification is proven,
 * not just present") for the `DISMISS-NET-EXEMPT` marker `#2846` added to
 * `dismiss-registry.ts`'s own `installGlobalDismissNet()` — the line that
 * newly matches once the `dismiss-registry-enrolment.test.ts` scan widened
 * to include `window`-target listeners (#2846 point 1).
 *
 * The marker's claim is narrow and mechanical, not "this manages state not
 * a surface" (the shape of the three drag/maximize markers) — it is "this
 * call site IS the global net's own installation, so it cannot enrol INTO
 * itself". That claim is only true if the call actually behaves like a
 * one-time net installer: attaches on `window`, bubble phase (`capture:
 * false`), for `"keydown"`, and — critically — does NOT re-attach on a
 * second `registerDismissible()` call (see `dismiss-registry.ts`'s own doc
 * comment on why a second listener would double-fire every registered
 * `onClose`). This file pins exactly that, against the real
 * `installGlobalDismissNet`/`registerDismissible` module, not a
 * reimplementation.
 */

import { describe, it, expect, afterEach, vi } from "vitest";
import { registerDismissible, resetDismissRegistryForTests } from "../dismiss-registry.js";

afterEach(() => {
  resetDismissRegistryForTests();
});

describe("dismiss-registry.ts's own net-install listener (#2846 DISMISS-NET-EXEMPT)", () => {
  it("installs exactly one window, bubble-phase (capture=false) keydown listener on first registerDismissible()", () => {
    resetDismissRegistryForTests();
    const addSpy = vi.spyOn(window, "addEventListener");

    const dispose = registerDismissible(() => {});

    const netCalls = addSpy.mock.calls.filter((call) => call[0] === "keydown");
    expect(netCalls).toHaveLength(1);
    // capture is the call's 3rd arg; the net listener is bubble-phase
    // (false) — deliberately the OPPOSITE of every real surface's own
    // document-capture dismiss listener (see the "LISTENER ORDERING" note
    // on this module).
    expect(netCalls[0][2]).toBe(false);

    addSpy.mockRestore();
    dispose();
  });

  it("does NOT re-attach a second listener on a second registerDismissible() call (the claim that makes it safe to be exempt rather than re-enrolled per call)", () => {
    resetDismissRegistryForTests();
    const addSpy = vi.spyOn(window, "addEventListener");

    const disposeA = registerDismissible(() => {});
    const disposeB = registerDismissible(() => {});

    const netCalls = addSpy.mock.calls.filter((call) => call[0] === "keydown");
    // If this ever became 2, a single Escape would double-fire every
    // registered onClose (this module's own doc comment names that exact
    // failure) — the net-install call site would then be lying about being
    // a one-time install, and the DISMISS-NET-EXEMPT marker's premise would
    // be false.
    expect(netCalls).toHaveLength(1);

    addSpy.mockRestore();
    disposeA();
    disposeB();
  });
});
