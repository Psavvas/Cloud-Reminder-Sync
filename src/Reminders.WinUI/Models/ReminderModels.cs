using System.ComponentModel;
using System.Runtime.CompilerServices;
using System.Text.Json.Serialization;

namespace Reminders.Windows;

public sealed class ReminderList
{
    [JsonPropertyName("id")] public string Id { get; set; } = "";
    [JsonPropertyName("title")] public string Title { get; set; } = "";
    [JsonPropertyName("color_hex")] public string? ColorHex { get; set; }
    [JsonPropertyName("count")] public long Count { get; set; }
    [JsonPropertyName("is_group")] public bool IsGroup { get; set; }
}

public sealed class ReminderItem : INotifyPropertyChanged, Services.IScheduledReminder
{
    private string _title = "";
    private bool _completed;
    [JsonPropertyName("id")] public string Id { get; set; } = "";
    [JsonPropertyName("list_id")] public string ListId { get; set; } = "";
    [JsonPropertyName("title")] public string Title { get => _title; set => Set(ref _title, value); }
    [JsonPropertyName("description")] public string Description { get; set; } = "";
    [JsonPropertyName("due_date")] public string? DueDate { get; set; }
    [JsonPropertyName("priority")] public long Priority { get; set; }
    [JsonPropertyName("completed")] public bool Completed { get => _completed; set => Set(ref _completed, value); }
    [JsonPropertyName("flagged")] public bool Flagged { get; set; }
    [JsonPropertyName("all_day")] public bool AllDay { get; set; }
    [JsonPropertyName("deleted")] public bool Deleted { get; set; }
    [JsonPropertyName("tags")] public List<string> Tags { get; set; } = [];
    [JsonPropertyName("dirty")] public long Dirty { get; set; }

    [JsonIgnore] public string DisplayTitle => string.IsNullOrWhiteSpace(Title) ? "Untitled reminder" : Title;
    [JsonIgnore] public bool IsOverdue => !Completed && !Deleted && DateTimeOffset.TryParse(DueDate, out var due)
        && (AllDay ? due.LocalDateTime.Date < DateTime.Today : due < DateTimeOffset.Now);
    [JsonIgnore] public Microsoft.UI.Xaml.Media.Brush MetadataBrush => IsOverdue
        ? (Microsoft.UI.Xaml.Media.Brush)Microsoft.UI.Xaml.Application.Current.Resources["SystemFillColorCriticalBrush"]
        : (Microsoft.UI.Xaml.Media.Brush)Microsoft.UI.Xaml.Application.Current.Resources["AccentTextFillColorPrimaryBrush"];
    [JsonIgnore] public string DueText
    {
        get
        {
            if (!DateTimeOffset.TryParse(DueDate, out var value)) return "";
            var due = value.LocalDateTime;
            if (AllDay) return due.Date == DateTime.Today ? "Today" : due.ToString("ddd, MMM d");
            if (due.Date == DateTime.Today) return $"Today, {due:t}";
            if (due.Date == DateTime.Today.AddDays(1)) return $"Tomorrow, {due:t}";
            return $"{due:ddd, MMM d} · {due:t}";
        }
    }
    [JsonIgnore] public string ApplePriority => Priority switch { 1 => "!!!", 5 => "!!", 9 => "!", _ => "" };
    [JsonIgnore] public string AppleTags => string.Join("  ", Tags.Select(tag => "#" + tag));
    [JsonIgnore] public string ApplePending => Dirty != 0 ? "•" : "";
    [JsonIgnore] public string AppleSyncDescription => Dirty != 0 ? "Waiting to sync" : "";
    [JsonIgnore] public Microsoft.UI.Xaml.Media.Brush AppleDueBrush => new Microsoft.UI.Xaml.Media.SolidColorBrush(
        IsOverdue ? Microsoft.UI.ColorHelper.FromArgb(255, 255, 59, 48)
        : !Completed && DateTimeOffset.TryParse(DueDate, out var due) && due.LocalDateTime.Date <= DateTime.Today.AddDays(1)
            ? Microsoft.UI.ColorHelper.FromArgb(255, 224, 122, 0)
            : Microsoft.UI.ColorHelper.FromArgb(255, 128, 128, 134));
    [JsonIgnore] public string Metadata
    {
        get
        {
            var parts = new List<string>();
            if (IsOverdue) parts.Add("Overdue");
            if (DueText.Length > 0) parts.Add(DueText);
            if (Priority is 1 or 5 or 9) parts.Add(Priority switch { 1 => "High priority", 5 => "Medium priority", _ => "Low priority" });
            if (Tags.Count > 0) parts.Add(string.Join("  ", Tags.Select(t => "#" + t)));
            if (Dirty != 0) parts.Add("Waiting to sync");
            return string.Join("  ·  ", parts);
        }
    }
    public void RefreshTimeMetadata()
    {
        PropertyChanged?.Invoke(this, new(nameof(Metadata)));
        PropertyChanged?.Invoke(this, new(nameof(MetadataBrush)));
        PropertyChanged?.Invoke(this, new(nameof(DueText)));
        PropertyChanged?.Invoke(this, new(nameof(AppleDueBrush)));
        PropertyChanged?.Invoke(this, new(nameof(ApplePriority)));
        PropertyChanged?.Invoke(this, new(nameof(AppleTags)));
        PropertyChanged?.Invoke(this, new(nameof(ApplePending)));
        PropertyChanged?.Invoke(this, new(nameof(AppleSyncDescription)));
    }
    public event PropertyChangedEventHandler? PropertyChanged;
    private void Set<T>(ref T field, T value, [CallerMemberName] string? name = null)
    {
        if (EqualityComparer<T>.Default.Equals(field, value)) return;
        field = value; PropertyChanged?.Invoke(this, new(name)); PropertyChanged?.Invoke(this, new(nameof(DisplayTitle)));
        RefreshTimeMetadata();
    }
}

public sealed class ConflictItem
{
    [JsonPropertyName("id")] public long Id { get; set; }
    [JsonPropertyName("reminder_id")] public string ReminderId { get; set; } = "";
}

public sealed class ReminderTag
{
    [JsonPropertyName("name")] public string Name { get; set; } = "";
    [JsonPropertyName("n")] public long Count { get; set; }
}

public sealed class SyncException(string code, string message, string detail = "") : Exception(message)
{
    public string Code { get; } = code;
    public string Detail { get; } = detail;
}

public sealed record NavEntry(string Label, string Glyph, NavKind Kind, string Key, long Count = 0, string? Color = null);
public enum NavKind { Smart, List, Tag }

public sealed class ReminderSection(string title, IEnumerable<ReminderItem> items)
    : System.Collections.ObjectModel.ObservableCollection<ReminderItem>(items)
{
    public string Title { get; } = title;
}
