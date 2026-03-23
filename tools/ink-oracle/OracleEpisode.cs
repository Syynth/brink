using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.Json.Serialization;

namespace InkOracle;

/// <summary>
/// A complete recorded execution of a story from start to termination.
/// Uses string names for all identifiers (variables, container paths).
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
/// A single step: one ContinueMaximally()-equivalent call's output.
/// We accumulate Continue() calls until choices or termination.
/// </summary>
public class OracleStep
{
    [JsonPropertyName("text")]
    public string Text { get; set; } = "";

    [JsonPropertyName("tags")]
    public List<List<string>> Tags { get; set; } = new();

    [JsonPropertyName("outcome")]
    [JsonConverter(typeof(StepOutcomeConverter))]
    public OracleStepOutcome Outcome { get; set; } = new OracleStepOutcomeDone();

    [JsonPropertyName("variable_changes")]
    public Dictionary<string, JsonNode?> VariableChanges { get; set; } = new();

    [JsonPropertyName("visit_changes")]
    public Dictionary<string, int> VisitChanges { get; set; } = new();

    [JsonPropertyName("turn_index")]
    public int TurnIndex { get; set; }
}

public abstract class OracleStepOutcome { }

public class OracleStepOutcomeDone : OracleStepOutcome { }
public class OracleStepOutcomeEnded : OracleStepOutcome { }

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

public class OracleOutcomeDone
{
    public string Type => "Done";
}

public class OracleOutcomeEnded
{
    public string Type => "Ended";
}

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

/// <summary>
/// Serializes step outcomes to match brink's serde format:
///   "Done", "Ended", or { "Choices": { "presented": [...], "selected": N } }
/// </summary>
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

/// <summary>
/// Serializes episode-level outcomes to match brink's serde format.
/// </summary>
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
