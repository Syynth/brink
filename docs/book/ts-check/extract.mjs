// Extract the book's TypeScript examples into standalone .ts files so `tsc`
// can type-check them against the published @brink-lang packages.
//
// This is the TS counterpart of `mdbook test` for the Rust examples: the Rust
// blocks are compiled by rustdoc, the TS blocks by tsc here. mdbook itself
// only knows how to run Rust doctests, so this harness fills the gap.
//
// Rules, mirroring the Rust convention (```rust,ignore):
//
//   - ```ts  /  ```typescript          → extracted and type-checked
//   - ```ts,no-check                    → skipped (logged), for blocks that
//                                         cannot type-check standalone — e.g.
//                                         raw `brink-web` (wasm-pack) module
//                                         usage, which is not a published npm
//                                         package. Must be a deliberate,
//                                         logged exception, never the default.
//
// Hidden setup. mdbook hides `#`-prefixed lines only in Rust blocks, so a TS
// block can't hide imports/`declare`s the way a Rust one can. Instead, put them
// in an HTML comment immediately before the fence — mdbook doesn't render HTML
// comments, and this script prepends the comment's body to the snippet:
//
//   <!-- ts-hidden
//   import { StoryRunnerHandle } from "@brink-lang/web";
//   declare const bytes: Uint8Array;
//   -->
//   ```ts
//   const runner = new StoryRunnerHandle(bytes);
//   ```

import { readdirSync, readFileSync, writeFileSync, rmSync, mkdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const here = fileURLToPath(new URL(".", import.meta.url));
const srcRoot = join(here, "..", "src");
const outDir = join(here, "generated");

function walk(dir) {
  const out = [];
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) out.push(...walk(p));
    else if (name.endsWith(".md")) out.push(p);
  }
  return out;
}

// Returns { lang, info, body, hidden } for each fenced block in `text`.
function blocks(text) {
  const lines = text.split("\n");
  const found = [];
  let i = 0;
  while (i < lines.length) {
    const open = lines[i].match(/^```(\S*)\s*$/);
    if (!open) {
      i++;
      continue;
    }
    const info = open[1];
    const start = i + 1;
    let j = start;
    while (j < lines.length && !/^```\s*$/.test(lines[j])) j++;
    const body = lines.slice(start, j).join("\n");

    // A hidden prelude is an HTML comment opening with `ts-hidden`,
    // immediately before the fence (blank lines allowed between).
    let hidden = "";
    let k = i - 1;
    while (k >= 0 && lines[k].trim() === "") k--;
    if (k >= 0 && lines[k].trim() === "-->") {
      let h = k;
      while (h >= 0 && !lines[h].includes("<!--")) h--;
      if (h >= 0) {
        const first = lines[h].replace("<!--", "").trim();
        if (first.startsWith("ts-hidden")) {
          const inner = lines.slice(h + 1, k); // between marker line and `-->`
          hidden = inner.join("\n");
        }
      }
    }

    found.push({ lang: info.split(",")[0], info, body, hidden });
    i = j + 1;
  }
  return found;
}

rmSync(outDir, { recursive: true, force: true });
mkdirSync(outDir, { recursive: true });

let checked = 0;
const skipped = [];

for (const file of walk(srcRoot).sort()) {
  const rel = relative(srcRoot, file);
  const text = readFileSync(file, "utf8");
  const bs = blocks(text);
  bs.forEach((b, n) => {
    if (b.lang !== "ts" && b.lang !== "typescript") return;
    if (b.info.includes("no-check")) {
      skipped.push(`${rel} #${n} (${b.info})`);
      return;
    }
    const slug = rel.replace(/[\/.]/g, "-");
    const parts = [];
    if (b.hidden) parts.push(b.hidden);
    parts.push(b.body);
    // Force module scope: separate files don't share globals, and top-level
    // `await` needs a module. `export {}` is a no-op that makes it one.
    parts.push("export {};");
    writeFileSync(join(outDir, `${slug}-${n}.ts`), parts.join("\n") + "\n");
    checked++;
  });
}

console.log(`ts-check: extracted ${checked} snippet(s) for type-checking`);
if (skipped.length) {
  console.log(`ts-check: skipped ${skipped.length} no-check block(s):`);
  for (const s of skipped) console.log(`  - ${s}`);
}
