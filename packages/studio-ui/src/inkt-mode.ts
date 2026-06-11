/**
 * Minimal CM6 mode for the `.inkt` textual dump (issue #91, spec §4
 * "Compiled Output").
 *
 * `.inkt` is the WAT-inspired s-expression dump of compiled `StoryData`
 * (crates/internal/brink-format/src/inkt/) — strictly machine-generated,
 * two-space indented, one form or instruction per line. This mode is
 * deliberately *not* a parser (the pest grammar in inkt.pest is the real
 * one); it is a line tokenizer for syntax color plus indentation-based fold
 * ranges, enough to make a long dump scannable.
 *
 * Token classes (from the grammar):
 *   - section keywords — the identifier right after `(`: story, container,
 *     code, lines, globals, addresses, …
 *   - opcodes — bare identifiers anywhere else (inside `(code …)` bodies)
 *   - `$xx_hex` definition ids, `@hex16` source-line hashes
 *   - `key=value` operands (checksum=, name=, argc=, slot=)
 *   - strings, integers/floats/0x literals, `a:b` source locations
 *   - `->` / `..` / `+` punctuation
 */

import {
  HighlightStyle,
  StreamLanguage,
  foldService,
  syntaxHighlighting,
  type StreamParser,
} from "@codemirror/language";
import type { Extension } from "@codemirror/state";
import { tags as t } from "@lezer/highlight";

interface InktState {
  /** True when the previous token was `(` — the next word is a form head. */
  afterOpen: boolean;
}

const inktParser: StreamParser<InktState> = {
  name: "inkt",

  startState: () => ({ afterOpen: false }),

  token(stream, state) {
    if (stream.eatSpace()) return null;

    const afterOpen = state.afterOpen;
    state.afterOpen = false;

    if (stream.eat("(")) {
      state.afterOpen = true;
      return "paren";
    }
    if (stream.eat(")")) return "paren";

    // Strings (single-line in the writer's output; escapes per the grammar).
    if (stream.match(/^"(?:\\.|[^"\\])*"?/)) return "string";

    // Definition ids ($01_406ea523c53def) and source-line hashes (@…16 hex).
    if (stream.match(/^\$[0-9a-fA-F]{2}_[0-9a-fA-F]+/)) return "defId";
    if (stream.match(/^@[0-9a-fA-F]+/)) return "sourceHash";

    // key=value operands: highlight the key, leave `=`+value for next calls.
    if (stream.match(/^[A-Za-z_][A-Za-z0-9_]*(?==)/)) return "kvKey";

    // Numbers: hex literals, floats, integers, and `line:col` source locs.
    if (stream.match(/^0x[0-9a-fA-F]+/)) return "number";
    if (stream.match(/^-?\d+\.\.\d+/)) return "number"; // span `12..34`
    if (stream.match(/^-?\d+:\d+/)) return "number"; // source_loc `3:14`
    if (stream.match(/^-?\d+(?:\.\d*)?/)) return "number";

    // Punctuation between operands.
    if (stream.match("->") || stream.match("..")) return "operator";
    if (stream.eat("+") || stream.eat(":") || stream.eat("=")) return "operator";

    // Bare identifiers: form heads after `(`, opcodes/flags everywhere else.
    if (stream.match(/^[A-Za-z_][A-Za-z0-9_]*/)) {
      return afterOpen ? "sectionKeyword" : "opcode";
    }

    stream.next();
    return null;
  },

  tokenTable: {
    paren: t.paren,
    string: t.string,
    defId: t.atom,
    sourceHash: t.meta,
    kvKey: t.propertyName,
    number: t.number,
    operator: t.operator,
    sectionKeyword: t.keyword,
    opcode: t.function(t.variableName),
  },
};

export const inktLanguage = StreamLanguage.define(inktParser);

/**
 * Maps the mode's tags onto the studio's existing `tok-*` semantic-token
 * classes (studio.css) so the dump matches the editor palette without new
 * theme machinery.
 */
const inktHighlightStyle = HighlightStyle.define([
  { tag: t.keyword, class: "tok-keyword" },
  { tag: t.function(t.variableName), class: "tok-function" },
  { tag: t.string, class: "tok-string" },
  { tag: t.number, class: "tok-number" },
  { tag: t.atom, class: "tok-label" },
  { tag: t.meta, class: "tok-comment" },
  { tag: t.propertyName, class: "tok-parameter" },
  { tag: t.operator, class: "tok-operator" },
]);

export const inktHighlighting: Extension = syntaxHighlighting(inktHighlightStyle);

function indentOf(text: string): number {
  let i = 0;
  while (i < text.length && text[i] === " ") i++;
  return i;
}

/**
 * Indentation-based folding: a line folds over the contiguous run of
 * more-indented lines below it (blank lines pass through). The writer indents
 * two spaces per nesting level and puts closing parens at the parent's
 * indent, so a folded `(container …` keeps its `)` line visible.
 */
export const inktFolding: Extension = foldService.of((state, lineStart, lineEnd) => {
  const doc = state.doc;
  const line = doc.lineAt(lineStart);
  if (line.text.trim().length === 0) return null;
  const indent = indentOf(line.text);

  let end = -1;
  for (let n = line.number + 1; n <= doc.lines; n++) {
    const next = doc.line(n);
    if (next.text.trim().length === 0) continue;
    if (indentOf(next.text) <= indent) break;
    end = next.to;
  }

  return end > lineEnd ? { from: lineEnd, to: end } : null;
});
