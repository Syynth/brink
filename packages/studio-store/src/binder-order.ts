/**
 * The `.binder.json` order sidecar (#3038 — compare
 * `docs/design/binder-v2/Order.dc.html`), adapted from the maintainer's
 * celeris binder (spine-space decision 1: "placement is authorship").
 *
 * Pure model only: parsing (corrupt self-heals to the fallback, the
 * recents.json posture), the listed-then-fallback child ordering, the
 * rename re-keying, and reorder application. Storage goes through the
 * host `FileProvider` (the sidecar never enters the wasm session — it is
 * presentation, and `.json` is not a source file by construction).
 *
 * Identity convention (celeris's): child ids are project-relative paths;
 * FOLDER ids carry a trailing `/`; the root container is `""`.
 */

export interface BinderOrder {
  /** Per-container display order: container id ("" = root, else a
   *  folder id) → child ids, in order. Unlisted children follow by the
   *  fallback rule; ids for children that no longer exist are ignored
   *  (and dropped on the next write). */
  order: Record<string, string[]>;
  /** Folders with no files yet — created in-app, they render immediately
   *  and survive reloads (the empty-folder registry, closing #3012's
   *  gap: a file-derived tree cannot represent them otherwise). */
  folders: string[];
}

export const EMPTY_BINDER_ORDER: BinderOrder = { order: {}, folders: [] };

/** The sidecar's project-relative path. */
export const BINDER_SIDECAR_PATH = ".binder.json";

/** Parse sidecar text. Any malformed shape self-heals to the fallback —
 *  display order is cached authorship, not authored content. */
export function parseBinderOrder(text: string | null): BinderOrder {
  if (text === null) return EMPTY_BINDER_ORDER;
  try {
    const raw: unknown = JSON.parse(text);
    if (typeof raw !== "object" || raw === null) return EMPTY_BINDER_ORDER;
    const order: Record<string, string[]> = {};
    const rawOrder = (raw as Record<string, unknown>).order;
    if (typeof rawOrder === "object" && rawOrder !== null) {
      for (const [container, ids] of Object.entries(rawOrder)) {
        if (Array.isArray(ids)) {
          order[container] = ids.filter((id): id is string => typeof id === "string");
        }
      }
    }
    const rawFolders = (raw as Record<string, unknown>).folders;
    const folders = Array.isArray(rawFolders)
      ? rawFolders.filter((f): f is string => typeof f === "string" && f.endsWith("/"))
      : [];
    return { order, folders };
  } catch {
    return EMPTY_BINDER_ORDER;
  }
}

export function serializeBinderOrder(value: BinderOrder): string {
  return `${JSON.stringify({ order: value.order, folders: value.folders }, null, 2)}\n`;
}

/** Whether an id names a folder (trailing-slash convention). */
export function isFolderId(id: string): boolean {
  return id.endsWith("/");
}

/**
 * Order one container's children: sidecar-listed ids first, in saved
 * order (ids not present in `children` are skipped); everything unlisted
 * follows by the fallback — the ENTRY first, folders before files, then
 * alphabetical. A missing sidecar entry IS the fallback.
 */
export function orderChildIds(
  container: string,
  children: readonly string[],
  order: BinderOrder,
  entry: string | null,
): string[] {
  const present = new Set(children);
  const out: string[] = [];
  for (const id of order.order[container] ?? []) {
    if (present.has(id) && !out.includes(id)) out.push(id);
  }
  const placed = new Set(out);
  const rest = children.filter((id) => !placed.has(id));
  rest.sort((a, b) => {
    const aEntry = a === entry ? 0 : 1;
    const bEntry = b === entry ? 0 : 1;
    if (aEntry !== bEntry) return aEntry - bEntry;
    const aFolder = isFolderId(a) ? 0 : 1;
    const bFolder = isFolderId(b) ? 0 : 1;
    if (aFolder !== bFolder) return aFolder - bFolder;
    return a.localeCompare(b);
  });
  return [...out, ...rest];
}

/** Record a reorder: the container's full child list, in its new order.
 *  Always the FULL list (never a delta) so the entry is self-describing. */
export function applyReorder(
  value: BinderOrder,
  container: string,
  orderedIds: readonly string[],
): BinderOrder {
  return {
    ...value,
    order: { ...value.order, [container]: [...orderedIds] },
  };
}

/** Register a created (possibly empty) folder. */
export function addFolder(value: BinderOrder, folderId: string): BinderOrder {
  if (!isFolderId(folderId) || value.folders.includes(folderId)) return value;
  return { ...value, folders: [...value.folders, folderId] };
}

/**
 * Re-key for a rename/move (`oldPrefix` → `newPrefix`, both either two
 * file paths or two folder ids): every container KEY, ordered child id,
 * and registered folder at or under the old id moves to the new one —
 * authored order survives reorganization (celeris's `rekeyFolderRename`,
 * generalized to files too).
 */
export function rekeyBinderOrder(
  value: BinderOrder,
  oldId: string,
  newId: string,
): BinderOrder {
  const rekey = (id: string): string => {
    if (id === oldId) return newId;
    if (isFolderId(oldId) && id.startsWith(oldId)) return newId + id.slice(oldId.length);
    return id;
  };
  const order: Record<string, string[]> = {};
  for (const [container, ids] of Object.entries(value.order)) {
    order[rekey(container)] = ids.map(rekey);
  }
  return { order, folders: value.folders.map(rekey) };
}

/** Drop a removed id (file or folder subtree) from the sidecar. */
export function removeFromBinderOrder(value: BinderOrder, id: string): BinderOrder {
  const gone = (candidate: string): boolean =>
    candidate === id || (isFolderId(id) && candidate.startsWith(id));
  const order: Record<string, string[]> = {};
  for (const [container, ids] of Object.entries(value.order)) {
    if (gone(container)) continue;
    order[container] = ids.filter((child) => !gone(child));
  }
  return { order, folders: value.folders.filter((f) => !gone(f)) };
}
