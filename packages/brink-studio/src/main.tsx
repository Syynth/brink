/**
 * Standalone app entry — the playground (dev server, e2e, and the embedded
 * dist-embed build) is itself an embedding host: it calls mountStudio with
 * demo files and the example host extension (spec §8), exactly like an
 * external embedder would.
 */

import { mountStudio, type StudioHandle } from "./mount.js";
import { InMemoryFileProvider, type FileChange } from "@brink-lang/editor";
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
-> interrogation.evidence

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

// Native-surface project, loaded via `?fixture=native` — the first `.brink`
// content the playground has ever seeded. Exists so the native editor
// surfaces can be *seen* rather than only asserted: the analysis half
// (hover, navigation, rename, signature, completions, symbols, folding) has
// worked on `.brink` for weeks with no way to look at it (#1131, hold lifted
// 2026-08-05). Two files, because the cross-file half is the interesting
// part — `pub` visibility (#1582) and per-project extent (#1580) only mean
// anything with a second file to reference.
// ⚠ THIS FIXTURE DOES NOT FULLY COMPILE TODAY, ON PURPOSE. It is written to
// the RULED model, not to whatever currently happens to work, so the studio
// shows the real gaps instead of hiding them:
//
//   - Conventions live in their OWN module named by `brink.toml`, per §9.1
//     item 4. They currently claim nothing outside their own file (#2289) —
//     `VENDOR` in story.brink renders unclaimed. DO NOT "fix" this by
//     inlining the handlers into story.brink; that hides the defect, and the
//     single-file tier1-native fixture already covers the inline shape.
//   - `-> barter::haggle` is the intended module-qualified divert and does
//     not resolve (#2287). DO NOT swap it for bare `-> haggle`, which happens
//     to compile but is itself wrong — bare names require a symbol or glob
//     import, not a module import.
//
// When #2287 and #2289 land, this fixture should go green on its own. That is
// the point: it is a live acceptance test you can look at.
const NATIVE_FIXTURE: Record<string, string> = {
  "brink.toml": '[project]\nentry = "story.brink"\nconventions = "conventions.brink"\n',
  "conventions.brink": `struct Cue {
  speaker: string,
}

@[convention(claims = "^(?<name>[A-Z][A-Z '-]*)$", attach = Cue, order = 10)]
fn cue(name: string): Cue {
  return Cue { speaker: name };
}

@[convention(claims = "^(?<kind>INT|EXT)\\\\. (?<title>.+)$", order = 20)]
fn heading(kind: string, title: string) {
  return "-- {kind}. {title} --";
}
`,
  "story.brink": `use story::market::barter;

pub flow main() {
  INT. MARKET SQUARE - NIGHT
  The square is empty.

  VENDOR
  You shouldn't be here after dark.

  -> barter::haggle
}
`,
  "market/barter.brink": `pub flow haggle() {
  KID
  How much for the lantern?
  -> DONE
}
`,
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
        : fixture === "native"
          ? NATIVE_FIXTURE
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

  // The playground owns its provider so the external-conflict merge view
  // (#320, Track V) is verifiable without a real filesystem: a watcher would
  // call `pushExternalChange`; here `window.__brinkSimulateExternalChange`
  // does, exercising the conflict detection → kept buffer → merge surface.
  const provider = new InMemoryFileProvider(files);
  const w = window as unknown as {
    __brinkSimulateExternalChange?: (path: string, content: string | null) => void;
  };
  w.__brinkSimulateExternalChange = (path, content) =>
    provider.pushExternalChange(path, content);

  const handle: StudioHandle = await mountStudio(appRoot, {
    files,
    provider,
    // The native fixture has no `main.ink` — hardcoding one opened a phantom
    // tab for a file outside the project, which then reported an error the
    // Problems panel could not show (mistaken for #2281 until traced here).
    entryFile: fixture === "native" ? "story.brink" : "main.ink",
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
