using System;
using System.Collections.Generic;
using System.Linq;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Text.RegularExpressions;

namespace AgentUI;

internal sealed record Field(string Id, string Label, string Type, bool Required = false,
    string[]? Options = null, double? Min = null, double? Max = null);

internal sealed record Question(string QuestionText, string? Context = null, string? Project = null,
    string[]? Options = null, Field[]? Fields = null);

internal sealed record Answer(string SubmissionId, string Action, string? Choice = null,
    Dictionary<string, JsonElement>? Values = null, string? Comment = null);

internal sealed record Delivery(string State = "available", int Attempts = 0,
    string? Error = null, DateTimeOffset? NextAttempt = null);

internal sealed record Feedback
{
    public int SchemaVersion { get; init; } = 1;
    public required string Id { get; init; }
    public required string Session { get; init; }
    public required string Agent { get; init; }
    public required Question Question { get; init; }
    public string? ThreadId { get; init; }
    public string Status { get; init; } = "pending";
    public DateTimeOffset CreatedAt { get; init; } = DateTimeOffset.UtcNow;
    public DateTimeOffset? AnsweredAt { get; init; }
    public Answer? Answer { get; init; }
    public Delivery Delivery { get; init; } = new();
}

internal sealed record Draft(Dictionary<string, string> Values, string Comment = "", string? Choice = null);
internal sealed record PanelState(string? SelectedId = null, bool Hidden = false, bool Muted = false,
    bool Pinned = false, string[]? Later = null);

internal static class Json
{
    internal static readonly JsonSerializerOptions Options = new(JsonSerializerDefaults.Web)
    {
        WriteIndented = false,
        UnmappedMemberHandling = JsonUnmappedMemberHandling.Disallow
    };
    internal static string Encode<T>(T value) => JsonSerializer.Serialize(value, Options);
    internal static T Decode<T>(string value) => JsonSerializer.Deserialize<T>(value, Options)
        ?? throw new ArgumentException("Expected a JSON object.");
}

internal static class Validate
{
    internal static void Text(string? value, string label, int maximum, bool required = false)
    {
        if (required && string.IsNullOrWhiteSpace(value)) throw new ArgumentException($"{label} is required.");
        if (value?.Length > maximum) throw new ArgumentException($"{label} must be at most {maximum} characters.");
    }

    internal static void Question(Question question)
    {
        Text(question.QuestionText, "Question", 2000, true);
        Text(question.Context, "Context", 12000);
        Text(question.Project, "Project", 160);
        Choices(question.Options);
        if (question.Fields?.Length > 8) throw new ArgumentException("Use at most eight fields.");
        var ids = new HashSet<string>();
        foreach (var field in question.Fields ?? [])
        {
            if (field is null) throw new ArgumentException("Fields cannot contain null.");
            if (!Regex.IsMatch(field.Id ?? "", "^[a-z][a-z0-9_]{0,39}$") || !ids.Add(field.Id!))
                throw new ArgumentException("Field IDs must be unique snake_case identifiers.");
            Text(field.Label, "Field label", 160, true);
            if (field.Type is not ("text" or "number" or "boolean" or "choice"))
                throw new ArgumentException($"Unknown field type: {field.Type}.");
            if (field.Type == "choice" && (field.Options?.Length ?? 0) < 2)
                throw new ArgumentException($"{field.Label} needs at least two choices.");
            Choices(field.Options);
            if (field.Type != "choice" && field.Options is not null)
                throw new ArgumentException("Only choice fields accept options.");
            if (field.Type != "number" && (field.Min is not null || field.Max is not null))
                throw new ArgumentException("Only number fields accept min/max.");
            if (field.Min > field.Max) throw new ArgumentException("Minimum cannot exceed maximum.");
        }
    }

    private static void Choices(string[]? options)
    {
        if (options is null) return;
        if (options.Length is < 2 or > 8 || options.Distinct().Count() != options.Length)
            throw new ArgumentException("Provide two to eight unique choices.");
        foreach (var option in options) Text(option, "Choice", 240, true);
    }

    internal static void Answer(Question question, Answer answer)
    {
        if (!Guid.TryParseExact(answer.SubmissionId, "N", out _))
            throw new ArgumentException("Invalid submission ID.");
        Text(answer.Comment, "Comment", 6000);
        if (answer.Action == "dismiss") return;
        if (answer.Action != "answer") throw new ArgumentException("Action must be answer or dismiss.");
        if (question.Options is not null && !question.Options.Contains(answer.Choice))
            throw new ArgumentException("Choose one of the offered answers.");
        if (question.Options is null && answer.Choice is not null)
            throw new ArgumentException("This question does not have choices.");
        var values = answer.Values ?? new();
        var fields = question.Fields ?? [];
        if (values.Keys.Any(id => !fields.Any(f => f.Id == id)))
            throw new ArgumentException("Response contains an unknown field.");
        foreach (var field in fields)
        {
            if (!values.TryGetValue(field.Id, out var value) || value.ValueKind == JsonValueKind.Null)
            {
                if (field.Required) throw new ArgumentException($"{field.Label} is required.");
                continue;
            }
            switch (field.Type)
            {
                case "text":
                    if (value.ValueKind != JsonValueKind.String) throw new ArgumentException($"{field.Label} must be text.");
                    Text(value.GetString(), field.Label, 4000, field.Required);
                    break;
                case "number":
                    if (value.ValueKind != JsonValueKind.Number || !value.TryGetDouble(out var number) || double.IsNaN(number) || double.IsInfinity(number) || number < field.Min || number > field.Max)
                        throw new ArgumentException($"{field.Label} must be a number within the offered range.");
                    break;
                case "boolean":
                    if (value.ValueKind is not (JsonValueKind.True or JsonValueKind.False))
                        throw new ArgumentException($"{field.Label} must be yes or no.");
                    break;
                case "choice":
                    if (value.ValueKind != JsonValueKind.String || !field.Options!.Contains(value.GetString()))
                        throw new ArgumentException($"Choose an option for {field.Label}.");
                    break;
            }
        }
        if (question.Options is null && fields.Length == 0 && string.IsNullOrWhiteSpace(answer.Comment))
            throw new ArgumentException("Write a response before sending.");
    }
}
