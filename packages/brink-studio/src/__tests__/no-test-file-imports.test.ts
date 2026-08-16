import {
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { basename, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

// Workspace-wide guard against importing from test files (#2516).
//
// Vitest re-registers a .test.ts file's describe/it blocks in the importing
// file — measured in PR #2510: six tests ran twice (22 where 16 expected),
// silently passing under the enrolment suite's name. The incident importer
// (packages/brink-studio/src/__tests__/save-path-enrolment.test.ts) and the
// file it imported (save-retire-invariant.test.ts) were BOTH inside
// __tests__/, so this guard must scan __tests__/ files too, not just the
// package's non-test sources. The fix extracted a plain module
// (save-paths.ts) instead.
//
// This guard scans every packages/*/src file (test directories included) for
// an import from a *.test.ts(x) sibling, via a static `from "..."`, a
// dynamic `import("...")`, or `vi.importActual("...")` — all three
// re-register the target file's tests identically. A file that needs a
// fixture or registry defined in another test should extract it to a plain
// module (no .test.ts suffix) and import that instead.
//
// Rule mirrored in CLAUDE.md § Rules and
// .claude/skills/autonomous-pump/BRINK-CONFIG.md "House rules". Named as the
// guard on save-retire-invariant.test.ts and save-paths.ts, following the
// #2450 precedent (packages/brink-studio/tsconfig.json /
// vitest.config.ts naming their own guard).

const packagesDir = resolve(fileURLToPath(import.meta.url), "../../../../");

// Matches a specifier ending in `.test`, optionally followed by a `.ts`,
// `.tsx`, `.js`, or `.jsx` extension — the repo's own imports are all
// extension-`.js` (moduleResolution "bundler" + no allowImportingTsExtensions
// means `from "./x.test.ts"` is a TS5097 compile error and never appears),
// so the extension must be optional to catch the actual incident form
// (`from "./x.test.js"`) as well as an extensionless `from "./x.test"`.
const TEST_FILE_IMPORT_PATTERNS: readonly RegExp[] = [
  /from\s+["']([^"']*\.test(?:\.[jt]sx?)?)["']/,
  // Excludes a JSDoc `{@link import("./x.test.js")}` type-reference — that's
  // a doc comment, not executable code, and workspace-roots.ts legitimately
  // uses one to point at save-path-enrolment.test.ts without re-registering it.
  /(?<!@link\s)\bimport\(\s*["']([^"']*\.test(?:\.[jt]sx?)?)["']\s*\)/,
  /\bvi\.importActual\(\s*["']([^"']*\.test(?:\.[jt]sx?)?)["']\s*\)/,
];

// This guard's own source necessarily contains example specifiers (in
// comments explaining the pattern, and in fixture strings the end-to-end
// test below writes out) that would otherwise self-match — exempt it rather
// than contort the regex into comment/string-literal awareness it can't
// have without a real parser.
const GUARD_FILE_BASENAME = "no-test-file-imports.test.ts";

function listPackages(dir: string): string[] {
  return readdirSync(dir)
    .filter((name) => {
      const fullPath = join(dir, name);
      try {
        const stat = statSync(fullPath);
        return stat.isDirectory() && !name.startsWith(".");
      } catch {
        return false;
      }
    })
    .sort();
}

function listTypeScriptFiles(srcDir: string): string[] {
  const files: string[] = [];

  function scan(dir: string) {
    try {
      const entries = readdirSync(dir, { withFileTypes: true });
      for (const entry of entries) {
        if (entry.isDirectory()) {
          scan(join(dir, entry.name));
        } else if (
          entry.isFile() &&
          (entry.name.endsWith(".ts") || entry.name.endsWith(".tsx"))
        ) {
          files.push(join(dir, entry.name));
        }
      }
    } catch {
      // Skip directories that don't exist or can't be read
    }
  }

  scan(srcDir);
  return files.sort();
}

function findTestFileImports(filePath: string): string[] {
  try {
    const source = readFileSync(filePath, "utf8");
    const imports: string[] = [];

    for (const line of source.split("\n")) {
      for (const pattern of TEST_FILE_IMPORT_PATTERNS) {
        const match = line.match(pattern);
        if (match) imports.push(match[1]);
      }
    }

    return imports;
  } catch {
    return [];
  }
}

interface Offense {
  file: string;
  specifier: string;
}

/** Scans every package's src tree (no __tests__/ carve-out) for the trap. */
function scanPackagesForTestFileImports(root: string): Offense[] {
  const offenses: Offense[] = [];
  for (const pkg of listPackages(root)) {
    const srcDir = join(root, pkg, "src");
    for (const file of listTypeScriptFiles(srcDir)) {
      if (basename(file) === GUARD_FILE_BASENAME) continue;
      for (const specifier of findTestFileImports(file)) {
        offenses.push({ file, specifier });
      }
    }
  }
  return offenses;
}

describe("packages/*/src must not import from .test.ts files (#2516)", () => {
  it("resolves packagesDir to the repo's packages/ directory", () => {
    // A moved guard file that silently mislocates packagesDir would scan
    // nothing and stay green — assert the exact directory name, not merely
    // that some package was found.
    expect(basename(packagesDir)).toBe("packages");
  });

  it("no package under packages/*/src imports from a .test.ts(x) file", () => {
    const offenses = scanPackagesForTestFileImports(packagesDir);
    expect(
      offenses,
      offenses.map((o) => `${o.file} imports "${o.specifier}"`).join("\n"),
    ).toEqual([]);
  });

  describe("findTestFileImports", () => {
    function withFixture(content: string, run: (filePath: string) => void): void {
      const tmp = mkdtempSync(join(tmpdir(), "find-test-file-imports-"));
      try {
        const filePath = join(tmp, "fixture.ts");
        writeFileSync(filePath, content);
        run(filePath);
      } finally {
        rmSync(tmp, { recursive: true, force: true });
      }
    }

    it("detects a static import of a .test.js file (the measured incident form)", () => {
      withFixture(
        'import { SAVE_PATHS } from "./save-retire-invariant.test.js";\n',
        (filePath) => {
          expect(findTestFileImports(filePath)).toEqual([
            "./save-retire-invariant.test.js",
          ]);
        },
      );
    });

    it("detects a .test.tsx import", () => {
      withFixture('import { Helper } from "../helpers.test.tsx";\n', (filePath) => {
        expect(findTestFileImports(filePath)).toEqual(["../helpers.test.tsx"]);
      });
    });

    it("detects an extensionless .test import", () => {
      withFixture('import { Helper } from "../helpers.test";\n', (filePath) => {
        expect(findTestFileImports(filePath)).toEqual(["../helpers.test"]);
      });
    });

    it("detects a dynamic import() of a .test.js file", () => {
      withFixture('const mod = await import("./fixture.test.js");\n', (filePath) => {
        expect(findTestFileImports(filePath)).toEqual(["./fixture.test.js"]);
      });
    });

    it("detects vi.importActual() of a .test.js file", () => {
      withFixture(
        'const actual = await vi.importActual("./fixture.test.js");\n',
        (filePath) => {
          expect(findTestFileImports(filePath)).toEqual(["./fixture.test.js"]);
        },
      );
    });

    it("does not flag a regular .js import (the repo's own import convention)", () => {
      withFixture('import { SAVE_PATHS } from "./save-paths.js";\n', (filePath) => {
        expect(findTestFileImports(filePath)).toEqual([]);
      });
    });

    it("does not flag a plain module import", () => {
      withFixture('import { Helper } from "./helper";\n', (filePath) => {
        expect(findTestFileImports(filePath)).toEqual([]);
      });
    });

    it("does not flag a node_modules import", () => {
      withFixture('import { describe, it } from "vitest";\n', (filePath) => {
        expect(findTestFileImports(filePath)).toEqual([]);
      });
    });
  });

  describe("scanPackagesForTestFileImports (end-to-end fixture)", () => {
    it("flags an importer inside __tests__/ and ignores a clean sibling", () => {
      // Reproduces the #2510 incident shape: the importer AND the imported
      // .test.ts file both live inside __tests__/, which the old
      // isInTestDirectory() carve-out would have skipped entirely.
      const tmp = mkdtempSync(join(tmpdir(), "no-test-file-imports-fixture-"));
      try {
        const testsDir = join(tmp, "fixture-pkg", "src", "__tests__");
        mkdirSync(testsDir, { recursive: true });

        writeFileSync(
          join(testsDir, "importer.test.ts"),
          'import { X } from "./sibling.test.js";\n',
        );
        writeFileSync(join(testsDir, "sibling.test.ts"), "export const X = 1;\n");
        // A clean sibling at the same nesting, importing a plain module:
        // must not be flagged.
        writeFileSync(join(testsDir, "clean.test.ts"), 'import { X } from "./plain.js";\n');
        writeFileSync(join(tmp, "fixture-pkg", "src", "plain.ts"), "export const X = 1;\n");

        const offenses = scanPackagesForTestFileImports(tmp);
        expect(offenses).toEqual([
          {
            file: join(testsDir, "importer.test.ts"),
            specifier: "./sibling.test.js",
          },
        ]);
      } finally {
        rmSync(tmp, { recursive: true, force: true });
      }
    });
  });
});
