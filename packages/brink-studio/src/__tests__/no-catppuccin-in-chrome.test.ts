/**
 * Theme-agnostic chrome must not reach for a raw `--ctp-*` palette token.
 *
 * The trap is not a missing variable — it is a variable that is always
 * there and never changes. `mocha.css` is the BARE-CLASS default
 * (`.brink-studio { … }`), so it defines the whole Catppuccin palette for
 * every theme; the other themes are `[data-theme="…"]` scopes that
 * override only the semantic `--bs-*` layer above it. A sheet reading
 * `var(--ctp-base)` therefore resolves — to mocha's `#1e1e2e` — under
 * inky, inky-dark, latte and manuscript alike, including the LIGHT ones,
 * where a dark blue-grey surface is simply wrong.
 *
 * That is invisible in review (the value is defined, the fallback never
 * fires, nothing errors) and invisible in the mocha-themed screenshot
 * everyone looks at. It shipped in the Settings rail and the toggle track
 * and was reported as "a lot of the settings UI just doesn't respond to
 * theme changes" (#3294), which is exactly what it is.
 *
 * The theme files themselves are exempt: defining and mapping the palette
 * is their job. Everything else states its colours in `--bs-*`.
 */
import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// Same idiom as `no-test-file-imports.test.ts`, the other repo-scanning
// test in this suite — a `new URL(...)` base does not resolve to a file
// path through vite's module graph.
const PACKAGES = resolve(fileURLToPath(import.meta.url), "../../../../");

/** Every `.css` under a package's own `src` tree, minus theme definitions. */
function chromeStylesheets(): string[] {
  const out: string[] = [];
  const walk = (dir: string): void => {
    for (const entry of readdirSync(dir)) {
      const full = join(dir, entry);
      if (statSync(full).isDirectory()) {
        if (entry === "node_modules" || entry === "themes") continue;
        walk(full);
        continue;
      }
      if (entry.endsWith(".css")) out.push(full);
    }
  };
  for (const pkg of readdirSync(PACKAGES)) {
    const src = join(PACKAGES, pkg, "src");
    try {
      if (statSync(src).isDirectory()) walk(src);
    } catch {
      // package without a src/ — nothing to scan
    }
  }
  return out;
}

/** The theme definitions — exempt from the --ctp- rule, but they are where
 *  `--bs-*` tokens are declared, so the second check reads them. */
function themeStylesheets(): string[] {
  const dir = join(PACKAGES, "studio-shell", "src", "styles", "themes");
  return readdirSync(dir)
    .filter((f) => f.endsWith(".css"))
    .map((f) => join(dir, f));
}

describe("theme-agnostic chrome", () => {
  it("scans a real, non-empty set of stylesheets", () => {
    // Guard the guard: a walk that silently found nothing would pass the
    // assertion below forever.
    const sheets = chromeStylesheets();
    expect(sheets.length).toBeGreaterThan(10);
    expect(sheets.some((f) => f.endsWith("settings.css"))).toBe(true);
  });

  it("never reads a raw --ctp-* palette token", () => {
    const offenders: string[] = [];
    for (const file of chromeStylesheets()) {
      const source = readFileSync(file, "utf8");
      source.split("\n").forEach((line, i) => {
        // `var(--ctp-…)` in a declaration. Prose in a comment explaining
        // this very rule is not a use.
        if (!/var\(\s*--ctp-/.test(line)) return;
        const beforeUse = line.slice(0, line.indexOf("var("));
        if (beforeUse.includes("/*") || beforeUse.trimStart().startsWith("*")) return;
        offenders.push(`${relative(PACKAGES, file)}:${i + 1}: ${line.trim()}`);
      });
    }
    expect(
      offenders,
      `A --ctp-* token resolves to mocha's value under EVERY theme, so this\n` +
        `paints one fixed colour that no theme can move. State the colour as a\n` +
        `--bs-* token instead:\n\n${offenders.join("\n")}\n`,
    ).toEqual([]);
  });

  /**
   * The same defect wearing a different hat.
   *
   * `var(--bs-bg, #1e1e2e)` looks defensive and is not: no theme defines
   * `--bs-bg`, so the literal is not a fallback, it is THE value — one
   * fixed Catppuccin colour under every theme, including the light ones.
   * That is what put dark dropdowns in the middle of latte's Settings.
   *
   * A fallback naming another token — `var(--bs-input-bg,
   * var(--bs-surface-bg))` — is the opposite and stays: it is an optional
   * override that still resolves to something the theme controls.
   */
  it("never falls back to a literal colour for a token no theme defines", () => {
    const defined = new Set<string>();
    for (const file of chromeStylesheets().concat(themeStylesheets())) {
      for (const m of readFileSync(file, "utf8").matchAll(/^\s*(--bs-[a-z0-9-]+)\s*:/gm)) {
        defined.add(m[1]);
      }
    }
    const literal = String.raw`#[0-9a-fA-F]{3,8}|\d{1,3} \d{1,3} \d{1,3}`;
    const pattern = new RegExp(String.raw`var\(\s*(--bs-[a-z0-9-]+)\s*,\s*(${literal})\s*\)`, "g");
    const offenders: string[] = [];
    for (const file of chromeStylesheets()) {
      const source = readFileSync(file, "utf8");
      source.split("\n").forEach((line, i) => {
        for (const m of line.matchAll(pattern)) {
          if (defined.has(m[1])) continue; // dead fallback: harmless
          offenders.push(`${relative(PACKAGES, file)}:${i + 1}: ${m[0]}`);
        }
      });
    }
    expect(
      offenders,
      `No theme defines these tokens, so the literal IS the colour — fixed\n` +
        `under every theme. Define the token per theme, or read one that\n` +
        `already exists:\n\n${offenders.join("\n")}\n`,
    ).toEqual([]);
  });
});
