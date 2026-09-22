using System.Text.Json;
using System.IO;
using System.Diagnostics;
using AgentUI;

if (args.FirstOrDefault() == "queue")
{
    DeliveryChecks.FakeQueue(args);
    return;
}
if (args.FirstOrDefault() == "--echo-args")
{
    Console.Write(Json.Encode(args.Skip(1).ToArray()));
    return;
}
if (args.FirstOrDefault() == "--live-delivery")
{
    // Explicit, opt-in integration check to the task that launched this test.
    // Never use a fabricated task ID or present this as a human decision.
    var settings = Settings.Parse(["--codex", "--data-dir", args[1]]);
    if (settings.ThreadId is null || !settings.CodexEnabled) throw new Exception("No originating Codex task in this environment.");
    var liveStore = new FeedbackStore(settings.DataDirectory);
    var probe = liveStore.Create(new("AUTOMATED AGENTUI DELIVERY TEST — no human decision", Context: "This synthetic test verifies feedback delivery to the originating development task only."),
        settings.Session, "AgentUI test", settings.ThreadId);
    liveStore.Submit(probe.Id, new(Guid.NewGuid().ToString("N"), "answer", Comment: "AUTOMATED TEST: AgentUI's native delivery path reached this task. This is not user feedback. Record receipt and continue the existing implementation work."));
    using var deadline = new CancellationTokenSource(TimeSpan.FromSeconds(38));
    var worker = new CodexDelivery(liveStore, settings).Run(deadline.Token);
    while (!deadline.IsCancellationRequested && liveStore.Read(probe.Id).Delivery.State is "pending" or "sending") await Task.Delay(150);
    deadline.Cancel(); await worker;
    Console.WriteLine(Json.Encode(liveStore.Read(probe.Id).Delivery));
    return;
}

var root = Path.Combine(Path.GetTempPath(), "agentui-tests-" + Guid.NewGuid().ToString("N"));
Directory.CreateDirectory(root);
var passed = 0;
void Check(bool condition, string label)
{
    if (!condition) throw new Exception(label);
    passed++;
    Console.WriteLine("PASS " + label);
}
void Reject(Action operation, string label)
{
    try { operation(); } catch (ArgumentException) { Check(true, label); return; }
    throw new Exception("Should reject: " + label);
}
Answer Answer(string? choice = null, Dictionary<string, JsonElement>? values = null, string? comment = null)
    => new(Guid.NewGuid().ToString("N"), "answer", choice, values, comment);

try
{
    var samples = new[] { "", "plain", "two words", "a\"b", "C:\\folder with spaces\\", "slash\\\"quote", "line1\nline2", "$() & | %PATH% ; `", "日本語 · café" };
    using (var echo = Process.Start(new ProcessStartInfo(typeof(Program).Assembly.Location)
    {
        UseShellExecute = false, CreateNoWindow = true, RedirectStandardOutput = true,
        Arguments = NativeProcess.Arguments(new[] { "--echo-args" }.Concat(samples).ToArray())
    })!)
    {
        var result = await echo.StandardOutput.ReadToEndAsync();
        await NativeProcess.Wait(echo, CancellationToken.None);
        Check(echo.ExitCode == 0 && Json.Decode<string[]>(result).SequenceEqual(samples), "native process arguments preserve quotes, Unicode, newlines and shell characters");
    }
    var store = new FeedbackStore(root);
    var second = new FeedbackStore(root);
    var a = store.Create(new("Choose a path", Options: ["First", "Second"]), "session", "Test", null);
    Check(second.Read(a.Id).Status == "pending", "other process view sees a pending request");
    Reject(() => store.Submit(a.Id, Answer("Not offered")), "unoffered choice rejected");
    Check(store.Read(a.Id).Status == "pending", "invalid input cannot finish a request");
    var answer = Answer("Second", comment: "Use this one");
    store.Submit(a.Id, answer);
    Check(second.Read(a.Id).Answer?.Choice == "Second", "accepted answer survives a new store instance");
    Check(store.Submit(a.Id, answer).Answer == store.Read(a.Id).Answer || store.Read(a.Id).Answer?.SubmissionId == answer.SubmissionId, "identical retry is idempotent");
    Reject(() => store.Submit(a.Id, answer with { Choice = "First" }), "same submission ID cannot change an answer");
    Reject(() => store.Submit(a.Id, Answer("First")), "conflicting terminal answer rejected");
    Check(store.Cancel(a.Id, "Late cancellation").Status == "answered", "cancellation cannot overwrite an answer");

    var b = store.Create(new("Open response"), "session", "Test", null);
    Reject(() => store.Submit(b.Id, Answer(comment: " ")), "blank open response is not consent");
    var dismissed = store.Submit(b.Id, new(Guid.NewGuid().ToString("N"), "dismiss", Comment: "Wrong question"));
    Check(dismissed.Status == "dismissed" && dismissed.Answer?.Comment == "Wrong question", "dismissal preserves the human's reason");

    var form = store.Create(new("Configuration", Fields:
    [new("count", "Count", "number", true, Min: 1, Max: 5), new("enabled", "Enabled", "boolean")]), "session", "Test", null);
    Reject(() => store.Submit(form.Id, Answer()), "required unanswered fields rejected");
    Reject(() => store.Submit(form.Id, Answer(values: new() { ["count"] = JsonSerializer.SerializeToElement(6) })), "out of range answer rejected");
    var accepted = store.Submit(form.Id, Answer(values: new() { ["count"] = JsonSerializer.SerializeToElement(3) }));
    Check(!accepted.Answer!.Values!.ContainsKey("enabled"), "untouched optional boolean remains unanswered");
    Reject(() => store.Read("../outside"), "path traversal rejected");
    Reject(() => store.Create(new("", Options: ["A", "B"]), "s", "t", null), "empty questions rejected");
    Reject(() => store.Create(new("Question", Options: ["A", "A"]), "s", "t", null), "duplicate options rejected");
    Reject(() => store.Create(new("Question", Fields: [new("x", "X", "text"), new("x", "X", "text")]), "s", "t", null), "duplicate fields rejected");

    var draft = new Draft(new() { ["x"] = "still typing" }, "draft note");
    store.SaveDraft(a.Id, draft);
    Check(second.ReadDraft(a.Id)?.Comment == "draft note", "drafts restore independently of terminal results");
    Check(second.Read(a.Id).Answer?.Comment == "Use this one", "draft edits never change accepted answers");

    for (var i = 0; i < 12; i++)
    {
        var race = store.Create(new("Race", Options: ["Yes", "No"]), "s", "t", null);
        await Task.WhenAll(Task.Run(() => store.Cancel(race.Id, "obsolete")), Task.Run(() =>
        {
            try { second.Submit(race.Id, Answer("Yes")); } catch (ArgumentException) { }
        }));
        var final = store.Read(race.Id);
        Check(final.Status is "answered" or "cancelled" && final.Answer is not null, "answer/cancel race produces one complete result " + i);
    }
    Check(!Directory.EnumerateFiles(Path.Combine(root, "requests"), "*.tmp").Any(), "atomic writes leave no temporary records");
    var changing = store.Create(new("Concurrent snapshots", Context: new string('x', 8000)), "s", "t", null);
    using (var gate = new Barrier(2))
    {
        await Task.WhenAll(Task.Run(() =>
        {
            gate.SignalAndWait();
            for (var i = 0; i < 120; i++) store.Change(changing.Id, value => value with { Delivery = value.Delivery with { Attempts = value.Delivery.Attempts + 1 } });
        }), Task.Run(() =>
        {
            gate.SignalAndWait();
            for (var i = 0; i < 300; i++)
            {
                var snapshot = second.Read(changing.Id);
                if (snapshot.Question.Context?.Length != 8000) throw new Exception("Incomplete snapshot during replacement.");
            }
        }));
    }
    Check(store.Read(changing.Id).Delivery.Attempts == 120, "concurrent readers retain complete records while writers replace them");
    UiChecks.Run(root, Check);
    await DeliveryChecks.Run(root, Check);
    var codexSettings = Settings.Parse(["--no-delivery", "--data-dir", Path.Combine(root, "setup")]);
    if (codexSettings.CodexExecutable is not null)
    {
        var originalCodexHome = Environment.GetEnvironmentVariable("CODEX_HOME");
        var isolatedHome = Path.Combine(root, "codex-home");
        Directory.CreateDirectory(isolatedHome);
        var configPath = Path.Combine(isolatedHome, "config.toml");
        File.WriteAllText(configPath, "[mcp_servers.keep_me]\ncommand = 'existing-tool'\n");
        Environment.SetEnvironmentVariable("CODEX_HOME", isolatedHome);
        try
        {
            await Setup.Connect(codexSettings, Path.Combine(root, "installed"));
            var config = File.ReadAllText(configPath);
            Check(config.Contains("mcp_servers.keep_me") && config.Contains("mcp_servers.agentui-native") && config.Contains("--codex"), "guided setup adds only its connection and preserves existing Codex configuration");
            Check(Directory.GetFiles(Path.Combine(codexSettings.DataDirectory, "setup-backups")).Length == 1, "setup backs up the pre-existing configuration");
            await Setup.Connect(codexSettings, Path.Combine(root, "installed"));
            Check(Directory.GetDirectories(Path.Combine(root, "installed")).Length == 1, "repeated setup reuses the same verified package");
        }
        finally { Environment.SetEnvironmentVariable("CODEX_HOME", originalCodexHome); }
    }
    Console.WriteLine($"{passed} checks passed.");
}
finally { Directory.Delete(root, recursive: true); }
