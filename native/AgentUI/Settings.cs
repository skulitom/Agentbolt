using System;
using System.IO;
using System.Linq;

namespace AgentUI;

internal sealed record Settings(string DataDirectory, string Session, string Agent, string? ThreadId,
    string? CodexExecutable, bool Headless, bool Stdio, bool Demo, bool CodexEnabled = false)
{
    internal static Settings Parse(string[] args)
    {
        string? Value(string flag)
        {
            var index = Array.IndexOf(args, flag);
            if (index < 0) return null;
            if (index + 1 >= args.Length || args[index + 1].StartsWith("--")) throw new ArgumentException($"Missing value for {flag}.");
            return args[index + 1];
        }
        var root = Value("--data-dir") ?? Environment.GetEnvironmentVariable("AGENTUI_DATA_DIR") ??
            Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "AgentUI", "v2");
        var disabled = args.Contains("--no-delivery") || args.Contains("--demo");
        var thread = disabled ? null :
            Environment.GetEnvironmentVariable("CODEX_THREAD_ID");
        if (string.IsNullOrWhiteSpace(thread)) thread = null;
        if (thread is not null && !Guid.TryParse(thread, out _)) throw new ArgumentException("CODEX_THREAD_ID must be a task UUID.");
        var executable = FindCodex();
        if (executable is null) thread = null;
        return new(root, Value("--session") ?? thread ?? Guid.NewGuid().ToString("N"),
            Value("--agent") ?? (thread is null ? "Connected agent" : "Codex"), thread, executable,
            args.Contains("--headless"), args.Contains("--stdio"), args.Contains("--demo"),
            !disabled && executable is not null && (args.Contains("--codex") || thread is not null));
    }

    private static string? FindCodex()
    {
        var configured = Environment.GetEnvironmentVariable("AGENTUI_CODEX_EXE");
        if (configured is not null)
        {
            if (!Path.IsPathRooted(configured) || Path.GetFullPath(configured) != configured || !File.Exists(configured) || !configured.EndsWith(".exe", StringComparison.OrdinalIgnoreCase))
                throw new ArgumentException("AGENTUI_CODEX_EXE must be an absolute path to codex.exe.");
            return configured;
        }
        foreach (var entry in (Environment.GetEnvironmentVariable("PATH") ?? "").Split(Path.PathSeparator))
        {
            var candidate = Path.Combine(entry.Trim('"'), "codex.exe");
            if (File.Exists(candidate)) return candidate;
        }
        var npm = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
            "npm", "node_modules", "@openai", "codex", "node_modules", "@openai", "codex-win32-x64",
            "vendor", "x86_64-pc-windows-msvc", "bin", "codex.exe");
        return File.Exists(npm) ? npm : null;
    }
}
