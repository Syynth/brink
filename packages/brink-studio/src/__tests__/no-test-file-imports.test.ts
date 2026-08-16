import { readFileSync, readdirSync, statSync } from "node:fs";
import { resolve, join } from "node:path";
import { describe, expect, it } from "vitest";
import { fileURLToPath } from "node:url";

// Workspace-wide guard against importing from test files (#2516).
//
// Vitest re-registers a .test.ts file's describe/it blocks in the importing
// file — measured in PR #2510: six tests ran twice (22 where 16 expected),
// silently passing under the enrolment suite's name. The fix extracted a
// plain module instead. The underlying trap is undocumented and unenforced.
//
// This guard scans packages/*/src for any import from a *.test.ts file
// outside the importing file's own __tests__/ directory. A file that needs
// a fixture or registry defined in another test should extract it to a plain
// module (no .test.ts suffix) and import that instead.
//
// The exact shape tested: `from ".../*.test.ts"` or `from ".../*.test.tsx"`
// — this catches the leading source of the problem (direct test-file imports)
// while remaining narrow enough to have real signal.

const packagesDir = resolve(
  fileURLToPath(import.meta.url),
  "../../../../"
);

// Pattern to match imports from .test.ts or .test.tsx files.
// Matches: from "path/to/file.test.ts" or from "../file.test.tsx"
// Does not match: from "./file" or from "module.ts"
const TEST_FILE_IMPORT_PATTERN = /from\s+["']([^"']*\.test\.tsx?)["']/;

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

function isInTestDirectory(filePath: string): boolean {
  const parts = filePath.split(/[/\\]/);
  return parts.includes("__tests__");
}

function findTestFileImports(filePath: string): string[] {
  try {
    const source = readFileSync(filePath, "utf8");
    const imports: string[] = [];

    // Split by lines and scan for import statements
    const lines = source.split("\n");
    for (const line of lines) {
      const match = line.match(TEST_FILE_IMPORT_PATTERN);
      if (match) {
        imports.push(match[1]);
      }
    }

    return imports;
  } catch {
    return [];
  }
}

describe("packages/*/src must not import from .test.ts files (#2516)", () => {
  const packages = listPackages(packagesDir);

  it("finds at least one package to check (test infrastructure is working)", () => {
    expect(packages.length).toBeGreaterThan(0);
  });

  for (const pkg of packages) {
    const srcDir = join(packagesDir, pkg, "src");
    const files = listTypeScriptFiles(srcDir);

    // Only test packages that have a src directory with TS files
    if (files.length === 0) continue;

    describe(`${pkg}/src`, () => {
      for (const file of files) {
        // Files inside __tests__/ are allowed to import from test files,
        // since they are test files themselves
        if (isInTestDirectory(file)) continue;

        const relPath = file.replace(srcDir + "/", "");

        it(`${relPath} does not import from .test.ts files`, () => {
          const imports = findTestFileImports(file);
          expect(imports).toEqual([]);
        });
      }
    });
  }

  // Negative tests proving the guard can actually fail
  describe("findTestFileImports", () => {
    it("detects a direct .test.ts import", () => {
      const source = 'import { SAVE_PATHS } from "./save-retire-invariant.test.ts";';
      const match = source.match(TEST_FILE_IMPORT_PATTERN);
      expect(match).not.toBeNull();
      expect(match?.[1]).toBe("./save-retire-invariant.test.ts");
    });

    it("detects a .test.tsx import", () => {
      const source = 'import { Helper } from "../helpers.test.tsx";';
      const match = source.match(TEST_FILE_IMPORT_PATTERN);
      expect(match).not.toBeNull();
      expect(match?.[1]).toBe("../helpers.test.tsx");
    });

    it("does not flag a regular .ts import", () => {
      const source = 'import { SAVE_PATHS } from "./save-paths.ts";';
      const match = source.match(TEST_FILE_IMPORT_PATTERN);
      expect(match).toBeNull();
    });

    it("does not flag a plain module import", () => {
      const source = 'import { Helper } from "./helper";';
      const match = source.match(TEST_FILE_IMPORT_PATTERN);
      expect(match).toBeNull();
    });

    it("does not flag a node_modules import", () => {
      const source = 'import { describe, it } from "vitest";';
      const match = source.match(TEST_FILE_IMPORT_PATTERN);
      expect(match).toBeNull();
    });
  });
});
