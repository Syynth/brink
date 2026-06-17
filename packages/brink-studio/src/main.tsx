/**
 * Standalone app entry — the playground (dev server, e2e, and the embedded
 * dist-embed build) is itself an embedding host: it calls mountStudio with
 * demo files and the example host extension (spec §8), exactly like an
 * external embedder would.
 */

import { mountStudio, type StudioHandle } from "./mount.js";
import type { FileChange } from "@brink/ink-editor";
import { createExampleExtension, EXAMPLE_HOST_MANIFEST } from "./example-extension.js";
import toppledTemple from "./stories/toppled-temple.ink.txt?raw";

const MAIN_INK = `INCLUDE toppled-temple.ink

// Host functions — provided by the playground's pretend host. Declared
// here ("already defined"); their signatures, types, and docs come from
// the host-capability manifest registered at mount. The Host Functions
// panel browses that manifest and inserts call sites.
EXTERNAL has_item(item)
EXTERNAL gain_gold(amount)
EXTERNAL play_se(name)
EXTERNAL show_picture(name, x, y)
EXTERNAL party_size()
EXTERNAL set_tint(color)
EXTERNAL go_region(region)
EXTERNAL teleport(map, x, y)

-> intro

// Argument-widget demo. \`color\` (hex_color) is a studio built-in widget; the
// filled call shows a swatch (Edit), the empty one a \`‹color›\` ghost (Fill).
// \`region\` (region_id) is a HOST widget: a chip + a host popover picker.
// teleport's \`map\` (map_id) is a value-list — the studio renders a name
// dropdown in the Form + an inline name label. Its (x, y) is an ARG-GROUP
// host widget: one chip over both args, a MODAL map picker taking the \`map\`
// arg as context. In the Form you pick a map, then a spot on that map.
=== palette ===
~ set_tint("#FF8800")
~ set_tint()
~ go_region("harbor")
~ teleport(5, 4, 3)
-> DONE
`;

// Deterministic single-file project for e2e, loaded via `?fixture=screenplay`.
// This decouples the binder/decorations/stitches specs from the demo default
// above (which is multi-file and has no top-level knots). Not used in normal
// app usage — only when the query param is present.
const SCREENPLAY_FIXTURE = `// A short screenplay-style demo.
-> opening

=== opening ===
The lights dim.
A figure steps into the light.
-> evidence

=== interrogation ===
= evidence
"Where were you that night?"
-> END
`;

// Deterministic multi-file project with a nested folder, loaded via
// `?fixture=nested` — for the binder file-lifecycle e2e (move a file into a
// folder, rename a folder). `main.ink` INCLUDEs both a nested file and a root
// file, so a move/rename must rewrite the referrer's INCLUDE and still compile.
const NESTED_FIXTURE: Record<string, string> = {
  "main.ink": "INCLUDE scenes/intro.ink\nINCLUDE helper.ink\nINCLUDE util.ink\n-> intro\n",
  "scenes/intro.ink": "=== intro ===\nThe intro scene.\n-> helper\n",
  "helper.ink": "=== helper ===\nDone.\n-> util\n",
  "util.ink": "=== util ===\n-> END\n",
};

// ── Bootstrap ──────────────────────────────────────────────────

// HMR guard (dev only). Under Vite HMR an update that reaches this entry
// re-executes the whole module, so without a guard each edit stacks another
// mount on #app and orphans the previous wasm EditorSession. The old
// instance's dispose hook unmounts its root *before* the new instance mounts
// (Root's unmount effect already disposes the player and frees the wasm
// session — HMR just never triggered it). The generation counter lets a
// superseded main() — disposed while still awaiting the mount — tear its
// instance down instead of leaving a second root.
interface HotData {
  generation?: number;
  teardown?: () => void;
}
const hotData = import.meta.hot?.data as HotData | undefined;
const generation = hotData ? (hotData.generation = (hotData.generation ?? 0) + 1) : 0;

function superseded(): boolean {
  return hotData !== undefined && hotData.generation !== generation;
}

async function main(): Promise<void> {
  const params = new URLSearchParams(window.location.search);

  // `?fixture=screenplay` loads a deterministic single-file project for e2e.
  const fixture = params.get("fixture");
  const files: Record<string, string> =
    fixture === "screenplay"
      ? { "main.ink": SCREENPLAY_FIXTURE }
      : fixture === "nested"
        ? NESTED_FIXTURE
        : {
            "main.ink": MAIN_INK,
            "toppled-temple.ink": toppledTemple,
          };

  const appRoot = document.getElementById("app");
  if (!appRoot) throw new Error("Missing #app container");

  // The example host extension (spec §8) ships with the playground —
  // `?ext=none` loads without it (the e2e removed-extension scenario; also
  // exercises that persisted layouts referencing its panel load cleanly).
  const withExtension = params.get("ext") !== "none";

  // `?egress=test` attaches an onFilesChanged hook recording delivered
  // batches on `window.__brinkFileChanges` (e2e for #154). The normal
  // playground stays hookless — it is the "host without persistence" case,
  // where dirty state only clears on explicit file.save / file.saveAll.
  const recordEgress = params.get("egress") === "test";
  const onFilesChanged = recordEgress
    ? (changes: FileChange[]): void => {
        const w = window as unknown as { __brinkFileChanges?: FileChange[][] };
        (w.__brinkFileChanges ??= []).push(changes);
      }
    : undefined;

  const handle: StudioHandle = await mountStudio(appRoot, {
    files,
    entryFile: "main.ink",
    extensions: withExtension ? createExampleExtension : undefined,
    // The pretend host's capability manifest (the panel renders the same
    // object). Registered regardless of `?ext=none` — the host's vocabulary
    // exists whether or not its UI extension is mounted.
    hostManifest: EXAMPLE_HOST_MANIFEST,
    onFilesChanged,
  });
  if (superseded()) {
    handle.unmount();
    return;
  }

  const loading = document.getElementById("loading");
  if (loading) loading.remove();

  if (hotData) {
    // Unmounting runs Root's cleanup effect: dispose session + views + project.
    hotData.teardown = () => handle.unmount();
  }
}

main();

if (import.meta.hot) {
  import.meta.hot.dispose((data: HotData) => {
    data.teardown?.();
    data.teardown = undefined;
  });
}
