using System.Text.Json.Nodes;
using Ink.Runtime;

namespace InkOracle;

public class ExploreConfig
{
    public int MaxDepth { get; set; } = 20;
    public int MaxEpisodes { get; set; } = 1000;
    public int MaxStepsPerEpisode { get; set; } = 10_000;
}

public class Explorer
{
    private readonly Story _story;
    private readonly ExploreConfig _config;
    private readonly List<string> _containerPaths;
    private readonly List<OracleEpisode> _episodes = new();

    public Explorer(Story story, ExploreConfig? config = null)
    {
        _story = story;
        _config = config ?? new ExploreConfig();
        _containerPaths = CollectContainerPaths(story.mainContentContainer, "");
    }

    public List<OracleEpisode> Explore()
    {
        var initialState = SnapshotInitialState();

        ExploreInner(
            initialState: initialState,
            steps: new List<OracleStep>(),
            choicePath: new List<int>(),
            depth: 0
        );

        return _episodes;
    }

    private void ExploreInner(
        OracleInitialState initialState,
        List<OracleStep> steps,
        List<int> choicePath,
        int depth)
    {
        if (_episodes.Count >= _config.MaxEpisodes)
            return;

        // Run one "step" — accumulate Continue() calls until choices or termination.
        var stepResult = RunStep();

        switch (stepResult)
        {
            case StepResultChoices choicesResult:
            {
                var presented = choicesResult.Choices.Select(c => new OracleChoiceRecord
                {
                    Text = c.text,
                    Index = c.index,
                    Tags = c.tags ?? new List<string>()
                }).ToList();

                if (depth >= _config.MaxDepth || _episodes.Count >= _config.MaxEpisodes)
                {
                    var truncatedSteps = new List<OracleStep>(steps);
                    truncatedSteps.Add(StepWithOutcome(choicesResult.Step,
                        new OracleStepOutcomeChoices
                        {
                            Presented = presented,
                            Selected = 0
                        }));
                    _episodes.Add(new OracleEpisode
                    {
                        Steps = truncatedSteps,
                        Outcome = new OracleOutcomeInputsExhausted
                        {
                            RemainingChoices = presented
                        },
                        ChoicePath = new List<int>(choicePath),
                        InitialState = initialState
                    });
                    return;
                }

                // Save state for branching.
                var savedState = _story.state.ToJson();

                for (int i = 0; i < choicesResult.Choices.Count; i++)
                {
                    if (_episodes.Count >= _config.MaxEpisodes)
                        return;

                    // Restore state before each branch.
                    _story.state.LoadJson(savedState);

                    var branchSteps = new List<OracleStep>(steps);
                    branchSteps.Add(StepWithOutcome(choicesResult.Step,
                        new OracleStepOutcomeChoices
                        {
                            Presented = presented,
                            Selected = i
                        }));

                    var branchPath = new List<int>(choicePath) { i };

                    _story.ChooseChoiceIndex(i);

                    ExploreInner(initialState, branchSteps, branchPath, depth + 1);
                }

                break;
            }

            case StepResultDone doneResult:
            {
                var finalSteps = new List<OracleStep>(steps) { doneResult.Step };
                _episodes.Add(new OracleEpisode
                {
                    Steps = finalSteps,
                    Outcome = "Done",
                    ChoicePath = new List<int>(choicePath),
                    InitialState = initialState
                });
                break;
            }

            case StepResultEnded endedResult:
            {
                var finalSteps = new List<OracleStep>(steps) { endedResult.Step };
                _episodes.Add(new OracleEpisode
                {
                    Steps = finalSteps,
                    Outcome = "Ended",
                    ChoicePath = new List<int>(choicePath),
                    InitialState = initialState
                });
                break;
            }

            case StepResultError errorResult:
            {
                _episodes.Add(new OracleEpisode
                {
                    Steps = new List<OracleStep>(steps),
                    Outcome = new OracleOutcomeError { Error = errorResult.Error },
                    ChoicePath = new List<int>(choicePath),
                    InitialState = initialState
                });
                break;
            }
        }
    }

    /// <summary>
    /// Runs Continue() in a loop until choices or termination.
    /// Tracks variable changes, visit count diffs, and turn index.
    /// </summary>
    private StepResult RunStep()
    {
        var variableChanges = new Dictionary<string, JsonNode?>();
        var visitsBefore = SnapshotVisitCounts();
        int turnBefore = _story.state.currentTurnIndex;

        // Track variable changes via observer.
        void OnVariableChanged(string name, object newValue)
        {
            variableChanges[name] = ToJsonNode(newValue);
        }

        // Register observer for all variables.
        foreach (var varName in _story.variablesState)
        {
            _story.ObserveVariable(varName, OnVariableChanged);
        }

        var textParts = new List<(string text, List<string> tags)>();
        int stepCount = 0;

        try
        {
            while (_story.canContinue)
            {
                if (stepCount++ > _config.MaxStepsPerEpisode)
                {
                    RemoveAllObservers(OnVariableChanged);
                    return new StepResultError($"Step limit exceeded ({_config.MaxStepsPerEpisode})");
                }

                _story.Continue();
                textParts.Add((_story.currentText, new List<string>(_story.currentTags)));
            }

            // Check for errors after continuing.
            if (_story.hasError)
            {
                RemoveAllObservers(OnVariableChanged);
                var errors = string.Join("; ", _story.currentErrors);
                return new StepResultError(errors);
            }
        }
        catch (Exception ex)
        {
            RemoveAllObservers(OnVariableChanged);
            return new StepResultError(ex.Message);
        }

        RemoveAllObservers(OnVariableChanged);

        // Build text and per-line tags.
        var (text, tags) = BuildTextAndTags(textParts);

        // Diff visit counts.
        var visitsAfter = SnapshotVisitCounts();
        var visitChanges = DiffVisitCounts(visitsBefore, visitsAfter);
        // C# ink runtime starts turnIndex at -1; brink starts at 0. Normalize by adding 1.
        int turnAfter = _story.state.currentTurnIndex + 1;

        var step = new OracleStep
        {
            Text = text,
            Tags = tags,
            VariableChanges = variableChanges,
            VisitChanges = visitChanges,
            TurnIndex = turnAfter
        };

        // Determine outcome.
        var choices = _story.currentChoices;
        if (choices.Count > 0)
        {
            return new StepResultChoices(step, choices);
        }

        // No choices — check if story has ended or just paused.
        // If we can't continue and there are no choices, the story is done.
        // The ink runtime doesn't distinguish "done" vs "ended" explicitly in its API,
        // but if the story has no more content and no choices, it's ended.
        // We check if we got any output at all — if we did, it ended; if not, it's done.
        // Actually, ink uses the concept: if the flow reaches the end of all content, it's "ended".
        // canContinue == false && choices.Count == 0 means the story is over.
        // We'll mark it as "Ended" since the C# runtime has reached end of content.
        step.Outcome = new OracleStepOutcomeEnded();
        return new StepResultEnded(step);
    }

    private void RemoveAllObservers(Story.VariableObserver observer)
    {
        _story.RemoveVariableObserver(observer);
    }

    private OracleInitialState SnapshotInitialState()
    {
        var variables = new Dictionary<string, JsonNode?>();
        foreach (var varName in _story.variablesState)
        {
            variables[varName] = ToJsonNode(_story.variablesState[varName]);
        }

        return new OracleInitialState
        {
            Variables = variables,
            TurnIndex = _story.state.currentTurnIndex + 1
        };
    }

    private Dictionary<string, int> SnapshotVisitCounts()
    {
        var counts = new Dictionary<string, int>();
        foreach (var path in _containerPaths)
        {
            int count = _story.state.VisitCountAtPathString(path);
            if (count > 0)
            {
                counts[path] = count;
            }
        }
        return counts;
    }

    private static Dictionary<string, int> DiffVisitCounts(
        Dictionary<string, int> before,
        Dictionary<string, int> after)
    {
        var diff = new Dictionary<string, int>();
        foreach (var (path, afterCount) in after)
        {
            before.TryGetValue(path, out int beforeCount);
            if (afterCount != beforeCount)
            {
                diff[path] = afterCount;
            }
        }
        return diff;
    }

    /// <summary>
    /// Build concatenated text and per-text-line tags from accumulated Continue() outputs.
    /// Matches brink's build_per_line_tags logic:
    /// - Filter out empty-text parts
    /// - For each part, count newlines. The first segment gets the part's tags.
    /// - For non-final parts, trailing \n is consumed by the next part (no extra segment).
    /// - For the final part, trailing \n does create an extra empty-tag segment.
    /// </summary>
    private static (string text, List<List<string>> tags) BuildTextAndTags(
        List<(string text, List<string> tags)> parts)
    {
        var fullText = string.Join("", parts.Select(p => p.text));
        var perLineTags = new List<List<string>>();

        // Filter out empty-text parts (matches brink's non_empty filter).
        var nonEmpty = parts.Where(p => !string.IsNullOrEmpty(p.text)).ToList();

        for (int i = 0; i < nonEmpty.Count; i++)
        {
            var (partText, partTags) = nonEmpty[i];
            bool isLast = i == nonEmpty.Count - 1;

            int newlineCount = partText.Count(c => c == '\n');
            int extraSegments = isLast ? newlineCount : Math.Max(0, newlineCount - 1);

            perLineTags.Add(new List<string>(partTags));
            for (int j = 0; j < extraSegments; j++)
            {
                perLineTags.Add(new List<string>());
            }
        }

        // When there are no non-empty parts, brink returns an empty tags vec.
        // Do not add a placeholder — match brink's behavior.

        return (fullText, perLineTags);
    }

    /// <summary>
    /// Walk the story's container tree to collect all named container paths.
    /// These are used for visit count tracking.
    /// </summary>
    private static List<string> CollectContainerPaths(Container container, string parentPath)
    {
        var paths = new List<string>();

        foreach (var kvp in container.namedContent)
        {
            if (kvp.Value is Container child)
            {
                var childPath = string.IsNullOrEmpty(parentPath)
                    ? kvp.Key
                    : $"{parentPath}.{kvp.Key}";
                paths.Add(childPath);
                paths.AddRange(CollectContainerPaths(child, childPath));
            }
        }

        return paths;
    }

    private static OracleStep StepWithOutcome(OracleStep source, OracleStepOutcome outcome)
    {
        return new OracleStep
        {
            Text = source.Text,
            Tags = source.Tags,
            Outcome = outcome,
            VariableChanges = source.VariableChanges,
            VisitChanges = source.VisitChanges,
            TurnIndex = source.TurnIndex
        };
    }

    /// <summary>
    /// Convert ink runtime values to JsonNode for correct serialization.
    /// The C# ink runtime returns boxed primitives from variablesState[name],
    /// but ObserveVariable callbacks receive the raw .NET value.
    /// </summary>
    private static JsonNode? ToJsonNode(object? value)
    {
        return value switch
        {
            int i => JsonValue.Create(i),
            float f => JsonValue.Create(f),
            bool b => JsonValue.Create(b),
            string s => JsonValue.Create(s),
            Ink.Runtime.Value inkVal => ToJsonNode(inkVal.valueObject),
            InkList list => JsonValue.Create(list.ToString()),
            Ink.Runtime.Path p => JsonValue.Create(p.ToString()),
            null => null,
            _ => JsonValue.Create(value.ToString()!)
        };
    }
}

// --- Step result types ---

abstract record StepResult;
record StepResultChoices(OracleStep Step, List<Choice> Choices) : StepResult;
record StepResultDone(OracleStep Step) : StepResult;
record StepResultEnded(OracleStep Step) : StepResult;
record StepResultError(string Error) : StepResult;
