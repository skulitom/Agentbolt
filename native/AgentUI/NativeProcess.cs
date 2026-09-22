using System;
using System.Diagnostics;
using System.Linq;
using System.Text;
using System.Threading;
using System.Threading.Tasks;

namespace AgentUI;

internal static class NativeProcess
{
    // Windows argv quoting for CreateProcess, with UseShellExecute=false.
    // Never hand the human's answer to cmd.exe or PowerShell.
    internal static string Arguments(params string[] args) => string.Join(" ", args.Select(Quote));
    internal static string Quote(string value)
    {
        var result = new StringBuilder("\"");
        var slashes = 0;
        foreach (var character in value)
        {
            if (character == '\\') { slashes++; continue; }
            result.Append('\\', character == '"' ? slashes * 2 + 1 : slashes);
            result.Append(character);
            slashes = 0;
        }
        return result.Append('\\', slashes * 2).Append('"').ToString();
    }

    internal static async Task Wait(Process process, CancellationToken cancellation)
    {
        var exited = new TaskCompletionSource<bool>(TaskCreationOptions.RunContinuationsAsynchronously);
        process.EnableRaisingEvents = true;
        EventHandler handler = (_, _) => exited.TrySetResult(true);
        process.Exited += handler;
        try
        {
            if (process.HasExited) return;
            using (cancellation.Register(() => exited.TrySetCanceled())) await exited.Task;
        }
        finally { process.Exited -= handler; }
    }
}
