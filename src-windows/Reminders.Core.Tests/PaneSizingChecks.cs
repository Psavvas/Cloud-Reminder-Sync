using Reminders.Windows.Services;

internal static class PaneSizingChecks
{
    public static void Run(Action<bool, string> check)
    {
        var balanced = PaneSizing.Calculate(1008, null);
        check(!balanced.Compact && balanced.List == 500 && balanced.Details == 500, "Automatic panes share usable width equally");
        var custom = PaneSizing.Calculate(1008, 0.65);
        check(custom.List == 350 && custom.Details == 650, "Custom detail share applies to usable width");
        var smaller = PaneSizing.Calculate(808, 0.65);
        check(smaller.List == 280 && smaller.Details == 520, "Custom sizing respects the chosen share without minimum stops");
        check(PaneSizing.Calculate(1008, 0.65) == custom, "Custom share returns when window grows");
        var narrow = PaneSizing.Calculate(647, null);
        check(narrow.Compact && narrow.List == 647 && narrow.Details == 647, "Automatic sizing uses full-width single panes in narrow windows");
        var boundary = PaneSizing.Calculate(648, null);
        check(!boundary.Compact && boundary.List == 320 && boundary.Details == 320, "Automatic panes fit exactly at minimum width");
        foreach (var fraction in new double?[] { null, 0, 0.2, 0.8, 1, -1, 2, double.NaN, double.PositiveInfinity })
        {
            foreach (var width in new[] { 648d, 808, 1008, 2008 })
            {
                var sizes = PaneSizing.Calculate(width, fraction);
                check(sizes.List >= 0 && sizes.Details >= 0 && Math.Abs(sizes.List + sizes.Details + 8 - width) < 0.001, "Custom panes stay inside available space");
            }
        }
        foreach (var width in new[] { 400d, 647, 648, 1008, 2008 })
        {
            var detailsClosed = PaneSizing.Calculate(width, 0);
            var listClosed = PaneSizing.Calculate(width, 1);
            check(!detailsClosed.Compact && detailsClosed.Details == 0 && detailsClosed.List == width - 8, "Details can close completely at any usable window width");
            check(!listClosed.Compact && listClosed.List == 0 && listClosed.Details == width - 8, "List can close completely at any usable window width");
        }
        check(PaneSizing.Calculate(1008, double.NaN) == balanced, "Invalid saved share falls back to automatic sizing");
        check(PaneSizing.Calculate(647, double.NaN).Compact, "Invalid custom preference uses compact automatic sizing");
        check(PaneSizing.Calculate(double.NaN, null).Compact, "Invalid or unmeasured width stays compact");
        check(PaneSizing.Calculate(8, 0.5).Compact, "A window smaller than the divider cannot split");

        var closedDetails = PaneSizing.Resize(1008, null, 1000);
        var closedList = PaneSizing.Resize(1008, null, -1000);
        check(closedDetails == 0, "Dragging past the right edge closes details without overshoot");
        check(closedList == 1, "Dragging past the left edge closes the list without overshoot");
        check(PaneSizing.Resize(1008, closedDetails, -24) == 0.024, "Dragging or pressing Left reopens closed details");
        check(PaneSizing.Resize(1008, closedList, 24) == 0.976, "Dragging or pressing Right reopens the closed list");
        check(PaneSizing.Resize(1008, 0.05, 10) == 0.04, "Resizing near an edge stays continuous without a snap jump");
        check(PaneSizing.Resize(1008, 0.5, double.NaN) == 0.5, "An invalid drag delta leaves the split unchanged");
        check(PaneSizing.Resize(647, null, 24) is null, "Resizing does not change a compact automatic layout");
        check(PaneSizing.Calculate(400, PaneSizing.Resize(400, 0, -24)).Details == 24, "Collapsed panes reopen in a narrow window");
    }
}
