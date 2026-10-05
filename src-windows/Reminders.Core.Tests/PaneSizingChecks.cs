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
        check(smaller.List == 320 && smaller.Details == 480, "Resize preserves readable list minimum");
        check(PaneSizing.Calculate(1008, 0.65) == custom, "Custom share returns when window grows");
        var narrow = PaneSizing.Calculate(647, 0.65);
        check(narrow.Compact && narrow.List == 647 && narrow.Details == 647, "Narrow windows use full-width single panes");
        var boundary = PaneSizing.Calculate(648, null);
        check(!boundary.Compact && boundary.List == 320 && boundary.Details == 320, "Two panes fit exactly at minimum width");
        foreach (var fraction in new double?[] { null, 0.2, 0.8, -1, 2, double.NaN, double.PositiveInfinity })
        {
            foreach (var width in new[] { 648d, 808, 1008, 2008 })
            {
                var sizes = PaneSizing.Calculate(width, fraction);
                check(sizes.List >= 320 && sizes.Details >= 320 && Math.Abs(sizes.List + sizes.Details + 8 - width) < 0.001, "Panes respect minimum widths and available space");
            }
        }
        check(PaneSizing.Calculate(1008, double.NaN) == balanced, "Invalid saved share falls back to automatic sizing");
        check(PaneSizing.Calculate(double.NaN, null).Compact, "Invalid or unmeasured width stays compact");
    }
}
