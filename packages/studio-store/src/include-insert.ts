/**
 * Pure helpers for the out-of-scope banner's "Add INCLUDE" action (#3017):
 * compute the INCLUDE path an entry file needs to reach a target, and
 * insert the INCLUDE line into the entry's source.
 *
 * Kept out of the slice so both are unit-testable without a store.
 */

/** Directory of a project-relative `/`-separated path ("" for root). */
function dirOf(path: string): string {
  const idx = path.lastIndexOf("/");
  return idx < 0 ? "" : path.slice(0, idx);
}

/**
 * The path an `INCLUDE` in `entryPath` must name to reach `targetPath` —
 * ink resolves INCLUDEs relative to the INCLUDING file
 * (`brink_db::resolve_include_path`), so this walks up from the entry's
 * directory with `../` segments as needed. Both arguments are
 * project-relative `/`-separated keys (the provider convention).
 */
export function relativeIncludePath(entryPath: string, targetPath: string): string {
  const from = dirOf(entryPath) === "" ? [] : dirOf(entryPath).split("/");
  const to = targetPath.split("/");
  let common = 0;
  while (common < from.length && common < to.length - 1 && from[common] === to[common]) {
    common++;
  }
  const ups = from.length - common;
  return `${"../".repeat(ups)}${to.slice(common).join("/")}`;
}

/**
 * Insert `INCLUDE ${includePath}` into `source`: after the last existing
 * INCLUDE line when there is one (keeping the include block together), at
 * the very top otherwise (ink convention — includes lead the file).
 * Returns `source` unchanged when an identical INCLUDE already exists.
 */
export function insertIncludeLine(source: string, includePath: string): string {
  const line = `INCLUDE ${includePath}`;
  const lines = source.split("\n");
  let lastInclude = -1;
  for (const [i, l] of lines.entries()) {
    const trimmed = l.trim();
    if (trimmed === line) return source;
    if (trimmed.startsWith("INCLUDE ")) lastInclude = i;
  }
  lines.splice(lastInclude + 1, 0, line);
  return lines.join("\n");
}
