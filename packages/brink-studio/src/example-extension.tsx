/**
 * Example host extension (docs/studio-shell-spec.md §8, issues #95/#146).
 *
 * The worked example validating the RPG Maker MZ use case: a host embedding
 * the studio registers a "Host Functions" tool window that browses the
 * external vocabulary the host already provides. The data flow models the
 * real one (Track B synergy, docs/host-capability-manifest.md): the host
 * owns a capability manifest — the playground registers
 * `EXAMPLE_HOST_MANIFEST` at mount via the `hostManifest` option
 * (`EditorSession.setHostManifest` underneath) — and the panel renders that
 * same manifest's metadata (signatures, doc comments, semantic types).
 * Click inserts ONLY a call site (`~ fn(args)`) at the cursor: the
 * `EXTERNAL` declarations already live in the story (the functions are
 * "already defined"); the panel's job is browsing the catalog, never
 * declaring it.
 *
 * Everything here goes through public surface only: the `StudioExtensions`
 * mount config, `useStudioApi()` (insertText / dispatch / notify /
 * select), and command-routed navigation (`editor.reveal`, spec §6.1). The
 * panel is an equal citizen — it docks, drags, persists, and appears in the
 * strip, palette (via its generated view.toggle command), and hamburger menu
 * with zero extension-specific shell code.
 */

import { useStudioApi, type StudioApi } from "@brink/studio-ui";
import type { StudioExtensions } from "@brink/studio-shell";
import type { HostManifest, ArgumentWidget } from "@brink/wasm-types";
import { openArgumentForm, type FormField, type FormGroup } from "@brink/ink-editor";

export const EXAMPLE_TOOL_WINDOW_ID = "host.example.functions";
export const EXAMPLE_REVEAL_COMMAND_ID = "host.example.revealStart";

// ── Pretend host-capability manifest ────────────────────────────────
//
// The playground's pretend host vocabulary, RPG Maker-flavored. The
// playground registers this at mount (`hostManifest` option), which makes
// the manifest-driven diagnostics live (E041 literal type mismatches etc. —
// toggleable via the Settings external-check flag), and the panel below
// renders the same object. The demo story declares the matching `EXTERNAL`s.

export const EXAMPLE_HOST_MANIFEST: HostManifest = {
  types: [
    { name: "item_id", base: "string" },
    // A static value source (#174): `switch_id` literals get a name label
    // inline — `set_switch(5 ⟨HarborGate⟩, true)` — with no host attached.
    {
      name: "switch_id",
      base: "int",
      values: {
        source: "static",
        items: [
          { value: "1", label: "IntroSeen", detail: "Switch #1" },
          { value: "5", label: "HarborGate", detail: "Switch #5" },
          { value: "9", label: "VaultOpen", detail: "Switch #9" },
        ],
      },
    },
    // A studio-builtin widget (argument-widget spec): a `hex_color` param
    // renders a color swatch over its string literal — click for the popover
    // picker. The `widget` declaration drives it (no magic type-name).
    { name: "hex_color", base: "string", widget: { kind: "color" } },
    // A HOST widget (argument-widget spec §3): `region_id`'s widget kind names a
    // host-provided ArgumentWidget (below). The studio renders the inline chip
    // from the host's label data and opens the host's popover editor.
    { name: "region_id", base: "string", widget: { kind: "host.example.region" } },
    // A value-list type (#174): `map_id` is an int whose pickable values carry
    // map names. The studio renders the dropdown (in the Form) and the inline
    // name label (`teleport(5 ⟨Old Temple⟩, …)`) — the host only declares the
    // values, it never reinvents a combobox.
    {
      name: "map_id",
      base: "int",
      values: {
        source: "static",
        items: [
          { value: "1", label: "Harbor", detail: "Map #1" },
          { value: "5", label: "Old Temple", detail: "Map #5" },
          { value: "9", label: "Catacombs", detail: "Map #9" },
        ],
      },
    },
  ],
  externals: [
    {
      name: "set_tint",
      params: [{ name: "color", ty: "hex_color" }],
      returns: "void",
      kind: "presentation",
      doc: "Tint the screen to a hex color.",
    },
    {
      name: "go_region",
      params: [{ name: "region", ty: "region_id" }],
      returns: "void",
      kind: "effect",
      doc: "Move the party to a named region.",
    },
    {
      // An ARG-GROUP host widget (spec §2): one `map_point` widget over (x, y),
      // opened as a MODAL, taking inter-arg `context` (the `map` arg).
      name: "teleport",
      params: [
        { name: "map", ty: "map_id" },
        { name: "x", ty: "int" },
        { name: "y", ty: "int" },
      ],
      returns: "void",
      kind: "effect",
      doc: "Teleport the party to a point on a map.",
      widgets: [
        { group: [1, 2], type: "map_point", surface: "modal", context: { map: 0 } },
      ],
    },
    {
      name: "has_item",
      params: [{ name: "item", ty: "item_id" }],
      returns: "bool",
      kind: "query",
      doc: "True if the party carries the item.",
    },
    {
      name: "set_switch",
      params: [
        { name: "id", ty: "switch_id" },
        { name: "on", ty: "bool" },
      ],
      returns: "void",
      kind: "effect",
      doc: "Set a game switch on or off.",
    },
    {
      name: "gain_gold",
      params: [{ name: "amount", ty: "int" }],
      returns: "void",
      kind: "effect",
      doc: "Add gold to the party's purse.",
    },
    {
      name: "play_se",
      params: [{ name: "name", ty: "string" }],
      returns: "void",
      kind: "presentation",
      doc: "Play a sound effect by name.",
    },
    {
      name: "show_picture",
      params: [
        { name: "name", ty: "string" },
        { name: "x", ty: "int" },
        { name: "y", ty: "int" },
      ],
      returns: "void",
      kind: "presentation",
      doc: "Show a picture at screen coordinates.",
    },
    {
      name: "party_size",
      params: [],
      returns: "int",
      kind: "query",
      doc: "Number of members in the active party.",
    },
  ],
};

// ── Manifest → panel items (pure mapping) ───────────────────────────

/** One row of the Host Functions panel, derived from a manifest entry. */
export interface HostFunctionItem {
  name: string;
  /** Display signature, e.g. `has_item(item: item_id) -> bool`. */
  signature: string;
  /** The call snippet inserted on a skeleton (modifier-click) — no `EXTERNAL`. */
  call: string;
  /** The manifest doc comment ("" when the entry carries none). */
  doc: string;
  /** The manifest kind tag ("query" | "effect" | "presentation" | "plain"). */
  kind: string;
  /** Form fields, one per non-grouped param (widget/value-list from the types). */
  fields: FormField[];
  /** Arg-group widgets (spec §2) spanning several params. */
  groups: FormGroup[];
}

/**
 * Derive the panel's rows from a host-capability manifest + the host's argument
 * widgets. Pure — the panel renders exactly what the host registered. Composing
 * a fresh call has no existing arguments, so every control starts empty.
 */
export function manifestPanelItems(
  manifest: HostManifest,
  widgets: ArgumentWidget[] = [],
): HostFunctionItem[] {
  const types = manifest.types ?? [];
  const widgetByType = new Map(widgets.map((w) => [w.type, w]));
  return (manifest.externals ?? []).map((ext) => {
    const params = ext.params ?? [];
    const sigParams = params
      .map((p) => (p.ty !== undefined ? `${p.name}: ${p.ty}` : p.name))
      .join(", ");
    const returns =
      ext.returns !== undefined && ext.returns !== "void" ? ` -> ${ext.returns}` : "";
    const args = params.map((p) => p.name).join(", ");

    // Arg-group widgets with a resolved host widget; their members are then
    // rendered by the group control, not as individual fields.
    const grouped = new Set<number>();
    const groups: FormGroup[] = [];
    for (const w of ext.widgets ?? []) {
      const hostWidget = widgetByType.get(w.type);
      if (hostWidget === undefined) continue;
      for (const idx of w.group) grouped.add(idx);
      groups.push({
        paramIndices: w.group,
        paramNames: w.group.map((i) => params[i]?.name ?? `arg${i}`),
        typeName: w.type,
        hostWidget,
        surface: w.surface,
        initialValues: [],
        contextParams: w.context,
      });
    }

    const fields: FormField[] = [];
    params.forEach((p, i) => {
      if (grouped.has(i)) return;
      const typeDef = p.ty !== undefined ? types.find((t) => t.name === p.ty) : undefined;
      const kind = typeDef?.widget?.kind;
      fields.push({
        paramName: p.name,
        paramIndex: i,
        typeName: p.ty,
        widgetKind: kind ?? undefined,
        values: typeDef?.values?.source === "static" ? typeDef.values.items : undefined,
        hostWidget: kind !== undefined ? widgetByType.get(kind) : undefined,
      });
    });

    return {
      name: ext.name,
      signature: `${ext.name}(${sigParams})${returns}`,
      call: `~ ${ext.name}(${args})\n`,
      doc: ext.doc ?? "",
      kind: ext.kind ?? "plain",
      fields,
      groups,
    };
  });
}

// ── Host argument widget (argument-widget-spec §3) ──────────────────
//
// A worked example of a host-rendered widget: `region_id` (the manifest type
// above declares `widget.kind: "host.example.region"`). The studio renders the
// inline chip from `inline()`'s label data; `editor.render` mounts the host's
// own picker UI into a studio-owned popover and `resolve`s the chosen literal.

const REGIONS = [
  { id: "harbor", label: "Harbor District" },
  { id: "market", label: "Market Square" },
  { id: "keep", label: "The Keep" },
  { id: "wilds", label: "The Wilds" },
];

export const EXAMPLE_REGION_WIDGET: ArgumentWidget = {
  type: "host.example.region",
  inline(ctx) {
    const value = ctx.values[0] ?? "";
    const region = REGIONS.find((r) => r.id === value);
    return { text: region ? region.label : value || "region", className: "host-example-region" };
  },
  editor: {
    surface: "popover",
    render(ctx, host, container) {
      const root = document.createElement("div");
      root.className = "host-example-region-picker";
      const current = ctx.values[0] ?? "";
      for (const region of REGIONS) {
        const btn = document.createElement("button");
        btn.type = "button";
        btn.className = "host-example-region-btn";
        btn.textContent = region.label;
        if (region.id === current) btn.setAttribute("aria-current", "true");
        // resolve a literal: region_id is a string type, so quote it.
        btn.addEventListener("click", () => host.resolve([`"${region.id}"`]));
        root.appendChild(btn);
      }
      container.appendChild(root);
      return () => root.remove();
    },
  },
};

// An ARG-GROUP host widget (spec §2): `teleport`'s `widgets` declares one
// `map_point` over (x, y), opened as a MODAL, taking the `map` arg as context.
// The inline chip shows the point; the modal is a click-to-place grid that
// resolves both literals at once (a multi-slot write).

const MAP_W = 10;
const MAP_H = 7;

// Map id → name, mirroring the `map_id` value-list above — the widget titles
// itself with the chosen map's name (the inter-arg context carries the id).
const MAP_NAMES: Record<string, string> = { "1": "Harbor", "5": "Old Temple", "9": "Catacombs" };

export const EXAMPLE_MAP_POINT_WIDGET: ArgumentWidget = {
  type: "map_point",
  inline(ctx) {
    const [x, y] = ctx.values;
    const text = x !== undefined && y !== undefined ? `(${x}, ${y})` : "pick point";
    return { text, className: "host-example-point" };
  },
  editor: {
    surface: "modal",
    render(ctx, host, container) {
      const root = document.createElement("div");
      root.className = "host-example-map";
      const title = document.createElement("div");
      title.className = "host-example-map-title";
      const mapId = ctx.context?.map;
      const mapName = mapId !== undefined ? (MAP_NAMES[mapId] ?? `map ${mapId}`) : undefined;
      title.textContent = mapName !== undefined ? `Pick a point — ${mapName}` : "Pick a point";
      root.appendChild(title);
      const grid = document.createElement("div");
      grid.className = "host-example-map-grid";
      grid.style.gridTemplateColumns = `repeat(${MAP_W}, 22px)`;
      const cx = Number(ctx.values[0]);
      const cy = Number(ctx.values[1]);
      for (let y = 0; y < MAP_H; y++) {
        for (let x = 0; x < MAP_W; x++) {
          const cell = document.createElement("button");
          cell.type = "button";
          cell.className = "host-example-map-cell";
          cell.title = `(${x}, ${y})`;
          if (x === cx && y === cy) cell.setAttribute("aria-current", "true");
          // int params — resolve unquoted literals for both x and y at once.
          cell.addEventListener("click", () => host.resolve([String(x), String(y)]));
          grid.appendChild(cell);
        }
      }
      root.appendChild(grid);
      container.appendChild(root);
      return () => root.remove();
    },
  },
};

// ── Panel component ─────────────────────────────────────────────────

function HostFunctionsPanel() {
  const api = useStudioApi();
  const items = manifestPanelItems(EXAMPLE_HOST_MANIFEST, [
    EXAMPLE_REGION_WIDGET,
    EXAMPLE_MAP_POINT_WIDGET,
  ]);

  /** Insert a bare skeleton (`~ fn(a, b)`) at the cursor — the quick path. */
  const insertSkeleton = (item: HostFunctionItem): void => {
    api.insertText(item.call);
    api.notify({
      severity: "info",
      source: "example host",
      message: `Inserted ${item.call.trim().slice(2)}`,
    });
  };

  /** Click → compose the call in the Form, then insert the completed call.
   *  Modifier-click (Alt) → the bare-skeleton quick path. Zero-param calls and
   *  manifests without `@brink/ink-editor`'s Form just insert the skeleton. */
  const launch = (item: HostFunctionItem, anchor: HTMLElement, quick: boolean): void => {
    if (quick || (item.fields.length === 0 && item.groups.length === 0)) {
      insertSkeleton(item);
      return;
    }
    openArgumentForm(anchor, {
      title: item.signature,
      external: item.name,
      applyLabel: "Insert",
      fields: item.fields,
      groups: item.groups,
      onApply: (literals) => {
        api.insertText(`~ ${item.name}(${literals.join(", ")})\n`);
        api.notify({
          severity: "info",
          source: "example host",
          message: `Inserted ${item.name}(${literals.join(", ")})`,
        });
      },
      onCancel: () => {},
    });
  };

  return (
    <div className="host-example-panel" style={{ padding: 8, overflow: "auto", height: "100%" }}>
      <p style={{ margin: "0 0 8px", fontSize: 12, color: "var(--bs-fg-muted)" }}>
        Functions the host provides (already declared in the story) — click to
        compose a call in the form, or Alt-click to insert a skeleton.
      </p>
      <ul style={{ listStyle: "none", margin: 0, padding: 0 }}>
        {items.map((item) => (
          <li key={item.name} style={{ marginBottom: 4 }}>
            <button
              type="button"
              className="host-example-fn"
              onClick={(e) => launch(item, e.currentTarget, e.altKey)}
              title={`${item.kind} — ${item.fields.length > 0 ? "compose" : "insert"} ${item.name}(…) · Alt-click for a skeleton`}
              style={{
                display: "block",
                width: "100%",
                textAlign: "left",
                padding: "4px 6px",
                font: "inherit",
                fontSize: 12,
                color: "var(--bs-fg)",
                background: "var(--bs-surface-bg)",
                border: "1px solid var(--bs-border)",
                borderRadius: 4,
                cursor: "pointer",
              }}
            >
              <span style={{ display: "block", fontFamily: "monospace" }}>
                {item.signature}
              </span>
              {item.doc !== "" && (
                <span
                  className="host-example-fn-doc"
                  style={{
                    display: "block",
                    marginTop: 2,
                    fontSize: 11,
                    color: "var(--bs-fg-muted)",
                  }}
                >
                  {item.doc}
                </span>
              )}
            </button>
          </li>
        ))}
      </ul>
      <button
        type="button"
        className="host-example-reveal"
        onClick={() => api.dispatch(EXAMPLE_REVEAL_COMMAND_ID)}
        style={{
          marginTop: 8,
          padding: "4px 8px",
          font: "inherit",
          fontSize: 12,
          color: "var(--bs-fg)",
          background: "transparent",
          border: "1px solid var(--bs-border)",
          borderRadius: 4,
          cursor: "pointer",
        }}
      >
        Go to story entry
      </button>
    </div>
  );
}

const EXAMPLE_ICON = (
  <svg
    width={16}
    height={16}
    viewBox="0 0 16 16"
    fill="none"
    stroke="currentColor"
    strokeWidth={1.5}
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden
  >
    {/* A plug: the host boundary. */}
    <path d="M5.5 1.5v3M10.5 1.5v3" />
    <path d="M3.5 4.5h9v3a4.5 4.5 0 0 1-9 0z" />
    <path d="M8 12v2.5" />
  </svg>
);

// ── Extension config ────────────────────────────────────────────────

/**
 * Build the example extension. Takes the `StudioApi` facade so the host
 * command can navigate via `dispatch("editor.reveal", …)` — the mount
 * accepts a `(api) => StudioExtensions` factory for exactly this.
 */
export function createExampleExtension(api: StudioApi): StudioExtensions {
  return {
    toolWindows: [
      {
        id: EXAMPLE_TOOL_WINDOW_ID,
        title: "Host Functions",
        icon: EXAMPLE_ICON,
        defaultPlacement: { dock: "right", section: "end" },
        defaultOpen: false,
        component: HostFunctionsPanel,
      },
    ],
    commands: [
      {
        id: EXAMPLE_REVEAL_COMMAND_ID,
        title: "Example Host: Go to Story Entry",
        run: () => {
          api.dispatch("editor.reveal", {
            kind: "source",
            file: "main.ink",
            span: { start: 0, end: 0 },
          });
        },
      },
    ],
    argumentWidgets: [EXAMPLE_REGION_WIDGET, EXAMPLE_MAP_POINT_WIDGET],
  };
}
