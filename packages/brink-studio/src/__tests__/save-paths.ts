/**
 * The canonical id list for the confirm→retire sweep (issues #2455, #2480).
 *
 * Two suites need these ids and must never disagree about them:
 *
 *  - `save-retire-invariant.test.ts` builds one driver per id and sweeps each
 *    save path for an `await` between its confirming read and `markFilesSaved`.
 *  - `save-path-enrolment.test.ts` scans production source for every
 *    `markFilesSaved`/`markAllSaved` call site and checks the `SAVE-PATH`
 *    marker above it names an id from this list.
 *
 * It lives in its own module rather than being exported from the sweep's test
 * file because importing a test file re-registers its `describe`/`it` blocks
 * in the importing file: the whole confirm→retire sweep would run a second
 * time under the enrolment suite's name (measured — 6 duplicate tests).
 *
 * This is not a second hand-maintained list that can drift from the drivers.
 * `SavePathId` types `SavePath.id`, so a driver naming an id absent here is a
 * type error, and "SAVE_PATHS drives exactly the ids in save-paths.ts" in
 * `save-retire-invariant.test.ts` fails if a driver is added or removed
 * without updating this array.
 *
 * Adding a save path means: add its id here, add its driver to `SAVE_PATHS`,
 * and put a `// SAVE-PATH markFilesSaved: <id>` marker above the call site
 * (`docs/embedder-api.md`, "Confirm and retire in ONE synchronous step").
 */
export const SAVE_PATH_IDS = [
  "OverlayPersistence.saveAll",
  "OverlayPersistence.save",
  "file.save",
  "file.save (settled)",
  "file.saveAll",
] as const;

/** The id of a save path the confirm→retire sweep drives. */
export type SavePathId = (typeof SAVE_PATH_IDS)[number];
