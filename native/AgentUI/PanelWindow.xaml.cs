using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Globalization;
using System.Linq;
using System.IO;
using System.Runtime.InteropServices;
using System.Text.Json;
using System.Windows;
using System.Windows.Automation;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Interop;
using System.Windows.Threading;
using Forms = System.Windows.Forms;

namespace AgentUI;

public partial class PanelWindow : Window, IDisposable
{
    private readonly FeedbackStore store;
    private readonly Settings settings;
    private readonly Forms.NotifyIcon tray;
    private readonly DispatcherTimer draftTimer;
    private readonly Dictionary<string, Func<string>> readers = new();
    private readonly HashSet<string> seen = new();
    private readonly HashSet<string> later = new();
    private TextBox? comment;
    private TextBlock? error;
    private Feedback? current;
    private string? choice;
    private string signature = "";
    private bool refreshing;
    private bool closing;
    private bool muted;
    private bool hiddenByUser;
    private string? restoredSelection;

    [DllImport("user32.dll")]
    private static extern bool SetWindowPos(IntPtr hwnd, IntPtr after, int x, int y, int width, int height, uint flags);

    internal PanelWindow(FeedbackStore store, Settings settings, bool showTray = true)
    {
        this.store = store;
        this.settings = settings;
        InitializeComponent();
        try
        {
            var statePath = Path.Combine(store.Root, "panel.json");
            if (File.Exists(statePath))
            {
                var state = Json.Decode<PanelState>(File.ReadAllText(statePath));
                muted = state.Muted; hiddenByUser = state.Hidden; Topmost = state.Pinned;
                restoredSelection = state.SelectedId;
                foreach (var id in state.Later ?? []) later.Add(id);
                PinButton.Content = Topmost ? "Unpin panel" : "Pin panel";
            }
        }
        catch (Exception ex) { Console.Error.WriteLine($"Cannot restore panel position/state: {ex.Message}"); }
        Left = Math.Max(20, SystemParameters.WorkArea.Right - Width - 28);
        Top = Math.Max(20, SystemParameters.WorkArea.Bottom - Height - 28);
        draftTimer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(350) };
        draftTimer.Tick += (_, _) => { draftTimer.Stop(); SaveDraft(); };
        tray = new Forms.NotifyIcon { Icon = System.Drawing.SystemIcons.Application, Text = "AgentUI · Feedback", Visible = showTray };
        tray.DoubleClick += (_, _) => Dispatcher.Invoke(() => ShowPanel(true));
        tray.BalloonTipClicked += (_, _) => Dispatcher.Invoke(() => ShowPanel(true));
        tray.ContextMenuStrip = new Forms.ContextMenuStrip();
        tray.ContextMenuStrip.Items.Add("Show feedback", null, (_, _) => Dispatcher.Invoke(() => ShowPanel(true)));
        var pause = new Forms.ToolStripMenuItem("Pause interruptions") { CheckOnClick = true, Checked = muted };
        pause.CheckedChanged += (_, _) => { muted = pause.Checked; SavePanelState(); };
        tray.ContextMenuStrip.Items.Add(pause);
        tray.ContextMenuStrip.Items.Add("Hide panel", null, (_, _) => Dispatcher.Invoke(HidePanel));
        if (!settings.Stdio) tray.ContextMenuStrip.Items.Add("Quit AgentUI", null, (_, _) => Dispatcher.Invoke(() => Application.Current.Shutdown()));
        Closing += OnClosing;
        RenderEmpty();
    }

    internal void ShowPanel(bool activate)
    {
        ShowActivated = activate;
        if (WindowState == WindowState.Minimized) WindowState = WindowState.Normal;
        Show();
        if (activate) Activate();
        else SetWindowPos(new WindowInteropHelper(this).Handle, IntPtr.Zero, 0, 0, 0, 0, 0x0010 | 0x0001 | 0x0002 | 0x0040);
        hiddenByUser = false;
        SavePanelState();
    }

    internal void Refresh()
    {
        var records = store.List();
        var pending = records.Where(x => x.Status == "pending").ToList();
        var fresh = pending.Where(x => seen.Add(x.Id)).ToList();
        var nextSignature = string.Join("|", records.Select(x => $"{x.Id}:{x.Status}:{x.Delivery.State}"));
        var count = pending.Count;
        QueueHeading.Text = count == 0 ? "ALL CAUGHT UP" : $"WAITING FOR YOU  ·  {count}";
        tray.Text = count == 0 ? "AgentUI · All caught up" : $"AgentUI · {count} waiting";
        if (nextSignature != signature)
        {
            signature = nextSignature;
            refreshing = true;
            Inbox.Items.Clear();
            var ordered = pending.Concat(records.Where(x => x.Status != "pending").Reverse().Take(12));
            foreach (var record in ordered)
            {
                var item = new ListBoxItem { Tag = record.Id, Content = Sidebar(record) };
                AutomationProperties.SetName(item, record.Question.QuestionText);
                Inbox.Items.Add(item);
                if (record.Id == current?.Id) item.IsSelected = true;
            }
            refreshing = false;
            var updated = records.FirstOrDefault(x => x.Id == current?.Id);
            if (updated is not null && (updated.Status != current!.Status || updated.Delivery.State != current.Delivery.State))
                Render(updated);
            if (current is null)
            {
                var first = records.FirstOrDefault(x => x.Id == restoredSelection) ?? pending.FirstOrDefault(x => !later.Contains(x.Id));
                restoredSelection = null;
                if (first is not null) Select(first);
            }
        }
        if (fresh.Count > 0 && !muted)
        {
            if (!IsVisible && seen.Count == fresh.Count && !hiddenByUser) ShowPanel(false);
            else if (!IsVisible || WindowState == WindowState.Minimized)
                tray.ShowBalloonTip(3500, "A quick decision", fresh.Count == 1 ? fresh[0].Question.QuestionText : $"{fresh.Count} questions are waiting in AgentUI.", Forms.ToolTipIcon.None);
        }
    }

    private static StackPanel Sidebar(Feedback feedback)
    {
        var panel = new StackPanel();
        panel.Children.Add(Text(feedback.Question.Project ?? feedback.Agent, 11, "#64705E"));
        panel.Children.Add(Text(feedback.Question.QuestionText, 13, "#25342E", true, new Thickness(0,5,0,7)));
        panel.Children.Add(Text(feedback.Status == "pending" ? "Needs your input" : Status(feedback), 11, "#526447"));
        return panel;
    }

    private static string Status(Feedback feedback) => feedback.Status switch
    {
        "cancelled" => "No longer needed",
        _ when feedback.Delivery.State == "queued" => "Queued in Codex",
        _ when feedback.Delivery.State is "pending" or "sending" => "Saved · sending to agent",
        _ when feedback.Delivery.State == "retry" => "Saved · retrying delivery",
        "dismissed" => "Dismissed · saved",
        "answered" => "Saved · ready for agent",
        _ => "Waiting for you"
    };

    private void SelectRequest(object sender, SelectionChangedEventArgs e)
    {
        if (!refreshing && Inbox.SelectedItem is ListBoxItem item && item.Tag is string id) Select(store.Read(id));
    }

    private void Select(Feedback record)
    {
        SaveDraft();
        Render(record);
        SavePanelState();
        refreshing = true;
        foreach (ListBoxItem item in Inbox.Items) item.IsSelected = (string)item.Tag == record.Id;
        refreshing = false;
    }

    private void Render(Feedback record)
    {
        draftTimer.Stop();
        current = record; choice = null; readers.Clear(); comment = null; error = null;
        Body.Children.Clear();
        Body.Children.Add(Text($"{record.Agent.ToUpperInvariant()}  /  {record.Question.Project ?? "FEEDBACK"}", 11, "#63715B", true));
        Body.Children.Add(Text(record.Question.QuestionText, 25, "#25342E", true, new Thickness(0,13,0,14)));
        if (!string.IsNullOrWhiteSpace(record.Question.Context))
            Body.Children.Add(Text(record.Question.Context!, 14, "#596353", false, new Thickness(0,0,0,22)));
        if (record.Status != "pending")
        {
            Body.Children.Add(Text(Status(record), 16, "#486741", true, new Thickness(0,8,0,15)));
            if (record.Answer?.Choice is { } selected) Body.Children.Add(Text(selected, 18, "#25342E", true));
            foreach (var field in record.Answer?.Values ?? [])
                Body.Children.Add(Text($"{record.Question.Fields?.FirstOrDefault(f => f.Id == field.Key)?.Label ?? field.Key}: {field.Value}", 14, "#25342E", false, new Thickness(0,8,0,0)));
            if (record.Answer?.Comment is { Length: > 0 } note) Body.Children.Add(Text(note, 14, "#596353", false, new Thickness(0,12,0,0)));
            if (record.Delivery.Error is { } deliveryError) Body.Children.Add(Text(deliveryError, 12, "#95442B", false, new Thickness(0,14,0,0)));
            Body.Children.Add(Action("Next question →", "next", () => Next(), false));
            Footer.Text = "Your answer is saved on this device.";
            return;
        }
        var draft = store.ReadDraft(record.Id) ?? new(new());
        choice = draft.Choice;
        var simple = (record.Question.Fields?.Length ?? 0) == 0;
        if (record.Question.Options is not null && !simple)
        {
            var options = new ComboBox { ItemsSource = record.Question.Options, SelectedItem = choice };
            AutomationProperties.SetName(options, "Choose an answer");
            AutomationProperties.SetAutomationId(options, "answer-choice");
            options.SelectionChanged += (_, _) => { choice = options.SelectedItem as string; Changed(); };
            Body.Children.Add(options);
        }
        foreach (var field in record.Question.Fields ?? [])
        {
            Body.Children.Add(Text(field.Label + (field.Required ? " *" : " · optional"), 13, "#25342E", true));
            draft.Values.TryGetValue(field.Id, out var prior);
            if (field.Type is "choice" or "boolean")
            {
                var options = field.Type == "boolean" ? new[] { "Yes", "No" } : field.Options!;
                var input = new ComboBox { ItemsSource = options, SelectedItem = prior };
                AutomationProperties.SetName(input, field.Label);
                AutomationProperties.SetAutomationId(input, "field-" + field.Id);
                input.SelectionChanged += (_, _) => Changed();
                readers[field.Id] = () => input.SelectedItem as string ?? "";
                Body.Children.Add(input);
            }
            else
            {
                var input = new TextBox { Text = prior ?? "", MaxLength = 4000 };
                AutomationProperties.SetName(input, field.Label);
                AutomationProperties.SetAutomationId(input, "field-" + field.Id);
                input.TextChanged += (_, _) => Changed();
                readers[field.Id] = () => input.Text;
                Body.Children.Add(input);
            }
        }
        var openText = simple && record.Question.Options is null;
        Body.Children.Add(Text(openText ? "Your response" : "Add a note · optional", 13, "#25342E", true));
        comment = new TextBox { Text = draft.Comment, AcceptsReturn = true, TextWrapping = TextWrapping.Wrap,
            MinHeight = openText ? 100 : 50, MaxHeight = 180, VerticalScrollBarVisibility = ScrollBarVisibility.Auto, MaxLength = 6000 };
        AutomationProperties.SetName(comment, openText ? "Your response" : "Optional note");
        AutomationProperties.SetAutomationId(comment, "comment");
        comment.TextChanged += (_, _) => Changed();
        Body.Children.Add(comment);
        error = Text("", 12, "#A13E29", false, new Thickness(0,0,0,10));
        error.Visibility = Visibility.Collapsed;
        AutomationProperties.SetLiveSetting(error, AutomationLiveSetting.Polite);
        AutomationProperties.SetAutomationId(error, "validation-error");
        Body.Children.Add(error);
        if (simple && record.Question.Options is { } choices)
        {
            foreach (var option in choices)
                Body.Children.Add(Action(option, "choice-" + Array.IndexOf(choices, option), () => Submit("answer", option), true));
        }
        else Body.Children.Add(Action("Send feedback", "send", () => Submit("answer", choice), true));
        var secondary = new WrapPanel { Margin = new Thickness(0,12,0,0) };
        secondary.Children.Add(Action("Later", "later", Defer, false));
        secondary.Children.Add(Action("Dismiss", "dismiss", () => Submit("dismiss", null), false));
        Body.Children.Add(secondary);
        Footer.Text = record.ThreadId is null ? "Answers stay on this device · your agent checks for replies" : "Replies return to the originating Codex task automatically";
    }

    private void Submit(string action, string? selected)
    {
        if (current is null) return;
        try
        {
            SaveDraft();
            var values = new Dictionary<string, JsonElement>();
            if (action == "answer") foreach (var field in current.Question.Fields ?? [])
            {
                var raw = readers[field.Id]();
                if (string.IsNullOrWhiteSpace(raw)) continue;
                object value = raw;
                if (field.Type == "number")
                {
                    if (!double.TryParse(raw, NumberStyles.Float, CultureInfo.CurrentCulture, out var number))
                        throw new ArgumentException($"{field.Label} must be a number.");
                    value = number;
                }
                else if (field.Type == "boolean") value = raw == "Yes";
                values[field.Id] = JsonSerializer.SerializeToElement(value);
            }
            var accepted = store.Submit(current.Id, new(Guid.NewGuid().ToString("N"), action, selected, values, comment?.Text));
            Render(accepted);
            Refresh();
        }
        catch (Exception ex) { if (error is not null) { error.Text = ex.Message; error.Visibility = Visibility.Visible; error.BringIntoView(); } }
    }

    private void Defer()
    {
        if (current is null) return;
        later.Add(current.Id);
        SavePanelState();
        SaveDraft();
        Next();
    }

    private void Next()
    {
        var next = store.List().FirstOrDefault(x => x.Status == "pending" && !later.Contains(x.Id) && x.Id != current?.Id);
        if (next is not null) Select(next);
        else { current = null; RenderEmpty(); Footer.Text = "You can return to a question from the inbox whenever you are ready."; }
    }

    private void Changed() { draftTimer.Stop(); draftTimer.Start(); }
    private void SaveDraft()
    {
        if (current?.Status != "pending" || comment is null) return;
        try { store.SaveDraft(current.Id, new(readers.ToDictionary(x => x.Key, x => x.Value()), comment.Text, choice)); }
        catch (Exception ex) { Footer.Text = "Draft could not be saved: " + ex.Message; }
    }

    private void RenderEmpty()
    {
        readers.Clear(); comment = null; Body.Children.Clear();
        Body.Children.Add(Text("ROOM TO THINK", 11, "#63715B", true));
        Body.Children.Add(Text("You're all caught up.", 28, "#25342E", true, new Thickness(0,18,0,12)));
        Body.Children.Add(Text("When your agent needs a decision, it will appear here. You can keep working; AgentUI will let you know.", 15, "#596353"));
        if (!settings.Stdio) Body.Children.Add(Action("Try a sample question", "demo", () =>
        {
            store.Create(new("How would you like to work together?", "This is a sample. Your answer stays on this device and is not sent to an agent.",
                "Sample", ["Ask me as you go", "Group questions together"]), settings.Session, "Demo", null);
            Refresh();
        }, false));
    }

    private void OpenSetup(object sender, RoutedEventArgs e)
    {
        var body = new StackPanel { Margin = new Thickness(26) };
        body.Children.Add(Text("Connect to Codex", 25, "#25342E", true));
        body.Children.Add(Text("Install AgentUI for your Windows account and add its Codex connection. Existing connections stay in place. Codex will open the panel when it needs your input.", 14, "#596353", false, new Thickness(0,16,0,20)));
        var status = Text("No account, browser or background service is needed for AgentUI.", 13, "#596353", false, new Thickness(0,12,0,0));
        var button = new Button { Content = "Install and connect", Padding = new Thickness(16,10,16,10), HorizontalAlignment = HorizontalAlignment.Left };
        AutomationProperties.SetAutomationId(button, "install-connect");
        button.Click += async (_, _) =>
        {
            button.IsEnabled = false; status.Text = "Connecting…";
            try { status.Text = await Setup.Connect(settings); button.Content = "Connected"; }
            catch (Exception ex) { status.Text = ex.Message; button.IsEnabled = true; }
        };
        body.Children.Add(button); body.Children.Add(status);
        new Window { Title = "AgentUI — Connect", Owner = this, Content = body, Width = 480, SizeToContent = SizeToContent.Height,
            ResizeMode = ResizeMode.NoResize, WindowStartupLocation = WindowStartupLocation.CenterOwner,
            Background = Background, FontFamily = FontFamily }.ShowDialog();
    }

    private static TextBlock Text(string value, double size, string color, bool bold = false, Thickness? margin = null) => new()
    {
        Text = value, FontSize = size, Foreground = (Brush)new BrushConverter().ConvertFromString(color)!,
        FontWeight = bold ? FontWeights.SemiBold : FontWeights.Normal,
        TextWrapping = TextWrapping.Wrap, Margin = margin ?? new Thickness(0)
    };

    private static Button Action(string title, string id, System.Action click, bool primary)
    {
        var button = new Button { Content = new TextBlock { Text = title, TextWrapping = TextWrapping.Wrap },
            HorizontalAlignment = HorizontalAlignment.Stretch };
        if (primary) { button.Background = new SolidColorBrush(Color.FromRgb(37,52,46)); button.Foreground = Brushes.White; }
        AutomationProperties.SetName(button, title);
        AutomationProperties.SetAutomationId(button, id);
        button.Click += (_, _) => click();
        return button;
    }

    private void SavePanelState()
    {
        try { FeedbackStore.Atomic(Path.Combine(store.Root, "panel.json"), Json.Encode(new PanelState(current?.Id, hiddenByUser, muted, Topmost, later.ToArray()))); }
        catch (Exception ex) { Console.Error.WriteLine($"Cannot save panel state: {ex.Message}"); }
    }
    private void HidePanel() { SaveDraft(); hiddenByUser = true; SavePanelState(); Hide(); }
    private void TogglePin(object sender, RoutedEventArgs e) { Topmost = !Topmost; PinButton.Content = Topmost ? "Unpin panel" : "Pin panel"; SavePanelState(); }
    private void OnClosing(object? sender, CancelEventArgs e) { if (!closing) { e.Cancel = true; HidePanel(); } }
    public void Dispose() { closing = true; draftTimer.Stop(); SaveDraft(); tray.Dispose(); Close(); }
}
