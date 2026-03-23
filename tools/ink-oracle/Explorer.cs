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

        // Run Continue() calls one at a time, emitting a step for each.
        var (newSteps, terminal) = RunUntilTerminal();
        var allSteps = new List<OracleStep>(steps);
        allSteps.AddRange(newSteps);

        switch (terminal)
        {
            case TerminalChoices tc:
            {
                var presented = tc.Choices.Select(c => new OracleChoiceRecord
                {
                    Text = c.text,
                    Index = c.index,
                    Tags = c.tags ?? new List<string>()
                }).ToList();

                if (depth >= _config.MaxDepth || _episodes.Count >= _config.MaxEpisodes)
                {
                    // Mark the last step's outcome as choices.
                    SetLastStepOutcome(allSteps, new OracleStepOutcomeChoices
                    {
                        Presented = presented,
                        Selected = 0
                    });
                    _episodes.Add(new OracleEpisode
                    {
                        Steps = allSteps,
                        Outcome = new OracleOutcomeInputsExhausted
                        {
                            RemainingChoices = presented
                        },
                        ChoicePath = new List<int>(choicePath),
                        InitialState = initialState
                    });
                    return;
                }

                var savedState = _story.state.ToJson();

                for (int i = 0; i < tc.Choices.Count; i++)
                {
                    if (_episodes.Count >= _config.MaxEpisodes)
                        return;

                    _story.state.LoadJson(savedState);

                    var branchSteps = new List<OracleStep>(allSteps);
                    SetLastStepOutcome(branchSteps, new OracleStepOutcomeChoices
                    {
                        Presented = presented,
                        Selected = i
                    });

                    var branchPath = new List<int>(choicePath) { i };
                    _story.ChooseChoiceIndex(i);

                    ExploreInner(initialState, branchSteps, branchPath, depth + 1);
                }

                break;
            }

            case TerminalEnded:
            {
                SetLastStepOutcome(allSteps, new OracleStepOutcomeEnded());
                _episodes.Add(new OracleEpisode
                {
                    Steps = allSteps,
                    Outcome = "Ended",
                    ChoicePath = new List<int>(choicePath),
                    InitialState = initialState
                });
                break;
            }

            case TerminalDone:
            {
                SetLastStepOutcome(allSteps, new OracleStepOutcomeDone());
                _episodes.Add(new OracleEpisode
                {
                    Steps = allSteps,
                    Outcome = "Done",
                    ChoicePath = new List<int>(choicePath),
                    InitialState = initialState
                });
                break;
            }

            case TerminalError te:
            {
                _episodes.Add(new OracleEpisode
                {
                    Steps = allSteps,
                    Outcome = new OracleOutcomeError { Error = te.Error },
                    ChoicePath = new List<int>(choicePath),
                    InitialState = initialState
                });
                break;
            }
        }
    }

    /// <summary>
    /// Replace the last step with a copy that has the given outcome.
    /// If there are no steps (choices appeared immediately), insert a
    /// synthetic empty step. Creates a new object to avoid aliasing
    /// between branches.
    /// </summary>
    private void SetLastStepOutcome(List<OracleStep> steps, OracleStepOutcome outcome)
    {
        if (steps.Count == 0)
        {
            steps.Add(new OracleStep
            {
                Text = "",
                Tags = new List<string>(),
                Outcome = outcome,
                TurnIndex = _story.state.currentTurnIndex + 1
            });
        }
        else
        {
            var last = steps[^1];
            steps[^1] = new OracleStep
            {
                Text = last.Text,
                Tags = last.Tags,
                Outcome = outcome,
                VariableChanges = last.VariableChanges,
                VisitChanges = last.VisitChanges,
                TurnIndex = last.TurnIndex
            };
        }
    }

    /// <summary>
    /// Run Continue() calls one at a time until choices or termination.
    /// Returns one OracleStep per Continue() call, plus the terminal condition.
    /// </summary>
    private (List<OracleStep> steps, Terminal terminal) RunUntilTerminal()
    {
        var steps = new List<OracleStep>();
        var variableChanges = new Dictionary<string, JsonNode?>();
        int stepCount = 0;

        // Track variable changes via observer.
        void OnVariableChanged(string name, object newValue)
        {
            variableChanges[name] = ToJsonNode(newValue);
        }

        foreach (var varName in _story.variablesState)
        {
            _story.ObserveVariable(varName, OnVariableChanged);
        }

        try
        {
            while (_story.canContinue)
            {
                if (stepCount++ > _config.MaxStepsPerEpisode)
                {
                    RemoveAllObservers(OnVariableChanged);
                    return (steps, new TerminalError($"Step limit exceeded ({_config.MaxStepsPerEpisode})"));
                }

                // Snapshot state before this Continue().
                var visitsBefore = SnapshotVisitCounts();
                variableChanges.Clear();

                _story.Continue();

                // Check for errors after Continue().
                if (_story.hasError)
                {
                    RemoveAllObservers(OnVariableChanged);
                    var errors = string.Join("; ", _story.currentErrors);
                    return (steps, new TerminalError(errors));
                }

                // Diff state.
                var visitsAfter = SnapshotVisitCounts();
                var visitChanges = DiffVisitCounts(visitsBefore, visitsAfter);
                int turnIndex = _story.state.currentTurnIndex + 1;

                // Determine step outcome: if canContinue, more output coming.
                // Terminal outcomes (choices/ended/done) are set by the caller.
                var outcome = _story.canContinue
                    ? (OracleStepOutcome)new OracleStepOutcomeContinue()
                    : new OracleStepOutcomeContinue(); // placeholder, overwritten by caller

                steps.Add(new OracleStep
                {
                    Text = _story.currentText,
                    Tags = new List<string>(_story.currentTags),
                    Outcome = outcome,
                    VariableChanges = new Dictionary<string, JsonNode?>(variableChanges),
                    VisitChanges = visitChanges,
                    TurnIndex = turnIndex
                });
            }
        }
        catch (Exception ex)
        {
            RemoveAllObservers(OnVariableChanged);
            return (steps, new TerminalError(ex.Message));
        }

        RemoveAllObservers(OnVariableChanged);

        // Determine terminal condition.
        var choices = _story.currentChoices;
        if (choices.Count > 0)
        {
            return (steps, new TerminalChoices(choices));
        }

        // No choices, no more content.
        return (steps, new TerminalEnded());
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

// --- Terminal types ---

abstract record Terminal;
record TerminalChoices(List<Choice> Choices) : Terminal;
record TerminalDone : Terminal;
record TerminalEnded : Terminal;
record TerminalError(string Error) : Terminal;
