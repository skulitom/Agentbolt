using System;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;

namespace AgentUI;

internal static class Setup
{
    internal const string ServerName = "agentui-native";

    // Keep releases in separate directories: an update must not replace DLLs
    // underneath a running MCP process. Only our named registration is changed.
    internal static async Task<string> Connect(Settings settings, string? installationRoot = null)
    {
        var codex = settings.CodexExecutable ?? throw new InvalidOperationException(
            "Codex CLI was not found. Install Codex CLI, then reopen AgentUI to connect.");
        var capability = await Run(codex, "queue", "--help");
        if (capability.Code != 0) throw new InvalidOperationException("This Codex CLI does not support automatic replies. Update it, then try again.");
        var existing = await Run(codex, "mcp", "get", ServerName, "--json");
        if (existing.Code == 0)
        {
            using var document = JsonDocument.Parse(existing.Output);
            var transport = document.RootElement.GetProperty("transport");
            if (!transport.TryGetProperty("command", out var command) ||
                !string.Equals(Path.GetFileName(command.GetString()), "AgentUI.exe", StringComparison.OrdinalIgnoreCase))
                throw new InvalidOperationException("The name agentui-native is already used by another connection. Rename that connection before continuing.");
        }
        var source = AppDomain.CurrentDomain.BaseDirectory;
        var files = Directory.GetFiles(source).Where(path =>
            path.EndsWith(".dll", StringComparison.OrdinalIgnoreCase) ||
            Path.GetFileName(path) is "AgentUI.exe" or "AgentUI.exe.config" or "THIRD-PARTY-NOTICES.txt" or "LICENSE" or "README.md" or "verification.md").OrderBy(x => x).ToArray();
        using var sha = SHA256.Create();
        var manifest = string.Join("\n", files.Select(path => Path.GetFileName(path) + ":" + Convert.ToBase64String(sha.ComputeHash(File.ReadAllBytes(path)))));
        var hash = BitConverter.ToString(sha.ComputeHash(Encoding.UTF8.GetBytes(manifest))).Replace("-", "").Substring(0, 12).ToLowerInvariant();
        var directory = Path.Combine(installationRoot ?? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Programs", "AgentUI"), "0.2.0-" + hash);
        Directory.CreateDirectory(directory);
        foreach (var file in files)
        {
            var destination = Path.Combine(directory, Path.GetFileName(file));
            if (!File.Exists(destination)) File.Copy(file, destination);
            else if (!File.ReadAllBytes(destination).SequenceEqual(File.ReadAllBytes(file)))
                throw new IOException("An installed AgentUI file has changed. Reinstall from a fresh package.");
        }
        var installed = Path.Combine(directory, "AgentUI.exe");
        var codexHome = Environment.GetEnvironmentVariable("CODEX_HOME") ??
            Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), ".codex");
        var config = Path.Combine(codexHome, "config.toml");
        if (File.Exists(config))
        {
            var backup = Path.Combine(settings.DataDirectory, "setup-backups");
            Directory.CreateDirectory(backup);
            File.Copy(config, Path.Combine(backup, "config-" + DateTime.UtcNow.ToString("yyyyMMdd-HHmmss") + "-" + Guid.NewGuid().ToString("N") + ".toml"));
        }
        var registered = await Run(codex, "mcp", "add", ServerName, "--", installed, "--stdio", "--codex");
        if (registered.Code != 0) throw new IOException("Codex could not save the connection: " + registered.Error);
        return "Connected. Restart Codex to load AgentUI. Questions will appear here automatically.";
    }

    private static async Task<(int Code, string Output, string Error)> Run(string executable, params string[] arguments)
    {
        using var process = Process.Start(new ProcessStartInfo(executable)
        {
            Arguments = NativeProcess.Arguments(arguments), UseShellExecute = false, CreateNoWindow = true,
            RedirectStandardOutput = true, RedirectStandardError = true, RedirectStandardInput = true
        }) ?? throw new IOException("Could not start Codex.");
        process.StandardInput.Close();
        var output = process.StandardOutput.ReadToEndAsync();
        var error = process.StandardError.ReadToEndAsync();
        using var deadline = new CancellationTokenSource(TimeSpan.FromSeconds(20));
        try { await NativeProcess.Wait(process, deadline.Token); }
        catch { if (!process.HasExited) process.Kill(); throw; }
        return (process.ExitCode, await output, await error);
    }
}
