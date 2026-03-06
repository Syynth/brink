// inkjs benchmark driver.
// Usage: node driver.mjs <story.ink.json> <input.txt> [--iterations N]
//
// Input file is 1-indexed (one choice number per line).
// Reports total and average time to stderr.

import { readFileSync } from "node:fs";
import { Story } from "inkjs";

const args = process.argv.slice(2);
if (args.length < 2) {
  console.error("Usage: node driver.mjs <story.ink.json> <input.txt> [--iterations N]");
  process.exit(1);
}

const storyPath = args[0];
const inputPath = args[1];
const iterIdx = args.indexOf("--iterations");
const iterations = iterIdx !== -1 ? parseInt(args[iterIdx + 1], 10) : 1;

const jsonStr = readFileSync(storyPath, "utf-8");
const inputLines = readFileSync(inputPath, "utf-8")
  .split("\n")
  .filter((l) => l.trim() !== "")
  .map((l) => parseInt(l.trim(), 10));

function runOnce() {
  const story = new Story(jsonStr);
  story.allowExternalFunctionFallbacks = true;
  let inputIdx = 0;

  while (story.canContinue || story.currentChoices.length > 0) {
    while (story.canContinue) {
      story.Continue();
    }
    if (story.currentChoices.length > 0) {
      if (inputIdx >= inputLines.length) break;
      const choiceNum = inputLines[inputIdx];
      inputIdx++;
      // Input is 1-indexed, inkjs uses 0-indexed
      story.ChooseChoiceIndex(choiceNum - 1);
    }
  }
}

const start = performance.now();
for (let i = 0; i < iterations; i++) {
  runOnce();
}
const elapsed = performance.now() - start;

console.error(
  `inkjs: ${iterations} iterations in ${(elapsed / 1000).toFixed(3)}s (${(elapsed / iterations).toFixed(3)}ms avg)`,
);
