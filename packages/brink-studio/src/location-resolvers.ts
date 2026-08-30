/**
 * Location-protocol resolver registration (W3/#3296) — the wiring
 * `docs/studio-shell-spec.md` §6.1 reserved: symbol → source over the
 * compile outline, session → program over a position-shaped session ref,
 * and program → source through the live provider's DebugInfo road.
 *
 * Extracted from `mount.tsx` (the `debug-commands.ts` pattern) so the
 * wiring is testable over a real store: `makeProgramResolver` and
 * `resolveSessionPositionRef` were exported and tested in
 * `@brink/studio-shell` but registered by nothing — the classic seam gap
 * the D9 survey flagged ("built — never registered").
 *
 * The `sessionDegraded` gate lives HERE, at the caller, per
 * `makeProgramResolver`'s own contract: a running program that diverges
 * from the latest compile must suppress resolution — a debugger that
 * reveals the wrong line is worse than one that refuses
 * (`docs/live-inspector-spec.md` §5).
 */

import {
  LocationResolvers,
  makeProgramResolver,
  resolveQualifiedSymbol,
  resolveSessionPositionRef,
} from "@brink/studio-shell";
import {
  isDebugSessionProvider,
  sessionDegraded,
  type StudioStore,
} from "@brink/studio-store";

/** Register all three non-source resolvers. Throws on double registration
 *  (the registry's own duplicate guard), so call once per mount. */
export function registerLocationResolvers(
  locations: LocationResolvers,
  store: StudioStore,
): void {
  locations.register("symbol", (location) =>
    location.kind === "symbol"
      ? resolveQualifiedSymbol(store.getState().outline, location.name)
      : null,
  );

  locations.register(
    "program",
    makeProgramResolver((containerIdx, offset) => {
      const s = store.getState();
      // Suppressed, never stale: byte offsets shift on every edit, so a
      // diverged program's answer would be confidently wrong.
      if (sessionDegraded(s.programChecksum, s.compiledChecksum)) return null;
      const provider = s._provider;
      if (provider === null || !isDebugSessionProvider(provider)) return null;
      return provider.resolveDebugPosition(containerIdx, offset);
    }),
  );

  // A session ref carrying a runtime position becomes a program Location,
  // which the program resolver above continues toward source — the full
  // session → program → source chain the step cap allows for.
  locations.register("session", resolveSessionPositionRef);
}
