/**
 * Standalone app entry — the playground (dev server, e2e, and the embedded
 * dist-embed build) is itself an embedding host: it calls mountStudio with
 * demo files and the example host extension (spec §8), exactly like an
 * external embedder would.
 */

import { mountStudio, type StudioHandle } from "./mount.js";
import { createExampleExtension } from "./example-extension.js";
import toppledTemple from "./stories/toppled-temple.ink.txt?raw";

const MAIN_INK = `INCLUDE toppled-temple.ink

-> intro
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

  const handle: StudioHandle = await mountStudio(appRoot, {
    files,
    entryFile: "main.ink",
    extensions: withExtension ? createExampleExtension : undefined,
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
