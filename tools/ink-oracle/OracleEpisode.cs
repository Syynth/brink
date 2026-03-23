using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.Json.Serialization;

namespace InkOracle;

/// <summary>
/// A complete recorded execution of a story from start to termination.
/// Each step corresponds to one Continue() call.
/// </summary>
public class OracleEpisode
{
    [JsonPropertyName("steps")]
    public List<OracleStep> Steps { get; set; } = new();

    [JsonPropertyName("outcome")]
    public object Outcome { get; set; } = "Done";

    [JsonPropertyName("choice_path")]
    public List<int> ChoicePath { get; set; } = new();

    [JsonPropertyName("initial_state")]
    public OracleInitialState InitialState { get; set; } = new();
}

/// <summary>
/// A single step: one Continue() call's output and state changes.
/// </summary>
public class OracleStep
{
    [JsonPropertyName("text")]
    public string Text { get; set; } = "";

    [JsonPropertyName("tags")]
    public List<string> Tags { get; set; } = new();

    [JsonPropertyName("outcome")]
    [JsonConverter(typeof(StepOutcomeConverter))]
    public OracleStepOutcome Outcome { get; set; } = new OracleStepOutcomeContinue();

    [JsonPropertyName("variable_changes")]
    public Dictionary<string, JsonNode?> VariableChanges { get; set; } = new();

    [JsonPropertyName("visit_changes")]
    public Dictionary<string, int> VisitChanges { get; set; } = new();

    [JsonPropertyName("turn_index")]
    public int TurnIndex { get; set; }
}

public abstract class OracleStepOutcome { }

/// <summary>More Continue() calls available.</summary>
public class OracleStepOutcomeContinue : OracleStepOutcome { }

/// <summary>Story paused — no more content, no choices.</summary>
public class OracleStepOutcomeDone : OracleStepOutcome { }

/// <summary>Story permanently ended.</summary>
public class OracleStepOutcomeEnded : OracleStepOutcome { }

/// <summary>Choices presented.</summary>
public class OracleStepOutcomeChoices : OracleStepOutcome
{
    public List<OracleChoiceRecord> Presented { get; set; } = new();
    public int Selected { get; set; }
}

public class OracleChoiceRecord
{
    [JsonPropertyName("text")]
    public string Text { get; set; } = "";

    [JsonPropertyName("index")]
    public int Index { get; set; }

    [JsonPropertyName("tags")]
    public List<string> Tags { get; set; } = new();
}

public class OracleInitialState
{
    [JsonPropertyName("variables")]
    public Dictionary<string, JsonNode?> Variables { get; set; } = new();

    [JsonPropertyName("turn_index")]
    public int TurnIndex { get; set; }
}

// --- Outcome types for the episode level ---

public class OracleOutcomeInputsExhausted
{
    [JsonPropertyName("remaining_choices")]
    public List<OracleChoiceRecord> RemainingChoices { get; set; } = new();
}

public class OracleOutcomeError
{
    [JsonPropertyName("error")]
    public string Error { get; set; } = "";
}

// --- JSON converters ---

public class StepOutcomeConverter : JsonConverter<OracleStepOutcome>
{
    public override OracleStepOutcome Read(ref Utf8JsonReader reader, Type typeToConvert, JsonSerializerOptions options)
    {
        throw new NotImplementedException("Deserialization not needed");
    }

    public override void Write(Utf8JsonWriter writer, OracleStepOutcome value, JsonSerializerOptions options)
    {
        switch (value)
        {
            case OracleStepOutcomeContinue:
                writer.WriteStringValue("Continue");
                break;
            case OracleStepOutcomeDone:
                writer.WriteStringValue("Done");
                break;
            case OracleStepOutcomeEnded:
                writer.WriteStringValue("Ended");
                break;
            case OracleStepOutcomeChoices choices:
                writer.WriteStartObject();
                writer.WritePropertyName("Choices");
                writer.WriteStartObject();
                writer.WritePropertyName("presented");
                JsonSerializer.Serialize(writer, choices.Presented, options);
                writer.WriteNumber("selected", choices.Selected);
                writer.WriteEndObject();
                writer.WriteEndObject();
                break;
        }
    }
}

public class EpisodeOutcomeConverter : JsonConverter<object>
{
    public override object Read(ref Utf8JsonReader reader, Type typeToConvert, JsonSerializerOptions options)
    {
        throw new NotImplementedException("Deserialization not needed");
    }

    public override void Write(Utf8JsonWriter writer, object value, JsonSerializerOptions options)
    {
        switch (value)
        {
            case "Done":
                writer.WriteStringValue("Done");
                break;
            case "Ended":
                writer.WriteStringValue("Ended");
                break;
            case OracleOutcomeInputsExhausted exhausted:
                writer.WriteStartObject();
                writer.WritePropertyName("InputsExhausted");
                writer.WriteStartObject();
                writer.WritePropertyName("remaining_choices");
                JsonSerializer.Serialize(writer, exhausted.RemainingChoices, options);
                writer.WriteEndObject();
                writer.WriteEndObject();
                break;
            case OracleOutcomeError error:
                writer.WriteStartObject();
                writer.WriteString("Error", error.Error);
                writer.WriteEndObject();
                break;
            default:
                writer.WriteStringValue(value?.ToString() ?? "Done");
                break;
        }
    }
}
