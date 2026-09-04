// A port of tools/ink-oracle's Explorer.cs onto inkjs (#3379,
// docs/program-generator-spec.md §6).
//
// Same crawl, same episode shape: a depth-first walk over every choice
// (state snapshot via `state.ToJson()` / `LoadJson()`), one `OracleStep` per
// `Continue()`, variable changes via observers, visit-count diffs over every
// named container, `storySeed = 0`, `allowExternalFunctionFallbacks = true`.
// `OracleEpisode.cs`'s wire shape is reproduced field for field so an
// inkjs episode is `JSON`-comparable against a checked-in `*.oracle.json`.
//
// What this is NOT: a replacement for the C# oracle. It is the rank-3
// reference's nearest stand-in for sessions that have `node` but no
// `dotnet`; the goldens stay C#-blessed. Where the two runtimes disagree,
// the C# golden wins and the inkjs sanction (`brink-test-harness`) records
// the divergence — see the module comment in `dotnet-random.mjs` for the one
// divergence this tool removes at the source (the PRNG).

import { createRequire } from "node:module";

import { installDotNetRandom } from "./dotnet-random.mjs";

const require = createRequire(import.meta.url);

// Install the .NET generator BEFORE loading the compiler, so nothing can have
// captured the stock export (Story.js reads `PRNG_1.PRNG` at each call site,
// so ordering is not strictly required — but it keeps the swap obvious).
installDotNetRandom();

const { Compiler, CompilerOptions } = require("inkjs/compiler/Compiler");
const { PosixFileHandler } = require("inkjs/compiler/FileHandler/PosixFileHandler");
const { ErrorType } = require("inkjs/engine/Error");
const { InkList } = require("inkjs/engine/InkList");
const { Path } = require("inkjs/engine/Path");

export const DEFAULT_CONFIG = Object.freeze({
  maxDepth: 20,
  maxEpisodes: 1000,
  maxStepsPerEpisode: 10_000,
});

/** Thrown by {@link compileStory} when inklecate-on-js rejects the source. */
export class CompileError extends Error {
  /** @param {string[]} messages */
  constructor(messages) {
    super(messages[0] ?? "Compilation failed.");
    this.name = "CompileError";
    this.messages = messages;
  }
}

/**
 * Compile `source` the way Program.cs does: INCLUDEs resolve against
 * `storyDir`, compile errors are collected (not printed), and the resulting
 * story has external-function fallbacks enabled.
 *
 * @param {string} source
 * @param {string} storyDir
 * @param {string} sourceFilename
 */
export function compileStory(source, storyDir, sourceFilename) {
  const errors = [];
  const errorHandler = (message, type) => {
    if (type === ErrorType.Error) errors.push(message);
  };
  const options = new CompilerOptions(
    sourceFilename,
    [],
    false,
    errorHandler,
    new PosixFileHandler(storyDir),
  );
  const compiler = new Compiler(source, options);
  let story;
  try {
    story = compiler.Compile();
  } catch (e) {
    if (errors.length === 0) errors.push(e instanceof Error ? e.message : String(e));
    throw new CompileError(errors);
  }
  if (!story || errors.length > 0) throw new CompileError(errors);
  story.allowExternalFunctionFallbacks = true;
  return story;
}

/** `Explorer.ToJsonNode` — the JSON encoding of a runtime value. */
export function valueToJson(value) {
  if (value === null || value === undefined) return null;
  if (typeof value === "number" || typeof value === "boolean" || typeof value === "string") {
    return value;
  }
  if (value instanceof InkList) return value.toString();
  if (value instanceof Path) return value.toString();
  // A wrapped `Value` (observer callbacks already unwrap, but be tolerant).
  if (typeof value === "object" && "valueObject" in value) return valueToJson(value.valueObject);
  return String(value);
}

/** A choice as the golden records it. */
function choiceRecord(choice) {
  return { text: choice.text, index: choice.index, tags: choice.tags ?? [] };
}

/**
 * The DFS crawler. `strictWarnings` mirrors the C# tool's default (no
 * `onError` handler, so a runtime WARNING throws and ends the episode);
 * the default here is the C# tool's `--warn-and-continue` — the mode the
 * curated goldens are regenerated under (see Explorer.cs's comment on
 * I010-temp-not-found).
 */
export class Explorer {
  /**
   * @param {import("inkjs/engine/Story").Story} story
   * @param {Partial<typeof DEFAULT_CONFIG>} config
   * @param {{ strictWarnings?: boolean }} options
   */
  constructor(story, config = {}, { strictWarnings = false } = {}) {
    this.story = story;
    this.config = { ...DEFAULT_CONFIG, ...config };
    this.containerPaths = collectContainerPaths(story.mainContentContainer, "");
    this.episodes = [];
    this.warnAndContinue = !strictWarnings;
    this.pendingErrors = [];
    /** Every runtime warning swallowed in warn-and-continue mode. */
    this.warnings = [];

    if (this.warnAndContinue) {
      story.onError = (message, type) => {
        if (type === ErrorType.Error) this.pendingErrors.push(message);
        else this.warnings.push(message);
      };
    }
  }

  explore() {
    // Deterministic seed — the goldens were all crawled at 0.
    this.story.state.storySeed = 0;
    const initialState = this.snapshotInitialState();
    this.exploreInner(initialState, [], [], 0);
    return this.episodes;
  }

  exploreInner(initialState, steps, choicePath, depth) {
    if (this.episodes.length >= this.config.maxEpisodes) return;

    const { steps: newSteps, terminal } = this.runUntilTerminal();
    const allSteps = [...steps, ...newSteps];

    switch (terminal.kind) {
      case "choices": {
        const presented = terminal.choices.map(choiceRecord);

        if (depth >= this.config.maxDepth || this.episodes.length >= this.config.maxEpisodes) {
          this.setLastStepOutcome(allSteps, { Choices: { presented, selected: 0 } });
          this.episodes.push({
            steps: allSteps,
            outcome: { InputsExhausted: { remaining_choices: presented } },
            choice_path: [...choicePath],
            initial_state: initialState,
          });
          return;
        }

        const savedState = this.story.state.ToJson();

        for (let i = 0; i < terminal.choices.length; i++) {
          if (this.episodes.length >= this.config.maxEpisodes) return;

          this.story.state.LoadJson(savedState);
          this.story.state.ResetErrors();

          const branchSteps = [...allSteps];
          this.setLastStepOutcome(branchSteps, { Choices: { presented, selected: i } });

          const branchPath = [...choicePath, i];
          this.story.ChooseChoiceIndex(i);

          this.exploreInner(initialState, branchSteps, branchPath, depth + 1);
        }
        break;
      }

      case "ended": {
        this.setLastStepOutcome(allSteps, "Ended");
        this.episodes.push({
          steps: allSteps,
          outcome: "Ended",
          choice_path: [...choicePath],
          initial_state: initialState,
        });
        break;
      }

      case "error": {
        this.episodes.push({
          steps: allSteps,
          outcome: { Error: terminal.error },
          choice_path: [...choicePath],
          initial_state: initialState,
        });
        break;
      }

      default:
        throw new Error(`unreachable terminal ${String(terminal.kind)}`);
    }
  }

  /**
   * Replace the last step with a copy carrying `outcome`; when there are no
   * steps (choices appeared immediately) insert a synthetic empty step.
   * Copies rather than mutates so sibling branches never alias.
   */
  setLastStepOutcome(steps, outcome) {
    if (steps.length === 0) {
      steps.push({
        text: "",
        tags: [],
        outcome,
        variable_changes: {},
        visit_changes: {},
        turn_index: this.story.state.currentTurnIndex + 1,
      });
    } else {
      const last = steps[steps.length - 1];
      steps[steps.length - 1] = { ...last, outcome };
    }
  }

  /** One `Continue()` per step until choices or termination. */
  runUntilTerminal() {
    const story = this.story;
    const steps = [];
    const variableChanges = {};
    let stepCount = 0;

    const onVariableChanged = (name, newValue) => {
      variableChanges[name] = valueToJson(newValue);
    };
    for (const name of globalVariableNames(story)) {
      story.ObserveVariable(name, onVariableChanged);
    }
    const removeObservers = () => story.RemoveVariableObserver(onVariableChanged);

    try {
      while (story.canContinue) {
        if (stepCount++ > this.config.maxStepsPerEpisode) {
          removeObservers();
          return {
            steps,
            terminal: { kind: "error", error: `Step limit exceeded (${this.config.maxStepsPerEpisode})` },
          };
        }

        const visitsBefore = this.snapshotVisitCounts();
        for (const key of Object.keys(variableChanges)) delete variableChanges[key];
        this.pendingErrors.length = 0;

        story.Continue();

        if (this.warnAndContinue) {
          if (this.pendingErrors.length > 0) {
            removeObservers();
            return { steps, terminal: { kind: "error", error: this.pendingErrors.join("; ") } };
          }
        } else if (story.hasError) {
          removeObservers();
          return { steps, terminal: { kind: "error", error: (story.currentErrors ?? []).join("; ") } };
        }

        const visitsAfter = this.snapshotVisitCounts();
        const visitChanges = diffVisitCounts(visitsBefore, visitsAfter);

        steps.push({
          text: story.currentText ?? "",
          tags: [...(story.currentTags ?? [])],
          outcome: "Continue",
          variable_changes: { ...variableChanges },
          visit_changes: visitChanges,
          turn_index: story.state.currentTurnIndex + 1,
        });
      }
    } catch (e) {
      removeObservers();
      return { steps, terminal: { kind: "error", error: e instanceof Error ? e.message : String(e) } };
    }

    removeObservers();

    const choices = story.currentChoices;
    if (choices.length > 0) return { steps, terminal: { kind: "choices", choices } };
    return { steps, terminal: { kind: "ended" } };
  }

  snapshotInitialState() {
    const variables = {};
    for (const name of globalVariableNames(this.story)) {
      variables[name] = valueToJson(this.story.variablesState.$(name));
    }
    return { variables, turn_index: this.story.state.currentTurnIndex + 1 };
  }

  snapshotVisitCounts() {
    const counts = new Map();
    for (const path of this.containerPaths) {
      const count = this.story.state.VisitCountAtPathString(path) ?? 0;
      if (count > 0) counts.set(path, count);
    }
    return counts;
  }
}

/** The story's global variable names, in declaration order. */
function globalVariableNames(story) {
  // `variablesState` is a Proxy whose `ownKeys` trap enumerates the globals;
  // C#'s `IEnumerable<string>` walks the same dictionary.
  return Object.keys(story.variablesState);
}

/** @returns {Record<string, number>} paths whose count changed, keyed in `after`'s order */
export function diffVisitCounts(before, after) {
  const diff = {};
  for (const [path, afterCount] of after) {
    if (afterCount !== (before.get(path) ?? 0)) diff[path] = afterCount;
  }
  return diff;
}

/** Every named container path, depth-first, parent before children. */
export function collectContainerPaths(container, parentPath) {
  const paths = [];
  for (const [name, child] of container.namedContent) {
    // Only containers carry visit counts; `INamedContent` may be anything.
    if (!child || typeof child !== "object" || !(child.namedContent instanceof Map)) continue;
    const childPath = parentPath === "" ? name : `${parentPath}.${name}`;
    paths.push(childPath);
    paths.push(...collectContainerPaths(child, childPath));
  }
  return paths;
}
