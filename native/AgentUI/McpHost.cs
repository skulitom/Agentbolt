using System;
using System.Linq;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using ModelContextProtocol.Protocol;
using ModelContextProtocol.Server;

namespace AgentUI;

internal static class McpHost
{
    private static Tool Define(string name, string description, string schema) => new()
    {
        Name = name, Description = description, InputSchema = JsonDocument.Parse(schema).RootElement.Clone()
    };

    private static readonly Tool[] Tools =
    [
        Define("feedback_request", "Ask the user through the desktop panel and return immediately. Continue useful work while they answer. No answer or default is consent.", """
        {"type":"object","properties":{"question":{"type":"string","maxLength":2000},"context":{"type":"string","maxLength":12000},"project":{"type":"string","maxLength":160},"options":{"type":"array","items":{"type":"string"},"minItems":2,"maxItems":8},"fields":{"type":"array","maxItems":8,"items":{"type":"object","properties":{"id":{"type":"string"},"label":{"type":"string"},"type":{"type":"string","enum":["text","number","boolean","choice"]},"required":{"type":"boolean"},"options":{"type":"array","items":{"type":"string"}},"min":{"type":"number"},"max":{"type":"number"}},"required":["id","label","type"],"additionalProperties":false}}},"required":["question"],"additionalProperties":false}
        """),
        Define("feedback_read", "Read feedback by ID. Reads are repeatable. Optionally wait up to 20 seconds when no independent work remains. Unanswered fields express no opinion.", """
        {"type":"object","properties":{"requestIds":{"type":"array","items":{"type":"string"},"minItems":1,"maxItems":30},"waitMs":{"type":"integer","minimum":0,"maximum":20000}},"required":["requestIds"],"additionalProperties":false}
        """),
        Define("feedback_cancel", "Cancel a question that no longer needs an answer. An existing answer is preserved.", """
        {"type":"object","properties":{"requestId":{"type":"string"},"reason":{"type":"string","maxLength":6000}},"required":["requestId"],"additionalProperties":false}
        """)
    ];

    internal static async Task Run(FeedbackStore store, Settings settings, CancellationToken stop)
    {
        var options = new McpServerOptions
        {
            ServerInfo = new() { Name = "AgentUI", Version = "0.2.0" },
            ServerInstructions = "Use feedback_request for decisions only the human can make. It returns immediately. " +
                "Collect replies with feedback_read. A native panel presents the question automatically. " +
                "A configured Codex connection also sends submitted answers to its originating task. " +
                "Feedback IDs identify decisions; repeated delivery of the same ID is the same answer.",
            Capabilities = new() { Tools = new() },
            Handlers = new()
            {
                ListToolsHandler = (_, _) => new ValueTask<ListToolsResult>(new ListToolsResult { Tools = Tools }),
                CallToolHandler = async (request, cancellation) =>
                {
                    try
                    {
                        var args = request.Params?.Arguments;
                        var definition = Tools.FirstOrDefault(t => t.Name == request.Params?.Name)
                            ?? throw new ArgumentException("Unknown feedback tool.");
                        if (args is not null && args.Keys.Any(key => !definition.InputSchema.GetProperty("properties").TryGetProperty(key, out _)))
                            throw new ArgumentException("Unknown tool argument.");
                        JsonElement Arg(string name) => args is not null && args.TryGetValue(name, out var value)
                            ? value : default;
                        string? String(string name) => Arg(name).ValueKind == JsonValueKind.Undefined ? null : Arg(name).GetString();
                        switch (request.Params?.Name)
                        {
                            case "feedback_request":
                                var question = new Question(String("question") ?? "", String("context"), String("project"),
                                    Arg("options").ValueKind == JsonValueKind.Undefined ? null : Arg("options").Deserialize<string[]>(Json.Options),
                                    Arg("fields").ValueKind == JsonValueKind.Undefined ? null : Arg("fields").Deserialize<Field[]>(Json.Options));
                                // A shared MCP process can serve several tasks. Prefer the calling
                                // host's per-request identity to the process environment.
                                var threadId = settings.CodexEnabled ? settings.ThreadId : null;
                                if (settings.CodexEnabled && request.Params?.Meta?["threadId"] is { } threadMeta)
                                {
                                    var candidate = threadMeta.GetValue<string>();
                                    if (!Guid.TryParse(candidate, out _)) throw new ArgumentException("Invalid Codex task identity.");
                                    threadId = candidate;
                                }
                                var created = store.Create(question, threadId ?? settings.Session,
                                    threadId is not null ? "Codex" : settings.Agent, threadId);
                                return Result(new { requestId = created.Id, status = created.Status,
                                    presentation = settings.Headless ? "headless" : "desktop",
                                    automaticDelivery = threadId is not null });
                            case "feedback_read":
                                var ids = Arg("requestIds").Deserialize<string[]>() ?? [];
                                if (ids.Length is < 1 or > 30) throw new ArgumentException("Provide one to thirty request IDs.");
                                var wait = Arg("waitMs").ValueKind == JsonValueKind.Undefined ? 0 : Arg("waitMs").GetInt32();
                                if (wait is < 0 or > 20000) throw new ArgumentException("waitMs must be between 0 and 20000.");
                                var deadline = DateTimeOffset.UtcNow.AddMilliseconds(wait);
                                Feedback[] records;
                                do
                                {
                                    cancellation.ThrowIfCancellationRequested();
                                    records = ids.Select(store.Read).ToArray();
                                    if (records.Any(x => x.Status != "pending") || DateTimeOffset.UtcNow >= deadline) break;
                                    await Task.Delay(Math.Min(100, Math.Max(1, (int)(deadline - DateTimeOffset.UtcNow).TotalMilliseconds)), cancellation);
                                } while (true);
                                return Result(new { requests = records.Select(Public), timedOut = wait > 0 && records.All(x => x.Status == "pending") });
                            case "feedback_cancel":
                                return Result(Public(store.Cancel(String("requestId") ?? "", String("reason"))));
                            default: throw new ArgumentException("Unknown feedback tool.");
                        }
                    }
                    catch (Exception ex) when (ex is ArgumentException or InvalidOperationException or JsonException or FormatException or System.IO.IOException or UnauthorizedAccessException)
                    { return Result(new { error = ex.Message }, true); }
                }
            }
        };
        // Preserve the protocol stream before redirecting incidental Console.Write.
        await using var transport = new StreamServerTransport(Console.OpenStandardInput(), Console.OpenStandardOutput());
        Console.SetOut(Console.Error);
        await using var server = McpServer.Create(transport, options);
        await server.RunAsync(stop);
    }

    private static object Public(Feedback feedback) => new
    {
        requestId = feedback.Id, status = feedback.Status, question = feedback.Question.QuestionText,
        answer = feedback.Answer, delivery = feedback.Delivery.State
    };
    private static CallToolResult Result(object value, bool error = false)
    {
        var json = Json.Encode(value);
        return new() { Content = [new TextContentBlock { Text = json }], StructuredContent = JsonDocument.Parse(json).RootElement.Clone(), IsError = error };
    }
}
