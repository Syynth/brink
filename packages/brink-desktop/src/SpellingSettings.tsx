/**
 * Settings › Spelling — which checker serves spelling on this machine.
 *
 * ## Why this is its own section rather than a row in Prose
 *
 * The rail splits on SCOPE, and that split is load-bearing: `project`
 * settings are written to `brink.toml` and shared with everyone who opens
 * the story, `app` settings are this machine's alone. The built-in Prose
 * section is project scope — dialect and dictionary both live in
 * `brink.toml`. Which checker runs is neither: it is a fact about the
 * machine's own dictionary, and two authors on the same story can
 * reasonably differ.
 *
 * Putting the row in Prose would have meant a machine preference sitting
 * under a heading that promises the project. A host section in the app tab
 * costs one rail row and keeps the promise.
 */

import { useEffect, useId, useState } from "react";
import { SettingsGroup, SettingsRow, SettingsToggle } from "@brink-lang/studio";

export interface SpellingSettingsApi {
  /** Current preference. */
  useSystem(): boolean;
  /** Persist it, and make the change visible now rather than next launch. */
  setUseSystem(next: boolean): void;
  /**
   * Whether this platform has a native checker at all.
   *
   * Resolved by asking, not by sniffing the user agent: the shell answers
   * `unavailable` on Linux and Windows, and that answer is the only honest
   * source. `null` while the probe is in flight.
   */
  probeAvailable(): Promise<boolean>;
}

export function SpellingSettings({ api }: { api: SpellingSettingsApi }) {
  const [useSystem, setUseSystem] = useState(() => api.useSystem());
  const [available, setAvailable] = useState<boolean | null>(null);
  const toggleId = useId();

  useEffect(() => {
    let live = true;
    void api.probeAvailable().then((next) => {
      if (live) setAvailable(next);
    });
    return () => {
      live = false;
    };
  }, [api]);

  // Off is never misreported as on. Where the platform has no checker the
  // switch reads OFF whatever the stored preference says, because Harper is
  // what is actually running — a switch showing "on" over a checker that is
  // not consulted is the lie the whole settings model avoids.
  const effective = useSystem && available !== false;

  let description: string;
  if (available === false) {
    description =
      "This platform has no system spell checker, so the built-in one handles spelling here. Your preference is kept for machines that do.";
  } else if (effective) {
    description =
      "Spelling comes from this machine's dictionary — including every word you have taught it in other apps — and words you add here go back into it. Grammar always comes from the built-in checker.";
  } else {
    description =
      "The built-in checker handles spelling as well as grammar. Its dictionary starts from scratch on every machine, so invented names need adding by hand.";
  }

  return (
    <SettingsGroup title="Spelling">
      <SettingsRow
        title="Use the system spell checker"
        description={description}
        htmlFor={toggleId}
      >
        <SettingsToggle
          id={toggleId}
          checked={effective}
          label="Use the system spell checker"
          onChange={(next) => {
            // Inert where there is nothing to switch to, rather than
            // writing a preference that changes nothing visible.
            if (available === false) return;
            setUseSystem(next);
            api.setUseSystem(next);
          }}
        />
      </SettingsRow>
    </SettingsGroup>
  );
}
