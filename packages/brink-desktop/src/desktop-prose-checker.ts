/**
 * Harper for grammar, the OS for spelling (`docs/desktop-ota-spec.md`
 * Stage 4).
 *
 * Two checkers with a clean line between them, because they are good at
 * different things:
 *
 * - **The platform checker knows the author's own words.** Every name they
 *   have ever taught macOS — from Mail, from Pages, from the last game they
 *   wrote — is already in it, and words they add here go back into it. A
 *   bundled dictionary starts from zero on every machine and stays there.
 * - **Harper knows English, not words.** Repetition, agreement, eggcorns,
 *   `can be seem` → `can be seen`. None of that is reachable from a word
 *   list, and the platform API offers none of it.
 *
 * So exactly one Harper category is suppressed: `Spelling`, whose own
 * documentation says it is "only ... used by linters doing spellcheck on
 * individual words". `Typo` STAYS — it is Harper catching a real word in the
 * wrong place, which a spell checker cannot see by construction.
 *
 * Off macOS the command answers `unavailable`, which is an ANSWER and not an
 * error: Harper keeps its spelling pass there and the author sees no
 * difference. That is why the composition is a runtime decision per check
 * rather than a build-time one.
 *
 * ## Only the prose is checked, and the offsets say so
 *
 * The request carries the document and the spans of it that are authored
 * prose. Rather than slice each span and map the answers back — one IPC call
 * per span, and an offset-mapping bug class — this builds a MASK: the same
 * string, with every non-prose character replaced by a space. Offsets come
 * back in the document's own coordinate space and need no translation at
 * all, and `You have {gold} pieces` cannot be mis-joined into one word,
 * because the machinery it replaced was the same width.
 */

import type { ProseChecker, ProseLint } from "@brink-lang/studio";
import type { SpellcheckOutcome } from "./tauri-provider.js";

/** The one Harper category the platform checker replaces. */
const REPLACED_KIND = "spelling";

/**
 * `text` with everything outside `spans` blanked to spaces, preserving
 * length so offsets need no mapping.
 *
 * Line breaks survive: collapsing them would run every line of a scene into
 * one paragraph, which is a different document to check even if the words
 * are the same.
 *
 * Spans are clamped and may arrive in any order or overlap — they come from
 * an HIR projection through a subtraction, and a caller should not have to
 * trust that.
 */
export function maskToProse(text: string, spans: readonly { start: number; end: number }[]): string {
  const keep = new Uint8Array(text.length);
  for (const span of spans) {
    const start = Math.max(0, Math.min(span.start, text.length));
    const end = Math.max(start, Math.min(span.end, text.length));
    keep.fill(1, start, end);
  }
  let out = "";
  for (let i = 0; i < text.length; i++) {
    const ch = text[i];
    if (keep[i] === 1 || ch === "\n" || ch === "\r") out += ch;
    else out += " ";
  }
  return out;
}

/** Whether a Harper lint is the kind the platform checker takes over. */
export function isReplacedBySpellchecker(lint: Pick<ProseLint, "kind">): boolean {
  return lint.kind.toLowerCase() === REPLACED_KIND;
}

/**
 * Fold an OS spellcheck outcome into Harper's findings.
 *
 * `null` for "the platform had no answer", which is what leaves Harper's own
 * spelling pass standing.
 */
export function mergeLints(
  harper: readonly ProseLint[],
  outcome: SpellcheckOutcome,
): ProseLint[] {
  if (outcome.kind !== "checked") return [...harper];
  const spelling: ProseLint[] = outcome.misspellings.map((m) => ({
    start: m.start,
    end: m.end,
    // The same kind Harper used, so every consumer that styles or filters by
    // it — the squiggle layer, the Problems panel — keeps working unchanged.
    kind: "Spelling",
    message: `“${m.word}” may be misspelled.`,
    suggestions: m.suggestions.map((text) => ({ kind: "replace", text })),
  }));
  return [...harper.filter((lint) => !isReplacedBySpellchecker(lint)), ...spelling];
}

/**
 * Wrap the studio's checker so spelling comes from the platform.
 *
 * Both checks run in parallel: they are independent, and serialising them
 * would put the slower one's latency on top of the faster one for no gain.
 *
 * A platform check that THROWS is treated as `unavailable` — the author gets
 * Harper's spelling rather than no spelling. The seam's contract is that a
 * rejection leaves the previous squiggles standing, which is right for a
 * transient fault and wrong as an answer.
 */
export function desktopProseChecker(
  builtin: ProseChecker,
  spellcheck: (
    text: string,
    language: string | null,
    dictionary: string[],
  ) => Promise<SpellcheckOutcome>,
): ProseChecker {
  return {
    async check(request): Promise<ProseLint[]> {
      const [harper, outcome] = await Promise.all([
        builtin.check(request),
        spellcheck(
          maskToProse(request.text, request.spans),
          // The dialect is Harper's vocabulary (`american`, `british`, …),
          // not a BCP-47 tag. Passing it through would ask macOS for a
          // language named "british"; `null` means "whatever this machine
          // is set to", which is the author's own choice already.
          null,
          request.dictionary,
        ).catch((error: unknown) => {
          console.warn("[prose] OS spellcheck failed; keeping Harper's spelling", error);
          return { kind: "unavailable", reason: String(error) } as const;
        }),
      ]);
      return mergeLints(harper, outcome);
    },
  };
}
