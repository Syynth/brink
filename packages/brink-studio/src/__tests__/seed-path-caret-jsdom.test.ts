import { describe, expect, it } from "vitest";

/**
 * The jsdom column of `docs/studio-shell-spec.md` §7.7.1's seeding/caret table
 * (#2595).
 *
 * §7.7.1 records, per seeding path, where the caret sits immediately after an
 * `<input>` is seeded — in Chromium and in jsdom. The Chromium column is pinned
 * by the control block in `packages/brink-studio/e2e/symbol-rename.spec.ts`
 * ("a defaultValue-seeded field parks the caret at the end in a real browser").
 * This file is the jsdom counterpart, deliberately measuring the SAME four
 * paths the same way, so the two columns are backed by the same kind of
 * evidence rather than one being asserted from a run nobody can repeat.
 *
 * ⚠ WHY THIS FILE EXISTS AT ALL. #2595 was filed because §7.7.1 stated a real-
 * browser reading that had only ever been measured in jsdom — a claim recorded
 * as fact with nothing in-tree to catch it going stale. Two of that table's
 * jsdom cells (the `value` attribute and `setAttribute` rows) were in the same
 * position: measured once while writing the spec, then cited. The seed-race
 * suites only ever exercise the `.value` PROPERTY path, so they cannot
 * corroborate the attribute rows. Recording an unverified reading inside the
 * fix for an unverified reading is the trap one level down; this closes it.
 *
 * These are platform assertions, not assertions about studio code. A red here
 * means jsdom's behaviour moved (or the table is wrong) — in either case
 * §7.7.1's table needs re-measuring, not this file relaxing.
 */

const SEED = "barter";

/** Mirrors the e2e control's `read` helper exactly. */
function read(el: HTMLInputElement): [number | null, number | null] {
  return [el.selectionStart, el.selectionEnd];
}

describe("§7.7.1 seeding paths — the jsdom column (#2595)", () => {
  it("the value ATTRIBUTE, parsed from markup, leaves the caret at the start", () => {
    const host = document.createElement("div");
    host.innerHTML = `<input value="${SEED}">`;
    document.body.appendChild(host);
    const input = host.firstElementChild as HTMLInputElement;

    expect(input.value).toBe(SEED);
    expect(read(input)).toEqual([0, 0]);

    host.remove();
  });

  it("setAttribute(\"value\", …) leaves the caret at the start", () => {
    const input = document.createElement("input");
    document.body.appendChild(input);
    input.setAttribute("value", SEED);

    expect(input.value).toBe(SEED);
    expect(read(input)).toEqual([0, 0]);

    input.remove();
  });

  it("a .value PROPERTY write parks the caret at the END", () => {
    // The row that carries the whole finding: this is the path React takes to
    // seed an uncontrolled field, and per the HTML standard the `value` setter
    // is SPECIFIED to move the cursor to the end of the control. jsdom
    // reproducing it is fidelity, not an artifact.
    const input = document.createElement("input");
    document.body.appendChild(input);
    input.value = SEED;

    expect(read(input)).toEqual([SEED.length, SEED.length]);

    input.remove();
  });

  it("the attribute and property paths genuinely disagree", () => {
    // Guards the table's shape, not just its cells: if jsdom ever collapsed
    // these two paths to one reading, every row above could still pass
    // individually while the distinction §7.7.1 is built on had vanished.
    const viaAttribute = document.createElement("input");
    document.body.appendChild(viaAttribute);
    viaAttribute.setAttribute("value", SEED);

    const viaProperty = document.createElement("input");
    document.body.appendChild(viaProperty);
    viaProperty.value = SEED;

    expect(read(viaAttribute)).not.toEqual(read(viaProperty));

    viaAttribute.remove();
    viaProperty.remove();
  });
});
