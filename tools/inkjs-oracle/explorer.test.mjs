import assert from "node:assert/strict";
import { test } from "node:test";

import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

import { CompileError, Explorer, compileStory, valueToJson } from "./explorer.mjs";

const here = new URL(".", import.meta.url).pathname;
const repoRoot = join(here, "..", "..");

function explore(source, options = {}) {
  const story = compileStory(source, here, "inline.ink");
  return new Explorer(story, {}, options).explore();
}

test("a two-choice story crawls into two Ended episodes in the golden shape", () => {
  const episodes = explore("Hello\n* [a]\n    A\n    -> END\n* [b]\n    B\n    -> END\n");
  assert.equal(episodes.length, 2);
  assert.deepEqual(
    episodes.map((e) => e.choice_path),
    [[0], [1]],
  );
  for (const ep of episodes) {
    assert.equal(ep.outcome, "Ended");
    assert.deepEqual(Object.keys(ep), ["steps", "outcome", "choice_path", "initial_state"]);
    assert.deepEqual(Object.keys(ep.steps[0]), [
      "text",
      "tags",
      "outcome",
      "variable_changes",
      "visit_changes",
      "turn_index",
    ]);
  }
  const first = episodes[0].steps[0];
  assert.equal(first.text, "Hello\n");
  assert.deepEqual(first.outcome, {
    Choices: {
      presented: [
        { text: "a", index: 0, tags: [] },
        { text: "b", index: 1, tags: [] },
      ],
      selected: 0,
    },
  });
  assert.equal(episodes[0].steps.at(-1).text, "A\n");
  assert.equal(episodes[0].steps.at(-1).outcome, "Ended");
  assert.equal(episodes[1].steps.at(-1).text, "B\n");
});

test("variable changes, visit counts and the initial state are recorded", () => {
  // `{k}` reads the count, so inklecate marks `k` as counted (the C# tool
  // compiles with countAllVisits = false, as does this one).
  const [ep] = explore("VAR n = 1\n-> k\n== k ==\n~ n = n + 1\n{n} {k}\n-> END\n");
  assert.deepEqual(ep.initial_state, { variables: { n: 1 }, turn_index: 0 });
  assert.deepEqual(ep.steps[0].variable_changes, { n: 2 });
  assert.deepEqual(ep.steps[0].visit_changes, { k: 1 });
  assert.equal(ep.steps[0].text, "2 1\n");
});

test("shuffles draw from the .NET generator: the C# shuffle golden is reproduced", () => {
  // The golden was crawled by the C# runtime (System.Random at storySeed
  // 0); inkjs's stock Park–Miller generator picks different elements.
  const caseDir = join(repoRoot, "tests", "tier2", "conditional", "shuffle");
  const source = readFileSync(join(caseDir, "story.ink"), "utf8");
  const episodes = new Explorer(compileStory(source, caseDir, "story.ink")).explore();
  const goldenFiles = readdirSync(join(caseDir, "oracle"))
    .filter((f) => f.endsWith(".oracle.json"))
    .sort();
  assert.equal(episodes.length, goldenFiles.length);
  goldenFiles.forEach((file, i) => {
    const golden = JSON.parse(readFileSync(join(caseDir, "oracle", file), "utf8"));
    assert.deepEqual(JSON.parse(JSON.stringify(episodes[i])), golden, file);
  });
  assert.match(source, /\{~|shuffle:/, "the fixture is a shuffle");
});

test("a runtime error ends the episode with the bare ink message by default", () => {
  const [ep] = explore("-> k\n== k ==\nHello\n");
  assert.deepEqual(ep.outcome, {
    Error: "RUNTIME ERROR: 'inline.ink' line 3: ran out of content. Do you need a '-> DONE' or '-> END'?",
  });
  // Raise-on-discovery: the C# runtime (and inkjs) raise inside the same
  // Continue() that would have delivered `Hello`, so no step is recorded —
  // the shape `diff_oracle`'s RanOutOfContent allowance (RULED 2026-08-01,
  // #1574) exists for.
  assert.equal(ep.steps.length, 0);
  // The C# tool's default (no onError handler) wraps the same message.
  const [strict] = explore("-> k\n== k ==\nHello\n", { strictWarnings: true });
  assert.match(strict.outcome.Error, /^Ink had 1 error\. It is strongly suggested/);
  assert.match(strict.outcome.Error, /ran out of content/);
});

test("a compile error throws CompileError carrying inklecate's messages", () => {
  assert.throws(
    () => compileStory("-> nowhere\n", here, "inline.ink"),
    (e) => e instanceof CompileError && e.messages.some((m) => /nowhere/.test(m)),
  );
});

test("valueToJson renders lists and divert targets the way the C# tool does", () => {
  const [ep] = explore(
    "LIST l = (a), b, (c)\nVAR t = -> k\n-> k\n== k ==\n~ l += b\n~ t = -> k\n-> END\n",
  );
  assert.deepEqual(ep.initial_state.variables, { l: "a, c", t: "k" });
  // ink notifies observers on every assignment, an unchanged value included.
  assert.deepEqual(ep.steps[0].variable_changes, { l: "a, b, c", t: "k" });
  assert.equal(valueToJson(null), null);
  assert.equal(valueToJson(2.5), 2.5);
});
