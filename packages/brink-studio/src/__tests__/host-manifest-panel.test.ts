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
  EXAMPLE_MAP_POINT_WIDGET,
  EXAMPLE_REGION_WIDGET,
  manifestPanelItems,
} from "../example-extension.js";

const EXAMPLE_WIDGETS = [EXAMPLE_REGION_WIDGET, EXAMPLE_MAP_POINT_WIDGET];

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
      {
        name: "ping",
        signature: "ping()",
        call: "~ ping()\n",
        doc: "",
        kind: "plain",
        fields: [],
        groups: [],
      },
    ]);
  });

  it("returns no rows for an empty manifest", () => {
    expect(manifestPanelItems({})).toEqual([]);
  });

  it("resolves per-param form fields, with the widget kind from the type", () => {
    const items = manifestPanelItems(EXAMPLE_HOST_MANIFEST);
    // set_tint(color: hex_color) — hex_color declares the `color` widget.
    const setTint = items.find((i) => i.name === "set_tint");
    expect(setTint?.fields).toEqual([
      { paramName: "color", paramIndex: 0, typeName: "hex_color", widgetKind: "color" },
    ]);
    // show_picture(name, x, y) — plain types, no widgets → text fields.
    const showPicture = items.find((i) => i.name === "show_picture");
    expect(showPicture?.fields.map((f) => f.widgetKind)).toEqual([undefined, undefined, undefined]);
  });

  it("resolves value-lists, host widgets, and arg-groups from the host widgets", () => {
    const items = manifestPanelItems(EXAMPLE_HOST_MANIFEST, EXAMPLE_WIDGETS);
    // teleport(map: map_id, x, y) — map is a value-list; (x, y) is a group.
    const teleport = items.find((i) => i.name === "teleport");
    const mapField = teleport?.fields.find((f) => f.paramName === "map");
    expect(mapField?.values?.map((v) => v.label)).toEqual(["Harbor", "Old Temple", "Catacombs"]);
    // x and y are folded into one group control, not individual fields.
    expect(teleport?.fields.map((f) => f.paramName)).toEqual(["map"]);
    expect(teleport?.groups).toHaveLength(1);
    expect(teleport?.groups[0]).toMatchObject({
      paramIndices: [1, 2],
      paramNames: ["x", "y"],
      typeName: "map_point",
      contextParams: { map: 0 },
    });
    // go_region(region: region_id) — a host widget, not a plain field.
    const region = items.find((i) => i.name === "go_region")?.fields[0];
    expect(region?.hostWidget?.type).toBe("host.example.region");
  });
});
