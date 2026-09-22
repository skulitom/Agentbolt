using System.Windows;
using System.IO;
using System.Windows.Automation;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using AgentUI;

internal static class UiChecks
{
    internal static void Run(string root, Action<bool, string> check)
    {
        Exception? failure = null;
        var thread = new Thread(() =>
        {
            try
            {
                var app = new Application();
                var store = new FeedbackStore(Path.Combine(root, "ui"));
                var settings = new Settings(store.Root, "ui-test", "Test", null, null, false, false, true);
                // Exercise the real controls without showing windows or tray icons
                // on the developer's desktop. Native focus/handoff have separate checks.
                FeedbackStore.Atomic(Path.Combine(store.Root, "panel.json"), Json.Encode(new PanelState(Hidden: true, Muted: true)));
                var first = store.Create(new("Choose the direction", "A focused question, with enough context to answer.", "UI verification", ["Keep it small", "Add more features"]), "a", "Test", null);
                using (var panel = new PanelWindow(store, settings, showTray: false))
                {
                    panel.Refresh(); Layout(panel);
                    var comment = Find<TextBox>(panel, "comment");
                    comment.Text = "Preserve this note";
                    var second = store.Create(new("Set the limit", Fields: [new("minutes", "Minutes", "number", true, Min: 1, Max: 60), new("sound", "Sound", "boolean")]), "b", "Test", null);
                    panel.Refresh();
                    check(ReferenceEquals(comment, Find<TextBox>(panel, "comment")) && comment.Text == "Preserve this note", "arriving requests preserve the active text control and draft");
                    var options = Descendants(panel).OfType<Button>().Where(b => AutomationProperties.GetAutomationId(b).StartsWith("choice-")).ToArray();
                    check(options.Length == 2, "native choice buttons are reachable");
                    options[0].RaiseEvent(new RoutedEventArgs(Button.ClickEvent));
                    check(store.Read(first.Id).Answer?.Choice == "Keep it small" && store.Read(first.Id).Answer?.Comment == "Preserve this note", "native one-click choice saves the answer and note");
                    Find<Button>(panel, "next").RaiseEvent(new RoutedEventArgs(Button.ClickEvent));
                    Find<Button>(panel, "send").RaiseEvent(new RoutedEventArgs(Button.ClickEvent));
                    check(store.Read(second.Id).Status == "pending" && Descendants(panel).OfType<TextBlock>().Any(x => x.Text == "Minutes is required." && x.Visibility == Visibility.Visible), "native form shows required-field errors without submitting");
                    Find<TextBox>(panel, "field-minutes").Text = "15";
                    Find<Button>(panel, "send").RaiseEvent(new RoutedEventArgs(Button.ClickEvent));
                    var answer = store.Read(second.Id).Answer!;
                    check(answer.Values!["minutes"].GetInt32() == 15 && !answer.Values.ContainsKey("sound"), "native form leaves optional controls unanswered");
                    var third = store.Create(new("A draft to resume", Fields: [new("name", "Name", "text")]), "c", "Test", null);
                    panel.Refresh(); Find<Button>(panel, "next").RaiseEvent(new RoutedEventArgs(Button.ClickEvent));
                    Find<TextBox>(panel, "field-name").Text = "still writing";
                    panel.Close(); // Normal close hides, saving the draft and selection.
                    check(store.ReadDraft(third.Id)?.Values["name"] == "still writing", "closing the panel saves unfinished native input");
                }
                using (var restored = new PanelWindow(store, settings, showTray: false))
                {
                    restored.Refresh(); Layout(restored);
                    check(Find<TextBox>(restored, "field-name").Text == "still writing", "a replacement panel restores the selected question and input");
                    Find<Button>(restored, "later").RaiseEvent(new RoutedEventArgs(Button.ClickEvent));
                    check(store.List().Last().Status == "pending", "Later does not answer or dismiss a request");
                }
                // Render a real WPF question to inspect spacing at the shipped size.
                var visualStore = new FeedbackStore(Path.Combine(root, "visual"));
                FeedbackStore.Atomic(Path.Combine(visualStore.Root, "panel.json"), Json.Encode(new PanelState(Hidden: true, Muted: true)));
                visualStore.Create(new("How should AgentUI announce a new question?", "Questions stay in your inbox while your agent keeps working. Choose how much attention they should ask for.", "AgentUI", ["Show the panel quietly", "Notify, then open on click"]), "visual", "Codex", null);
                using (var visual = new PanelWindow(visualStore, settings, showTray: false))
                {
                    visual.Refresh(); Layout(visual);
                    var bitmap = new RenderTargetBitmap(860, 580, 96, 96, PixelFormats.Pbgra32);
                    bitmap.Render((Visual)visual.Content);
                    var encoder = new PngBitmapEncoder(); encoder.Frames.Add(BitmapFrame.Create(bitmap));
                    var screenshot = Environment.GetEnvironmentVariable("AGENTUI_UI_SCREENSHOT");
                    if (screenshot is not null) using (var output = File.Create(screenshot)) encoder.Save(output);
                }
                app.Shutdown();
            }
            catch (Exception ex) { failure = ex; }
        });
        thread.SetApartmentState(ApartmentState.STA); thread.Start(); thread.Join();
        if (failure is not null) throw new Exception("Native UI check failed", failure);
    }

    private static void Layout(Window window)
    {
        var content = (FrameworkElement)window.Content;
        content.Measure(new Size(860, 580)); content.Arrange(new Rect(0, 0, 860, 580)); content.UpdateLayout();
    }
    private static T Find<T>(DependencyObject root, string id) where T : DependencyObject =>
        Descendants(root).OfType<T>().Single(x => AutomationProperties.GetAutomationId(x) == id);
    private static IEnumerable<DependencyObject> Descendants(DependencyObject root)
    {
        yield return root;
        foreach (var child in LogicalTreeHelper.GetChildren(root).OfType<DependencyObject>())
            foreach (var item in Descendants(child)) yield return item;
    }
}
