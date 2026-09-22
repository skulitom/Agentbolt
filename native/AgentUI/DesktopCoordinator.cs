using System;
using System.IO;
using System.Security.Cryptography;
using System.Text;
using System.Threading;
using System.Windows;
using System.Windows.Threading;

namespace AgentUI;

// Every MCP process has a dispatcher; exactly one owns this user/session/store's
// UI mutex. Ownership and release stay on that thread, including crash handoff.
internal sealed class DesktopCoordinator : IDisposable
{
    private readonly FeedbackStore store;
    private readonly Mutex mutex;
    private readonly DispatcherTimer timer;
    private readonly FileSystemWatcher watcher;
    private PanelWindow? panel;
    private bool owned;

    internal DesktopCoordinator(FeedbackStore store, Settings settings)
    {
        this.store = store;
        using var hash = SHA256.Create();
        var identity = BitConverter.ToString(hash.ComputeHash(Encoding.UTF8.GetBytes(store.Root.ToUpperInvariant()))).Replace("-", "").Substring(0, 24);
        mutex = new Mutex(false, @"Local\AgentUI.Panel." + identity);
        timer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(600) };
        timer.Tick += (_, _) => Refresh(settings);
        watcher = new FileSystemWatcher(Path.Combine(store.Root, "requests"), "*.json")
        { NotifyFilter = NotifyFilters.FileName | NotifyFilters.LastWrite };
        FileSystemEventHandler changed = (_, _) => Application.Current.Dispatcher.BeginInvoke(() => Refresh(settings));
        watcher.Created += changed;
        watcher.Changed += changed;
        watcher.Renamed += (_, _) => Application.Current.Dispatcher.BeginInvoke(() => Refresh(settings));
        watcher.EnableRaisingEvents = true;
        Refresh(settings);
        timer.Start();
    }

    private void Refresh(Settings settings)
    {
        try
        {
            if (!owned)
            {
                try { owned = mutex.WaitOne(0); } catch (AbandonedMutexException) { owned = true; }
                if (!owned) return;
                try { panel = new PanelWindow(store, settings); }
                catch { mutex.ReleaseMutex(); owned = false; throw; }
                panel.Refresh();
                if (!settings.Stdio) panel.ShowPanel(activate: true);
            }
            panel!.Refresh();
        }
        catch (Exception ex) { Console.Error.WriteLine($"Panel refresh: {ex.Message}"); }
    }

    public void Dispose()
    {
        timer.Stop(); watcher.Dispose();
        panel?.Dispose();
        if (owned) mutex.ReleaseMutex();
        mutex.Dispose();
    }
}
