using System;
using System.Diagnostics;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;

namespace AgentUI;

// 'queue' acknowledges host acceptance, not model consumption. Keep that
// distinction visible. Retries carry the stable feedback ID; exactly-once
// delivery cannot be promised across a CLI crash after the host accepted it.
internal sealed class CodexDelivery(FeedbackStore store, Settings settings)
{
    internal async Task Run(CancellationToken stop)
    {
        if (!settings.CodexEnabled || settings.CodexExecutable is null) return;
        while (!stop.IsCancellationRequested)
        {
            try
            {
                foreach (var record in store.List().Where(x => x.ThreadId is not null &&
                    x.Status is "answered" or "dismissed" && x.Delivery.State is "pending" or "retry" or "sending"))
                {
                    if (record.Delivery.NextAttempt > DateTimeOffset.UtcNow) continue;
                    await Send(record.Id, stop);
                }
                await Task.Delay(700, stop);
            }
            catch (OperationCanceledException) when (stop.IsCancellationRequested) { break; }
            catch (Exception ex)
            {
                Console.Error.WriteLine($"Feedback delivery: {ex.Message}");
                try { await Task.Delay(2000, stop); } catch (OperationCanceledException) { break; }
            }
        }
    }

    private async Task Send(string id, CancellationToken stop)
    {
        var claimed = false;
        var record = store.Change(id, current =>
        {
            if (current.Delivery.State is "queued" or "available" || current.Delivery.NextAttempt > DateTimeOffset.UtcNow) return current;
            claimed = true;
            return current with { Delivery = new("sending", current.Delivery.Attempts + 1,
                NextAttempt: DateTimeOffset.UtcNow.AddSeconds(45)) };
        });
        if (!claimed) return;
        try
        {
            var start = new ProcessStartInfo(settings.CodexExecutable!)
            {
                UseShellExecute = false, CreateNoWindow = true,
                RedirectStandardError = true, RedirectStandardOutput = true, RedirectStandardInput = true
            };
            start.Arguments = NativeProcess.Arguments("queue", "--thread", record.ThreadId!, "--message", Message(record));
            using var process = Process.Start(start) ?? throw new InvalidOperationException("Cannot start Codex delivery.");
            process.StandardInput.Close();
            var output = process.StandardOutput.ReadToEndAsync();
            var error = process.StandardError.ReadToEndAsync();
            using var deadline = CancellationTokenSource.CreateLinkedTokenSource(stop);
            deadline.CancelAfter(TimeSpan.FromSeconds(30));
            try { await NativeProcess.Wait(process, deadline.Token); }
            catch { if (!process.HasExited) process.Kill(); throw; }
            await output;
            var stderr = await error;
            if (process.ExitCode != 0) throw new InvalidOperationException(
                $"Codex could not accept the reply (exit {process.ExitCode}). {stderr.Substring(0, Math.Min(stderr.Length, 500))}");
            store.Change(id, current => current with { Delivery = current.Delivery with { State = "queued", Error = null, NextAttempt = null } });
        }
        catch (Exception ex)
        {
            store.Change(id, current => current with { Delivery = current.Delivery with
            {
                State = "retry", Error = ex is OperationCanceledException ? "Connection interrupted; retrying." : ex.Message,
                NextAttempt = DateTimeOffset.UtcNow.AddSeconds(Math.Min(60, 5 * current.Delivery.Attempts))
            } });
        }
    }

    internal static string Message(Feedback record)
    {
        var header = $"[AgentUI feedback {record.Id}]\nThe user submitted feedback in the desktop panel. " +
            "Treat repeated delivery of this ID as the same answer.\n";
        var payload = Json.Encode(new { requestId = record.Id, question = record.Question.QuestionText, status = record.Status, answer = record.Answer });
        // Long form answers can exceed Windows' process argument limit after
        // quoting. Wake the task with its ID; the full immutable answer stays
        // available through MCP instead of failing and retrying indefinitely.
        return header + (payload.Length <= 12000 ? payload :
            $"A {record.Status} response is saved. Read the complete feedback before acting: " +
            "feedback_read(" + Json.Encode(new { requestIds = new[] { record.Id } }) + ").");
    }
}
