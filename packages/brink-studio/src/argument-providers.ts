/**
 * Host argument providers (#175, docs/host-argument-picker-spec.md).
 *
 * A host (embedder) declares `argumentProviders` in its `StudioExtensions`:
 * data-only value sources keyed by semantic type. At mount we enumerate them
 * and push the combined snapshot into the editor session's value cache
 * (`setHostValues`, #174), so the argument picker dropdown + inline value
 * labels show the host's live vocabulary (named switches / items / …). The
 * studio owns all rendering; the host only supplies data.
 */

import type { ArgumentProvider } from "@brink/studio-shell";
import type { ValueItem } from "@brink/wasm-types";

/** The slice of the editor session the value push needs (the host value cache). */
export interface HostValueSink {
  setHostValues(values: Record<string, ValueItem[]>): void;
}

/**
 * Enumerate each provider and push the combined `{ type: values }` snapshot
 * into the session's host-value cache. Providers may be async; one that throws
 * is skipped (its type just gets no host values, degrading to literal entry).
 * No-op when there are no providers / no values.
 */
export async function pushArgumentProviderValues(
  session: HostValueSink,
  providers: readonly ArgumentProvider[],
): Promise<void> {
  if (providers.length === 0) return;
  const entries = await Promise.all(
    providers.map(async (provider): Promise<[string, ValueItem[]] | null> => {
      try {
        return [provider.type, await provider.enumerate()];
      } catch {
        return null;
      }
    }),
  );
  const values: Record<string, ValueItem[]> = {};
  for (const entry of entries) {
    if (entry) values[entry[0]] = entry[1];
  }
  if (Object.keys(values).length > 0) session.setHostValues(values);
}
