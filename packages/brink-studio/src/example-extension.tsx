/**
 * Example host extension (docs/studio-shell-spec.md §8, issue #95).
 *
 * The worked example validating the RPG Maker MZ use case: a host embedding
 * the studio registers a "Host Functions" tool window listing its external
 * vocabulary, with click-to-insert of `EXTERNAL` declarations + call
 * snippets through the StudioApi facade. A real host would render the
 * host-capability manifest it already registers via `set_host_manifest`
 * (docs/host-capability-manifest.md — Track B synergy); this example uses a
 * small static list of pretend functions.
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

export const EXAMPLE_TOOL_WINDOW_ID = "host.example.functions";
export const EXAMPLE_REVEAL_COMMAND_ID = "host.example.revealStart";

// ── Pretend host vocabulary ─────────────────────────────────────────
//
// A real host derives this from its capability manifest; the shapes mirror
// the RPG Maker-style verbs from docs/host-capability-manifest.md.

interface HostFunction {
  name: string;
  params: string[];
  doc: string;
}

const HOST_FUNCTIONS: HostFunction[] = [
  { name: "has_item", params: ["item_id"], doc: "Whether the party holds an item." },
  { name: "gain_gold", params: ["amount"], doc: "Give the party gold." },
  { name: "play_se", params: ["name"], doc: "Play a sound effect." },
  { name: "show_picture", params: ["name", "x", "y"], doc: "Show a picture." },
];

/** The `EXTERNAL` declaration + call snippet inserted for a function. */
export function functionSnippet(fn: HostFunction): string {
  const params = fn.params.join(", ");
  return `EXTERNAL ${fn.name}(${params})\n~ ${fn.name}(${params})\n`;
}

// ── Panel component ─────────────────────────────────────────────────

function HostFunctionsPanel() {
  const api = useStudioApi();

  const insert = (fn: HostFunction): void => {
    api.insertText(functionSnippet(fn));
    api.notify({
      severity: "info",
      source: "example host",
      message: `Inserted ${fn.name}(${fn.params.join(", ")})`,
    });
  };

  return (
    <div className="host-example-panel" style={{ padding: 8, overflow: "auto", height: "100%" }}>
      <p style={{ margin: "0 0 8px", fontSize: 12, color: "var(--bs-fg-muted)" }}>
        Pretend host functions — click to insert an EXTERNAL declaration and
        call at the cursor.
      </p>
      <ul style={{ listStyle: "none", margin: 0, padding: 0 }}>
        {HOST_FUNCTIONS.map((fn) => (
          <li key={fn.name} style={{ marginBottom: 4 }}>
            <button
              type="button"
              className="host-example-fn"
              onClick={() => insert(fn)}
              title={fn.doc}
              style={{
                display: "block",
                width: "100%",
                textAlign: "left",
                padding: "4px 6px",
                font: "inherit",
                fontFamily: "monospace",
                fontSize: 12,
                color: "var(--bs-fg)",
                background: "var(--bs-surface-bg)",
                border: "1px solid var(--bs-border)",
                borderRadius: 4,
                cursor: "pointer",
              }}
            >
              {fn.name}({fn.params.join(", ")})
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
