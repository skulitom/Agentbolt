using System;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using System.Windows;

namespace AgentUI;

internal static class Program
{
    [STAThread]
    private static int Main(string[] args)
    {
        try
        {
            var settings = Settings.Parse(args);
            var store = new FeedbackStore(settings.DataDirectory);
            using var stop = new CancellationTokenSource();
            if (settings.Demo && !store.List().Any(x => x.Status == "pending"))
            {
                store.Create(new("How should AgentUI announce a new question?",
                    "Choose the default for this desktop prototype. New questions always remain in the inbox; this decides how much attention they ask for.",
                    "AgentUI · demo", ["Show the panel quietly", "Notify, then open on click", "Keep the panel pinned"]),
                    settings.Session, "Demo", null);
                store.Create(new("What would make this panel easier to use?", "Try writing a short note. This demo saves locally and does not send a message to an agent.", "AgentUI · demo"),
                    settings.Session, "Demo", null);
            }
            var delivery = new CodexDelivery(store, settings).Run(stop.Token);
            if (settings.Headless)
            {
                try { McpHost.Run(store, settings, stop.Token).GetAwaiter().GetResult(); }
                finally { stop.Cancel(); delivery.GetAwaiter().GetResult(); }
                return 0;
            }
            var app = new Application { ShutdownMode = ShutdownMode.OnExplicitShutdown };
            using var coordinator = new DesktopCoordinator(store, settings);
            Task? server = null;
            if (settings.Stdio)
            {
                server = Task.Run(async () =>
                {
                    try { await McpHost.Run(store, settings, stop.Token); }
                    catch (Exception ex) { Console.Error.WriteLine(ex); }
                    finally { _ = app.Dispatcher.BeginInvoke(() => app.Shutdown()); }
                });
            }
            app.Run();
            stop.Cancel();
            try { server?.GetAwaiter().GetResult(); delivery.GetAwaiter().GetResult(); }
            catch (OperationCanceledException) { }
            return 0;
        }
        catch (Exception ex) { Console.Error.WriteLine(ex); return 1; }
    }
}
