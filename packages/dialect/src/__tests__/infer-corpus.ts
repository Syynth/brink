/**
 * Golden corpus for rule inference (#3409) — a plain module (never a
 * `.test.ts`, per the no-test-file-imports rule). Every entry: marked
 * lines → the rules expected in plain words, the decisions expected, and
 * whether the result fits the `[dialogue]` table form.
 *
 * Includes the ink documentation's own suggested line formats
 * (`RunningYourInk.md` "simple sub-formats" and tags; `WritingWithInk.md`
 * dialogue choices) — see #3392 for why each is here.
 */
import type { MarkedLine } from "../infer.js";

export interface Case {
  id: string;
  lines: MarkedLine[];
  /** Substrings each learned sentence must contain, in order. */
  learned: string[];
  /** Decision ids expected, in order. */
  decisions: string[];
  /** Element kinds expected on the dialect, sorted; `null` = no dialect. */
  kinds: string[] | null;
  /** Whether `toDialogueConfig` must find a table form. */
  tableForm: boolean;
}

const L = (text: string, mark?: MarkedLine["mark"], extra: Partial<MarkedLine> = {}): MarkedLine => ({
  text,
  ...(mark ? { mark } : {}),
  ...extra,
});

export const CORPUS: Case[] = [
  {
    id: "at-cue glued, action ends the turn (the canvas sample)",
    lines: [
      L("@MARA: <>", "cue"),
      L("We don't have until morning.", "dialogue"),
      L("Not even close.", "dialogue"),
      L("> She sets the lantern down.", "action"),
      L("The lantern gutters.", "narration"),
      L("@JUNO: <>", "cue"),
      L("Then we go now.", "dialogue"),
    ],
    learned: ["starts with “@” and ends with “: <>” is a cue", "“<>” at the end attaches", "starts with “>” is an action line", "until an action line, the next cue or the choices"],
    decisions: [],
    kinds: ["action", "character", "dialogue", "parenthetical"],
    tableForm: true,
  },
  {
    id: "action does not end the turn when dialogue is marked after it",
    lines: [
      L("@MARA: <>", "cue"),
      L("Wait.", "dialogue"),
      L("> She listens.", "action"),
      L("Nothing. Go.", "dialogue"),
    ],
    learned: ["is a cue", "attaches", "is an action line", "until the next cue or the choices", "does not end the speaker's turn"],
    decisions: [],
    kinds: ["action", "character", "dialogue"],
    tableForm: false,
  },
  {
    id: "ink docs: `Name: line`, double space, inside choice text",
    lines: [
      L("Lisa: Where did he go?", "cue", { origin: "choice" }),
      L("Joe:  I think he jumped over the garden fence.", "cue"),
      L("Lisa: Let's take a look.", "cue", { origin: "choice" }),
      L("The fence was higher than it looked.", "narration"),
    ],
    learned: ["starts with a name and a colon, like “Lisa:”"],
    decisions: [],
    kinds: ["character"],
    tableForm: false,
  },
  {
    id: "ink docs: a colon mid-sentence in narration is a decision, never a rule",
    lines: [
      L("Lisa: Where did he go?", "cue"),
      L("Joe: Over the fence.", "cue"),
      L("Warning: the bridge is out.", "narration"),
    ],
    learned: [],
    decisions: ["cue-ambiguous"],
    kinds: [],
    tableForm: false,
  },
  {
    id: "ink docs: tags never become part of a shape",
    lines: [
      L("Passepartout: Really, Monsieur.", "cue", { tags: ["surly", "really_monsieur.ogg"] }),
      L("Fogg: Quite.", "cue"),
      L("The clock struck.", "narration", { tags: ["chime"] }),
    ],
    learned: ["starts with a name and a colon"],
    decisions: [],
    kinds: ["character"],
    tableForm: false,
  },
  {
    id: "ink docs: a speaker carried in a tag is a decision this editor cannot express",
    lines: [
      L("Really, Monsieur.", "cue", { tags: ["speaker: Passepartout"] }),
      L("Quite.", "cue", { tags: ["speaker: Fogg"] }),
      L("The clock struck.", "narration"),
    ],
    learned: [],
    decisions: ["cue-no-shape"],
    kinds: [],
    tableForm: false,
  },
  {
    id: "ink docs: quoted prose with attribution teaches no cue rule",
    lines: [
      L('"What\'s that?" my master asked.', "narration"),
      L('"I am somewhat tired," I repeated.', "narration"),
      L('"Really," he responded. "How deleterious."', "narration"),
    ],
    learned: [],
    decisions: ["nothing-marked"],
    kinds: null,
    tableForm: false,
  },
  {
    id: "quoted prose marked as cues has no learnable shape (the naive-prefix trap)",
    lines: [
      L('"What\'s that?" my master asked.', "cue"),
      L('"Quite well," he replied.', "cue"),
      L("He looked away.", "narration"),
    ],
    learned: [],
    decisions: ["cue-no-shape"],
    kinds: [],
    tableForm: false,
  },
  {
    id: "screenplay: NAME on its own line with a parenthetical under it",
    lines: [
      L("MARA", "cue"),
      L("(quietly)", "parenthetical"),
      L("We don't have until morning.", "dialogue"),
      L("JUNO", "cue"),
      L("Then we go now.", "dialogue"),
    ],
    learned: ["in capitals on its own, like “MARA”", "starts with “(” and ends with “)” is a parenthetical", "until the next cue or the choices"],
    decisions: [],
    kinds: ["character", "dialogue", "parenthetical"],
    tableForm: false,
  },
  {
    id: "narration right after a speaker's lines is the author's call",
    lines: [
      L("@MARA: <>", "cue"),
      L("Wait.", "dialogue"),
      L("The wind picked up.", "narration"),
    ],
    learned: ["is a cue", "attaches", "until the next cue or the choices"],
    decisions: ["narration-after-cue"],
    kinds: ["character", "dialogue", "parenthetical"],
    tableForm: true,
  },
  {
    id: "dialogue marked with no cue anywhere",
    lines: [L("Wait.", "dialogue"), L("Go.", "dialogue")],
    learned: [],
    decisions: ["dialogue-without-cue"],
    kinds: [],
    tableForm: false,
  },
  {
    id: "unmarked lines are checked, not taught from",
    lines: [
      L("@MARA: <>", "cue"),
      L("We don't have until morning."),
      L("Not even close."),
      L("> She sets the lantern down.", "action"),
    ],
    learned: ["is a cue", "attaches", "is an action line", "until an action line, the next cue or the choices"],
    decisions: [],
    kinds: ["action", "character", "dialogue", "parenthetical"],
    tableForm: true,
  },
];
