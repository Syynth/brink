/**
 * Rule inference from marked lines (#3409) — the core of the
 * teach-by-example Conventions editor (RULED 2026-09-02).
 *
 * Explainable and verified, never clever:
 *
 * 1. PROPOSE a candidate shape per marked kind from a small fixed
 *    hypothesis space — an affix (common prefix/suffix, with `<>` glue),
 *    a `Name: text` line, or an all-caps line.
 * 2. REJECT any candidate a line of another mark also satisfies. Negatives
 *    are load-bearing: `Warning: the bridge is out` marked narration must
 *    kill a bare `…:` cue rule.
 * 3. VERIFY by re-parsing every line through `DialectParser` with the
 *    candidate dialect and keeping only what reproduces the marks. The
 *    support counts the UI shows are re-parse results, not estimates.
 * 4. Whatever the shapes cannot settle becomes a DECISION for the author
 *    — never a guess. Narration versus dialogue among bare lines is
 *    positional, not shape-based, so it is always the author's call.
 *
 * The output is the full dialect artifact. Whether it fits the
 * `[dialogue]` table form is a separate, verified question
 * (`toDialogueConfig`); the file form is the ruled escape hatch.
 */

import type { DialectElement, DialogueDialect } from "@brink/wasm-types";
import { affixElement, dialectFromConfig, type DialogueConfig, type DialogueElementConfig } from "./config.js";
import { DialectParser } from "./index.js";

/** What the author says a line is. */
export type Mark = "cue" | "dialogue" | "action" | "narration" | "parenthetical";

export interface MarkedLine {
  text: string;
  /** Tags ride separately from `text` (never part of a shape). */
  tags?: string[];
  origin?: "line" | "choice" | "gather";
  /** Absent = unmarked: still checked, never taught from. */
  mark?: Mark;
}

/** A rule the studio learned, in plain words, with the lines that support it. */
export interface Learned {
  id: string;
  sentence: string;
  /** Indices of the lines this rule reproduces on re-parse. */
  support: number[];
  /** How many lines carried the mark this rule is about. */
  total: number;
}

/** Something the shapes could not settle; the author decides. */
export interface Decision {
  id: string;
  message: string;
  lines: number[];
}

export interface Inference {
  /** The proposed dialect; `null` when nothing was taught. */
  dialect: DialogueDialect | null;
  learned: Learned[];
  decisions: Decision[];
}

/** The element kind each mark maps to; `null` = no element (plain narrative). */
const KIND_OF: Record<Mark, string | null> = {
  cue: "character",
  dialogue: "dialogue",
  action: "action",
  narration: null,
  parenthetical: "parenthetical",
};

const GLUE = "<>";

interface Shape {
  /** Which hypothesis produced it, for the sentence. */
  how: "affix" | "name-colon" | "caps";
  element: DialectElement;
  sentence: string;
  /** Extra learned sentences the shape carries (glue). */
  extras: string[];
}

const quote = (s: string): string => `“${s}”`;

/** The longest run of non-letter, non-digit characters at the start of every text. */
function commonPrefix(texts: readonly string[]): string {
  if (texts.length === 0) return "";
  let p = texts[0];
  for (const t of texts.slice(1)) {
    let i = 0;
    while (i < p.length && i < t.length && p[i] === t[i]) i++;
    p = p.slice(0, i);
  }
  // Cut back to marker characters only: `@MARA`/`@MARK` share "@MAR", but
  // the marker is "@".
  let end = 0;
  while (end < p.length && !/[\p{L}\p{N}]/u.test(p[end])) end++;
  return p.slice(0, end);
}

function commonSuffix(texts: readonly string[]): string {
  if (texts.length === 0) return "";
  let s = texts[0];
  for (const t of texts.slice(1)) {
    let i = 0;
    while (i < s.length && i < t.length && s[s.length - 1 - i] === t[t.length - 1 - i]) i++;
    s = s.slice(s.length - i);
  }
  let start = s.length;
  while (start > 0 && !/[\p{L}\p{N}]/u.test(s[start - 1])) start--;
  // Sentence punctuation is how sentences end, not a marker: `> She sets
  // the lantern down.` must not learn "." as its suffix.
  return s.slice(start).replace(/[.?!…,;]+$/u, "");
}

function describeAffix(prefix: string, suffix: string, glued: boolean, what: string): string {
  const parts: string[] = [];
  if (prefix !== "") parts.push(`starts with ${quote(prefix.trimEnd())}`);
  if (suffix !== "") parts.push(`ends with ${quote((suffix + (glued ? GLUE : "")).trimStart())}`);
  return `A line that ${parts.join(" and ")} is ${what}.`;
}

/** Hypothesis 1: a prefix/suffix marker, with `<>` glue split off the suffix. */
function affixShape(kind: string, role: string, texts: readonly string[], what: string): Shape | null {
  const glued = texts.every((t) => t.endsWith(GLUE));
  const bodies = glued ? texts.map((t) => t.slice(0, -GLUE.length)) : texts;
  const prefix = commonPrefix(bodies);
  const suffix = commonSuffix(bodies);
  if (prefix === "" && suffix === "" && !glued) return null;
  if (prefix === "" && suffix === "") return null;
  // Content must not be empty on every line — a marker alone is not a shape.
  if (bodies.every((b) => b.length <= prefix.length + suffix.length)) return null;
  // A cue's content is a name: short, no sentence inside. This is what
  // keeps `"What's that?" my master asked.` from teaching `"` as a cue
  // marker with the whole sentence as the "speaker".
  if (role === "speaker") {
    const plausible = bodies.every((b) => {
      const content = b.slice(prefix.length, b.length - suffix.length).trim();
      return content.length > 0 && content.length <= 40 && !/[.?!"]/.test(content);
    });
    if (!plausible) return null;
  }
  const element = affixElement(kind, "narrative", {
    prefix: prefix === "" ? null : prefix,
    suffix: suffix === "" ? null : suffix,
    glued,
    content_role: role,
  });
  const extras: string[] = [];
  if (glued) extras.push(`The ${quote(GLUE)} at the end attaches it to the line after it.`);
  return { how: "affix", element, sentence: describeAffix(prefix, suffix, glued, what), extras };
}

/** Hypothesis 2 (the ink docs' sub-format): `Name: text` on one line. */
const NAME_CLASS_FIRST = "[A-ZÀ-Þ]";
const NAME_CLASS_REST = "[A-Za-zÀ-ÿ0-9'’ -]";
const CAPS_CLASS_REST = "[A-ZÀ-Þ0-9 .'’-]";
// Portable-regex subset: no `\p{…}` — the TS parser compiles patterns
// without the `u` flag, and the same pattern travels to Rust's `regex`.
const NAME_COLON = new RegExp(`^(?<speaker>${NAME_CLASS_FIRST}${NAME_CLASS_REST}{0,40}?):\\s+(?<content>\\S.*)$`);
function nameColonShape(kind: string, texts: readonly string[]): Shape | null {
  if (!texts.every((t) => NAME_COLON.test(t))) return null;
  const element: DialectElement = {
    kind,
    nature: "narrative",
    source: {
      pattern: `^(?<speaker>${NAME_CLASS_FIRST}${NAME_CLASS_REST}{0,40}?):\\s+(?<content>\\S.*)$`,
      content_group: "content",
      template_group: null,
      hidden: [],
      template: "${speaker}: ${content}",
    },
    emitted: {
      pattern: `^(?<speaker>${NAME_CLASS_FIRST}${NAME_CLASS_REST}{0,40}?):\\s+`,
      content_group: "speaker",
      reserved_prefix: true,
    },
    malformed: [],
  };
  return {
    how: "name-colon",
    element,
    sentence: `A line that starts with a name and a colon, like ${quote(texts[0].slice(0, texts[0].indexOf(":") + 1))}, is a cue; the name is the speaker and the rest of the line is what they say.`,
    extras: [],
  };
}

/** Hypothesis 3 (screenplay): a line in capitals on its own. */
const CAPS = new RegExp(`^(?<speaker>${NAME_CLASS_FIRST}${CAPS_CLASS_REST}*)$`);
function capsShape(kind: string, texts: readonly string[]): Shape | null {
  if (!texts.every((t) => CAPS.test(t) && /[A-ZÀ-Þ]{2}/.test(t))) return null;
  const element: DialectElement = {
    kind,
    nature: "narrative",
    source: {
      pattern: `^(?<speaker>${NAME_CLASS_FIRST}${CAPS_CLASS_REST}*)$`,
      content_group: "speaker",
      template_group: null,
      hidden: [],
      template: "${speaker}",
    },
    emitted: {
      pattern: `^(?<speaker>${NAME_CLASS_FIRST}${CAPS_CLASS_REST}*)$`,
      content_group: "speaker",
      reserved_prefix: true,
    },
    malformed: [],
  };
  return {
    how: "caps",
    element,
    sentence: `A line in capitals on its own, like ${quote(texts[0])}, is a cue naming the speaker.`,
    extras: [],
  };
}

function candidatesFor(mark: Mark, texts: readonly string[]): Shape[] {
  const kind = KIND_OF[mark];
  if (kind === null) return [];
  const out: Shape[] = [];
  if (mark === "cue") {
    const a = affixShape(kind, "speaker", texts, "a cue naming the speaker");
    if (a) out.push(a);
    const n = nameColonShape(kind, texts);
    if (n) out.push(n);
    const c = capsShape(kind, texts);
    if (c) out.push(c);
  } else {
    const what = mark === "action" ? "an action line" : "a parenthetical";
    const a = affixShape(kind, "content", texts, what);
    if (a) out.push(a);
  }
  return out;
}

/** The `[dialogue]` table for learned affix elements over the shipped preset. */
function presetTable(elements: readonly DialectElement[], runEndsAt: string[]): DialogueConfig {
  const rows: DialogueElementConfig[] = [];
  for (const el of elements) {
    if (!el.source || "pattern" in el.source) continue;
    const row: DialogueElementConfig = { kind: el.kind };
    if (el.source.prefix) row.prefix = el.source.prefix;
    if (el.source.suffix) row.suffix = el.source.suffix;
    if (el.source.glued) row.glued = true;
    if (el.source.content_role && el.source.content_role !== "content") row.contentRole = el.source.content_role;
    rows.push(row);
  }
  return { preset: "at-cue", elements: rows, runEndsAt };
}

/** Does `element`'s source shape classify `text`? (Isolated, no chain.) */
function matchesSource(element: DialectElement, text: string): boolean {
  const d: DialogueDialect = { version: 1, name: "probe", elements: [element], chain: [], transitions: [], templates: { entries: [] } };
  const line = new DialectParser(d).parseSource(text)[0];
  return line?.kind === element.kind;
}

export function inferDialect(lines: readonly MarkedLine[]): Inference {
  const learned: Learned[] = [];
  const decisions: Decision[] = [];
  const idx = (m: Mark): number[] => lines.flatMap((l, i) => (l.mark === m ? [i] : []));
  const marked = (m: Mark): string[] => idx(m).map((i) => lines[i].text);

  const elements: DialectElement[] = [];
  const shapeSentences = new Map<string, Shape>();

  for (const mark of ["cue", "action", "parenthetical"] as const) {
    const positives = idx(mark);
    if (positives.length === 0) continue;
    const texts = positives.map((i) => lines[i].text);
    const negatives = lines.flatMap((l, i) => (l.mark !== undefined && l.mark !== mark ? [i] : []));
    let chosen: Shape | null = null;
    let rejectedBy: { shape: Shape; lines: number[] } | null = null;
    for (const cand of candidatesFor(mark, texts)) {
      const clashes = negatives.filter((i) => matchesSource(cand.element, lines[i].text));
      if (clashes.length === 0) {
        chosen = cand;
        break;
      }
      rejectedBy ??= { shape: cand, lines: clashes };
    }
    if (chosen) {
      elements.push(chosen.element);
      shapeSentences.set(chosen.element.kind, chosen);
      continue;
    }
    if (rejectedBy) {
      decisions.push({
        id: `${mark}-ambiguous`,
        message: `${rejectedBy.shape.sentence.replace(/\.$/, "")} — but ${rejectedBy.lines.length === 1 ? "this line" : "these lines"} would match too and ${rejectedBy.lines.length === 1 ? "is" : "are"} marked differently. Mark ${rejectedBy.lines.length === 1 ? "it" : "them"} the same way, or use a marker the other lines never start with.`,
        lines: rejectedBy.lines,
      });
      continue;
    }
    const allTagged = positives.every((i) => (lines[i].tags ?? []).length > 0);
    decisions.push({
      id: `${mark}-no-shape`,
      message: allTagged
        ? `These lines are marked ${mark} but share no marker in their text — the ${mark === "cue" ? "speaker" : "marking"} seems to live in a tag (${quote(`# ${lines[positives[0]].tags?.[0] ?? ""}`)}). This editor cannot express a tag-carried ${mark} yet.`
        : `These lines are marked ${mark} but share no marker the studio can learn. Give them one (a character at the start, or at the end), or mark them as something else.`,
      lines: positives,
    });
  }

  const hasCue = elements.some((e) => e.kind === KIND_OF.cue);
  const dialogueIdx = idx("dialogue");
  if (!hasCue && dialogueIdx.length > 0) {
    decisions.push({
      id: "dialogue-without-cue",
      message: "Lines are marked dialogue, but no cue names a speaker. Mark the line that names who is speaking as a cue.",
      lines: dialogueIdx,
    });
  }
  if (idx("cue").length === 0 && idx("dialogue").length === 0 && idx("action").length === 0 && idx("parenthetical").length === 0) {
    return { dialect: null, learned: [], decisions: [{ id: "nothing-marked", message: "Mark at least one line — a cue that names who is speaking is the usual place to start.", lines: [] }] };
  }

  // The chain: which shaped kinds a following bare line belongs to, and
  // which end a run. Read off the marks: dialogue right after an action
  // means the action did not end the turn. A cue that carries its speech
  // on the same line (`Name: text`) owns no following lines unless the
  // author marked one as dialogue; a header-only cue (affix, caps) always
  // does — a glued cue with nothing after it means nothing.
  const cueShape = shapeSentences.get(KIND_OF.cue as string) ?? null;
  const hasChain = hasCue && (dialogueIdx.length > 0 || cueShape?.how !== "name-colon");
  const chainAfter = new Set<string>();
  const runEndsAt = new Set<string>(["choices"]);
  if (hasChain) {
    chainAfter.add(KIND_OF.cue as string);
    chainAfter.add("dialogue");
    if (elements.some((e) => e.kind === "parenthetical")) chainAfter.add("parenthetical");
    let actionContinues = false;
    let actionEnds = false;
    for (let i = 1; i < lines.length; i++) {
      if (lines[i - 1].mark === "action") {
        if (lines[i].mark === "dialogue") actionContinues = true;
        if (lines[i].mark === "narration") actionEnds = true;
      }
    }
    if (elements.some((e) => e.kind === "action")) {
      if (actionContinues && !actionEnds) chainAfter.add("action");
      else runEndsAt.add("action");
    }
  }

  if (hasChain && !elements.some((e) => e.kind === "dialogue")) {
    elements.push({ kind: "dialogue", nature: "narrative" });
  }

  // Prefer the shipped preset when it can carry the result: every learned
  // shape is affix sugar and the chain is the preset's own. Then the
  // dialect IS the resolution of the table the editor will write, by
  // construction — no projection to verify, nothing to lose in a file.
  const presetFits =
    hasChain &&
    [...shapeSentences.values()].every((sh) => sh.how === "affix") &&
    !chainAfter.has("action");
  const dialect: DialogueDialect = presetFits
    ? dialectFromConfig(presetTable(elements, [...runEndsAt]))
    : {
    version: 1,
    name: "project",
    elements,
    chain: hasChain
      ? [
          {
            after: [...chainAfter],
            is: ["narrative"],
            becomes: "dialogue",
            carry: ["speaker"],
            run_ends_at: [...runEndsAt],
          },
        ]
      : [],
    transitions: [],
    templates: { entries: [] },
  };

  // VERIFY: re-parse every line with the candidate and read the support
  // counts off the result. Whatever does not reproduce a mark is a decision.
  const parsed = new DialectParser(dialect).parseSource(lines.map((l) => l.text).join("\n"));
  const kindAt = (i: number): string | null => parsed[i]?.kind ?? null;

  for (const [kind, shape] of shapeSentences) {
    const mark = (Object.keys(KIND_OF) as Mark[]).find((m) => KIND_OF[m] === kind) as Mark;
    const positives = idx(mark);
    const support = positives.filter((i) => kindAt(i) === kind);
    learned.push({ id: `${mark}-shape`, sentence: shape.sentence, support, total: positives.length });
    for (const extra of shape.extras) {
      learned.push({ id: `${mark}-extra-${learned.length}`, sentence: extra, support, total: positives.length });
    }
    const missed = positives.filter((i) => kindAt(i) !== kind);
    if (missed.length > 0) {
      decisions.push({
        id: `${mark}-unexplained`,
        message: `${missed.length === 1 ? "This line is" : "These lines are"} marked ${mark} but ${missed.length === 1 ? "does" : "do"} not fit the rule the other ${mark} lines taught.`,
        lines: missed,
      });
    }
  }

  if (hasChain) {
    // Support is every line the chain claimed that the author did not
    // contradict: marked dialogue, or left unmarked.
    const support = lines.flatMap((l, i) =>
      (l.mark === "dialogue" || l.mark === undefined) && kindAt(i) === "dialogue" ? [i] : [],
    );
    const enders: string[] = [];
    if (runEndsAt.has("action")) enders.push("an action line");
    enders.push("the next cue");
    enders.push("the choices");
    const last = enders.pop();
    const missedDialogue = dialogueIdx.filter((i) => kindAt(i) !== "dialogue");
    const total = support.length + missedDialogue.length;
    learned.push({
      id: "run",
      sentence: `Lines after a cue belong to that speaker until ${enders.join(", ")} or ${last}.`,
      support,
      total,
    });
    if (chainAfter.has("action")) {
      learned.push({
        id: "run-through-action",
        sentence: "An action line does not end the speaker's turn — the lines after it are still theirs.",
        support,
        total,
      });
    }
    if (missedDialogue.length > 0) {
      decisions.push({
        id: "dialogue-unexplained",
        message: `${missedDialogue.length === 1 ? "This line is" : "These lines are"} marked dialogue but nothing before ${missedDialogue.length === 1 ? "it" : "them"} names a speaker.`,
        lines: missedDialogue,
      });
    }
    const narrationAsDialogue = idx("narration").filter((i) => kindAt(i) === "dialogue");
    if (narrationAsDialogue.length > 0) {
      decisions.push({
        id: "narration-after-cue",
        message: `${narrationAsDialogue.length === 1 ? "This line is" : "These lines are"} marked narration but ${narrationAsDialogue.length === 1 ? "follows" : "follow"} a speaker's lines with nothing in between — the studio cannot tell ${narrationAsDialogue.length === 1 ? "it" : "them"} from more speech. Put an action line before ${narrationAsDialogue.length === 1 ? "it" : "them"}, or mark ${narrationAsDialogue.length === 1 ? "it" : "them"} as dialogue.`,
        lines: narrationAsDialogue,
      });
    }
  }
  // Narration or unmarked lines that a shape swallowed.
  const stolen = lines.flatMap((l, i) => {
    if (l.mark !== "narration") return [];
    const k = kindAt(i);
    return k !== null && k !== "dialogue" ? [i] : [];
  });
  if (stolen.length > 0) {
    decisions.push({
      id: "narration-shaped",
      message: `${stolen.length === 1 ? "This line is" : "These lines are"} marked narration but ${stolen.length === 1 ? "matches" : "match"} a rule above.`,
      lines: stolen,
    });
  }

  return { dialect, learned, decisions };
}
