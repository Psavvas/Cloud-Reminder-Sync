namespace Reminders.Windows.Services;

internal readonly record struct PaneWidths(bool Compact, double List, double Details);

internal static class PaneSizing
{
    public const double MinimumWidth = 320;
    public const double DividerWidth = 8;

    public static PaneWidths Calculate(double width, double? detailFraction)
    {
        width = double.IsFinite(width) ? Math.Max(0, width) : 0;
        if (width < MinimumWidth * 2 + DividerWidth) return new(true, width, width);
        var available = width - DividerWidth;
        var fraction = detailFraction is { } value && double.IsFinite(value) ? Math.Clamp(value, 0, 1) : 0.5;
        var details = Math.Clamp(available * fraction, MinimumWidth, available - MinimumWidth);
        return new(false, available - details, details);
    }
}
