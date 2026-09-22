using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text;
using System.Threading;

namespace AgentUI;

// Each record is a complete snapshot. One per-request OS file lock serializes
// cancellation, answers, and delivery receipts across MCP processes. A flushed
// temp file is renamed before success is returned; readers never see half JSON.
internal sealed class FeedbackStore
{
    internal string Root { get; }
    private string Requests => Path.Combine(Root, "requests");
    internal FeedbackStore(string root)
    {
        Root = Path.GetFullPath(root);
        Directory.CreateDirectory(Requests);
        Directory.CreateDirectory(Path.Combine(Root, "locks"));
        Directory.CreateDirectory(Path.Combine(Root, "drafts"));
    }

    internal Feedback Create(Question question, string session, string agent, string? threadId)
    {
        Validate.Question(question);
        if (List().Count(x => x.Status == "pending") >= 100)
            throw new ArgumentException("There are already 100 pending questions. Resolve or cancel some first.");
        var feedback = new Feedback
        {
            Id = Guid.NewGuid().ToString("N"), Question = question, Session = session,
            Agent = agent, ThreadId = threadId
        };
        Atomic(PathFor(feedback.Id), Json.Encode(feedback));
        return feedback;
    }

    private string PathFor(string id)
    {
        if (!Guid.TryParseExact(id, "N", out _)) throw new ArgumentException("Invalid feedback ID.");
        return Path.Combine(Requests, id + ".json");
    }

    internal Feedback Read(string id)
    {
        var path = PathFor(id);
        var record = Json.Decode<Feedback>(ReadSnapshot(path));
        if (record.SchemaVersion != 1 || record.Id != id) throw new IOException("Unsupported or corrupt feedback record.");
        return record;
    }

    private static string ReadSnapshot(string path)
    {
        // Do not preflight with File.Exists: Windows may report false during a
        // replacement. Open with delete sharing, and tolerate the brief sharing
        // or pathname transition without exposing a missing/partial record.
        for (var attempt = 0; ; attempt++)
        {
            try
            {
                using var stream = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete);
                using var reader = new StreamReader(stream, Encoding.UTF8);
                return reader.ReadToEnd();
            }
            catch (IOException) when (attempt < 6) { Thread.Sleep(5 * (attempt + 1)); }
            catch (FileNotFoundException) { throw new ArgumentException("Feedback request was not found."); }
        }
    }

    internal List<Feedback> List()
    {
        var result = new List<Feedback>();
        foreach (var file in Directory.EnumerateFiles(Requests, "*.json"))
        {
            try { result.Add(Read(Path.GetFileNameWithoutExtension(file))); }
            catch (Exception ex) when (ex is IOException or System.Text.Json.JsonException or ArgumentException)
            { Console.Error.WriteLine($"Cannot read feedback {Path.GetFileName(file)}: {ex.Message}"); }
        }
        return result.OrderBy(x => x.CreatedAt).ToList();
    }

    internal Feedback Submit(string id, Answer answer) => Change(id, current =>
    {
        if (current.Status != "pending")
        {
            if (current.Answer?.SubmissionId == answer.SubmissionId && Json.Encode(current.Answer) == Json.Encode(answer)) return current;
            throw new ArgumentException("This request has already ended. Your draft has been kept.");
        }
        Validate.Answer(current.Question, answer);
        return current with
        {
            Status = answer.Action == "dismiss" ? "dismissed" : "answered",
            Answer = answer, AnsweredAt = DateTimeOffset.UtcNow,
            Delivery = new(current.ThreadId is null ? "available" : "pending")
        };
    });

    internal Feedback Cancel(string id, string? reason) => Change(id, current =>
    {
        Validate.Text(reason, "Reason", 6000);
        return current.Status != "pending" ? current : current with
        {
            Status = "cancelled", AnsweredAt = DateTimeOffset.UtcNow,
            Answer = new(Guid.NewGuid().ToString("N"), "cancel", Comment: reason)
        };
    });

    internal Feedback Change(string id, Func<Feedback, Feedback> change)
    {
        _ = PathFor(id);
        using var claim = Lock(Path.Combine(Root, "locks", id + ".lock"));
        var current = Read(id);
        var next = change(current);
        if (!ReferenceEquals(current, next)) Atomic(PathFor(id), Json.Encode(next));
        return next;
    }

    private static FileStream Lock(string path)
    {
        for (var attempt = 0; ; attempt++)
        {
            try { return new FileStream(path, FileMode.OpenOrCreate, FileAccess.ReadWrite, FileShare.None); }
            catch (IOException) when (attempt < 60) { Thread.Sleep(25); }
        }
    }

    internal Draft? ReadDraft(string id)
    {
        _ = PathFor(id);
        var path = Path.Combine(Root, "drafts", id + ".json");
        try { return File.Exists(path) ? Json.Decode<Draft>(File.ReadAllText(path)) : null; }
        catch (Exception ex) when (ex is IOException or System.Text.Json.JsonException)
        { Console.Error.WriteLine($"Cannot restore draft: {ex.Message}"); return null; }
    }

    internal void SaveDraft(string id, Draft draft)
    {
        _ = PathFor(id);
        Atomic(Path.Combine(Root, "drafts", id + ".json"), Json.Encode(draft));
    }

    internal static void Atomic(string destination, string contents)
    {
        var temporary = destination + "." + Guid.NewGuid().ToString("N") + ".tmp";
        try
        {
            using (var stream = new FileStream(temporary, FileMode.CreateNew, FileAccess.Write, FileShare.None))
            {
                var bytes = Encoding.UTF8.GetBytes(contents);
                stream.Write(bytes, 0, bytes.Length);
                stream.Flush(flushToDisk: true);
            }
            for (var attempt = 0; ; attempt++)
            {
                try
                {
                    if (File.Exists(destination)) File.Replace(temporary, destination, null);
                    else File.Move(temporary, destination);
                    break;
                }
                catch (IOException) when (attempt < 4) { Thread.Sleep(30 * (attempt + 1)); }
            }
        }
        finally { if (File.Exists(temporary)) File.Delete(temporary); }
    }
}
