namespace Reminders.Windows.Services;

internal readonly record struct PaneWidths(bool Compact, double List, double Details);

internal static class PaneSizing
{
    public const double MinimumWidth = 320;
    public const double DividerWidth = 8;

    public static PaneWidths Calculate(double width, double? detailFraction, bool detailSelected = false)
    {
        width = double.IsFinite(width) ? Math.Max(0, width) : 0;
        var custom = detailFraction is { } value && double.IsFinite(value);
        if (width <= DividerWidth || (!custom && width < MinimumWidth * 2 + DividerWidth)) return new(true, width, width);
        var available = width - DividerWidth;
        var fraction = custom ? Math.Clamp(detailFraction!.Value, 0, 1) : 0.5;
        // Keep the collapsed divider available on the list, but open a selected
        // reminder through the same full-width layout used in narrow windows.
        if (fraction == 0 && detailSelected) return new(true, width, width);
        var details = available * fraction;
        return new(false, available - details, details);
    }

    public static double? Resize(double width, double? detailFraction, double delta)
    {
        var sizes = Calculate(width, detailFraction);
        if (sizes.Compact || !double.IsFinite(delta)) return detailFraction;
        return Math.Clamp((sizes.Details - delta) / (width - DividerWidth), 0, 1);
    }
}
