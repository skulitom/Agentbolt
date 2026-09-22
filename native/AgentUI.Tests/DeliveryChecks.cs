using System.IO;
using AgentUI;

internal static class DeliveryChecks
{
    // A real subprocess stand-in, used only by the regression suite. No real
    // agent task is messaged, and no Codex configuration is involved.
    internal static void FakeQueue(string[] args)
    {
        var root = Environment.GetEnvironmentVariable("AGENTUI_TEST_DELIVERY_DIRECTORY");
        if (root is null) { Environment.ExitCode = 82; return; }
        File.WriteAllText(Path.Combine(root, Guid.NewGuid().ToString("N") + ".json"), Json.Encode(args));
        if (File.Exists(Path.Combine(root, "reject")))
        {
            Console.Error.Write("Test host is unavailable.");
            Environment.ExitCode = 17;
        }
    }

    internal static async Task Run(string root, Action<bool, string> check)
    {
        var store = new FeedbackStore(Path.Combine(root, "delivery"));
        var captured = Path.Combine(root, "queue-calls");
        Directory.CreateDirectory(captured);
        var previous = Environment.GetEnvironmentVariable("AGENTUI_TEST_DELIVERY_DIRECTORY");
        Environment.SetEnvironmentVariable("AGENTUI_TEST_DELIVERY_DIRECTORY", captured);
        try
        {
            var settings = new Settings(store.Root, "test", "Test", null, typeof(DeliveryChecks).Assembly.Location, true, true, false, true);
            var first = store.Create(new("Reply after reconnection"), "a", "Test", Guid.NewGuid().ToString());
            var answer = new Answer(Guid.NewGuid().ToString("N"), "answer", Comment: "Keep this answer, including quotes \" and slashes \\.");
            store.Submit(first.Id, answer);
            File.WriteAllText(Path.Combine(captured, "reject"), "disconnected");
            using (var stop = new CancellationTokenSource(TimeSpan.FromSeconds(8)))
            {
                var worker = new CodexDelivery(store, settings).Run(stop.Token);
                try { await Until(() => store.Read(first.Id).Delivery.State == "retry"); }
                finally { stop.Cancel(); await worker; }
            }
            var failed = store.Read(first.Id);
            check(failed.Status == "answered" && failed.Answer?.SubmissionId == answer.SubmissionId && failed.Delivery.Error!.Contains("exit 17"), "a disconnected delivery preserves the accepted answer and reports the error");
            check(failed.Delivery.Attempts == 1 && failed.Delivery.NextAttempt > DateTimeOffset.UtcNow, "failed delivery schedules a bounded retry rather than spinning");

            File.Delete(Path.Combine(captured, "reject"));
            store.Change(first.Id, value => value with { Delivery = value.Delivery with { State = "sending", NextAttempt = DateTimeOffset.UtcNow.AddSeconds(-1) } });
            var second = store.Create(new("A different destination"), "b", "Test", Guid.NewGuid().ToString());
            store.Submit(second.Id, new(Guid.NewGuid().ToString("N"), "dismiss", Comment: "Skip this test question"));
            var large = store.Create(new("A large response"), "c", "Test", Guid.NewGuid().ToString());
            store.Submit(large.Id, new(Guid.NewGuid().ToString("N"), "answer", Comment: new string('界', 6000)));
            using (var stop = new CancellationTokenSource(TimeSpan.FromSeconds(8)))
            {
                var workers = Task.WhenAll(new CodexDelivery(store, settings).Run(stop.Token), new CodexDelivery(new FeedbackStore(store.Root), settings).Run(stop.Token));
                try { await Until(() => store.List().All(x => x.Delivery.State == "queued")); }
                finally { stop.Cancel(); await workers; }
            }
            var calls = Directory.GetFiles(captured, "*.json").Select(path => Json.Decode<string[]>(File.ReadAllText(path))).ToArray();
            var firstCalls = calls.Where(args => args[4].Contains(first.Id)).ToArray();
            check(firstCalls.Length == 2 && store.Read(first.Id).Delivery.Attempts == 2 && store.Read(first.Id).Delivery.Error is null,
                "two workers recover one expired claim without duplicating the retry");
            check(calls.Length == 4 && calls.All(args => args.Length == 5 && args[0] == "queue" && args[1] == "--thread" && args[3] == "--message") &&
                calls.All(args => store.List().Any(record => record.ThreadId == args[2] && args[4].Contains(record.Id))),
                "replies and dismissals reach their individual task destinations with stable IDs");
            var largeCall = calls.Single(args => args[4].Contains(large.Id));
            check(largeCall[4].Contains("feedback_read") && NativeProcess.Arguments(largeCall).Length < 30000 && store.Read(large.Id).Answer?.Comment?.Length == 6000,
                "large Unicode answers wake the task without exceeding process limits or losing the saved answer");
            var count = calls.Length;
            using (var stop = new CancellationTokenSource(TimeSpan.FromMilliseconds(850))) await new CodexDelivery(store, settings).Run(stop.Token);
            check(Directory.GetFiles(captured, "*.json").Length == count, "a restarted delivery worker does not resend acknowledged replies");
        }
        finally { Environment.SetEnvironmentVariable("AGENTUI_TEST_DELIVERY_DIRECTORY", previous); }
    }

    private static async Task Until(Func<bool> predicate)
    {
        var deadline = DateTime.UtcNow.AddSeconds(6);
        while (!predicate())
        {
            if (DateTime.UtcNow >= deadline) throw new TimeoutException("Delivery did not reach its expected state.");
            await Task.Delay(30);
        }
    }
}
