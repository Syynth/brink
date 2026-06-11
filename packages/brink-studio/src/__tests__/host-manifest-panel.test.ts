/**
 * Host Functions panel mapping (issue #146): the panel renders rows derived
 * from the host-capability manifest — name + display signature + doc — and
 * each row's insertion payload is a call site ONLY (`~ fn(args)\n`), never
 * an `EXTERNAL` declaration (those already live in the story).
 */

import { describe, expect, it } from "vitest";
import type { HostManifest } from "@brink/wasm-types";
import {
  EXAMPLE_HOST_MANIFEST,
  manifestPanelItems,
} from "../example-extension.js";

describe("manifestPanelItems", () => {
  it("maps a manifest entry to name, typed signature, doc, and kind", () => {
    const items = manifestPanelItems(EXAMPLE_HOST_MANIFEST);
    const hasItem = items.find((i) => i.name === "has_item");
    expect(hasItem).toMatchObject({
      name: "has_item",
      signature: "has_item(item: item_id) -> bool",
      doc: "True if the party carries the item.",
      kind: "query",
    });
  });

  it("inserts calls only — placeholder args, no EXTERNAL anywhere", () => {
    const items = manifestPanelItems(EXAMPLE_HOST_MANIFEST);
    expect(items.length).toBeGreaterThanOrEqual(4);
    for (const item of items) {
      expect(item.call.startsWith("~ ")).toBe(true);
      expect(item.call.endsWith("\n")).toBe(true);
      expect(item.call).not.toContain("EXTERNAL");
    }
    expect(items.find((i) => i.name === "show_picture")?.call).toBe(
      "~ show_picture(name, x, y)\n",
    );
  });

  it("elides a void return and renders zero-arg signatures", () => {
    const items = manifestPanelItems(EXAMPLE_HOST_MANIFEST);
    expect(items.find((i) => i.name === "gain_gold")?.signature).toBe(
      "gain_gold(amount: int)",
    );
    const partySize = items.find((i) => i.name === "party_size");
    expect(partySize?.signature).toBe("party_size() -> int");
    expect(partySize?.call).toBe("~ party_size()\n");
  });

  it("tolerates sparse manifest entries (no params/returns/doc/kind)", () => {
    const sparse: HostManifest = { externals: [{ name: "ping" }] };
    expect(manifestPanelItems(sparse)).toEqual([
      { name: "ping", signature: "ping()", call: "~ ping()\n", doc: "", kind: "plain" },
    ]);
  });

  it("returns no rows for an empty manifest", () => {
    expect(manifestPanelItems({})).toEqual([]);
  });
});
