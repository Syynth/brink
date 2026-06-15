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
import type { HostManifest } from "@brink/wasm-types";

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
  /** The call snippet inserted on click — a call site only, no `EXTERNAL`. */
  call: string;
  /** The manifest doc comment ("" when the entry carries none). */
  doc: string;
  /** The manifest kind tag ("query" | "effect" | "presentation" | "plain"). */
  kind: string;
}

/**
 * Derive the panel's rows from a host-capability manifest. Pure — the panel
 * renders exactly what the manifest registered, nothing else.
 */
export function manifestPanelItems(manifest: HostManifest): HostFunctionItem[] {
  return (manifest.externals ?? []).map((ext) => {
    const params = ext.params ?? [];
    const sigParams = params
      .map((p) => (p.ty !== undefined ? `${p.name}: ${p.ty}` : p.name))
      .join(", ");
    const returns =
      ext.returns !== undefined && ext.returns !== "void" ? ` -> ${ext.returns}` : "";
    const args = params.map((p) => p.name).join(", ");
    return {
      name: ext.name,
      signature: `${ext.name}(${sigParams})${returns}`,
      call: `~ ${ext.name}(${args})\n`,
      doc: ext.doc ?? "",
      kind: ext.kind ?? "plain",
    };
  });
}

// ── Panel component ─────────────────────────────────────────────────

function HostFunctionsPanel() {
  const api = useStudioApi();
  const items = manifestPanelItems(EXAMPLE_HOST_MANIFEST);

  const insert = (item: HostFunctionItem): void => {
    api.insertText(item.call);
    api.notify({
      severity: "info",
      source: "example host",
      message: `Inserted ${item.call.trim().slice(2)}`,
    });
  };

  return (
    <div className="host-example-panel" style={{ padding: 8, overflow: "auto", height: "100%" }}>
      <p style={{ margin: "0 0 8px", fontSize: 12, color: "var(--bs-fg-muted)" }}>
        Functions the host provides (already declared in the story) — click to
        insert a call at the cursor.
      </p>
      <ul style={{ listStyle: "none", margin: 0, padding: 0 }}>
        {items.map((item) => (
          <li key={item.name} style={{ marginBottom: 4 }}>
            <button
              type="button"
              className="host-example-fn"
              onClick={() => insert(item)}
              title={`${item.kind} — inserts ${item.call.trim()}`}
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
  };
}
