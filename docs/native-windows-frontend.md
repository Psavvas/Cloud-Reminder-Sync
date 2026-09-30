# Native Windows frontend

The desktop client is a C# WinUI 3 application using the Windows App SDK. Its
shell is composed from Windows controls including `NavigationView`, `InfoBar`,
`CommandBar`, `ListView`, `ContentDialog`, `CalendarDatePicker`, `TimePicker`,
`ToggleSwitch`, `InfoBadge`, and the platform theme resources.

No web frontend is built or shipped. The interface is implemented entirely with
Windows UI controls and verified through Windows UI Automation.

## Process boundary

The existing Rust binary remains the data and sync backend. It receives
newline-delimited JSON requests over standard input and emits responses and
events over standard output. The native client:

- locates and starts `reminders-sidecar.exe` without a console window;
- proves readiness with `ping` before showing cached data;
- correlates concurrent calls by numeric request ID;
- forwards sync, authentication and conflict events onto the WinUI dispatcher;
- stores its data under `%LOCALAPPDATA%\RemindersSync`;
- shuts the child down when the application exits.

## Demo mode

`--demo` is a first-class, side-effect-free UI testing mode. It does not start
the sidecar or access the real cache. Sample lists and reminders live only in
memory. The sign-in screen exposes the same path through an **Explore the UI
with sample data** button.

## Reminder and sidebar controls

Only Today shows a count badge. Today includes unfinished reminders due today
or earlier; Upcoming shows all unfinished dated reminders, including overdue
work. Overdue dates include an **Overdue** label and use the Windows critical
(red) color. All-day reminders become overdue on the following day; timed
reminders become overdue after their due time.

Right-click a list (or press Shift+F10 while it is focused) to change its icon
and color or move it up/down. These preferences are stored on this PC in
`%LOCALAPPDATA%\RemindersForWindows\ui-settings.json`; they do not modify
list metadata in iCloud. The existing sidebar open/collapsed preference is
preserved when customization is saved.

Sync now acts as a command and leaves the active list selected. Settings is a
scrollable dialog grouped into appearance, reminder defaults, sync, and list
customization guidance. It remembers the saved theme and offers a default
creation list and a completion-animation toggle.

The New reminder popup accepts notes, list, optional due date, all-day/time,
priority, and flag before saving. Opening it from Today defaults the due date
to today; opening it from a list uses that list. Completion holds the checked
row for 180 ms, then fades it over 420 ms before refreshing. Windows' reduced
motion preference and the app's animation toggle disable that delay.

Demo mode includes an overdue reminder and supports filtering, creation,
completion, customization, settings, and deletion in memory.

### Manual UI verification

Run `scripts\run-windows.ps1 -Demo`, then check:

- Today is the only sidebar item with a badge; the sample overdue item is red
  and labeled in Today and Upcoming.
- Sync now can be clicked repeatedly without replacing the selected list.
- Right-click a list, change its icon/color, and move it up/down; tags and the
  active list remain available.
- The add popup validates an empty title, allows an undated reminder, and saves
  due date, time, priority, and flag together.
- Settings opens on the current theme; Save applies changes and Cancel keeps
  the prior settings. Reopening shows the saved values.
- Completing a reminder visibly checks it before it leaves the list; disabling
  animation removes the pause.

For persistence checks, run the regular app and restart after customizing a
list. Demo mode intentionally does not write preferences.

## Build

Node.js and npm are not used. Run these commands from the repository root:

```powershell
.\scripts\run-windows.ps1
.\scripts\run-windows.ps1 -Demo
.\scripts\build-windows.ps1
.\scripts\build-windows.ps1 -Architecture ARM64
.\scripts\build-msix.ps1 -Architecture x64
```

The release output is a self-contained x64 folder under `dist-windows`.
ARM64 output is written to `dist-windows-arm64`, and MSIX packages are written
to `dist-msix`.
For prerequisites, direct executable commands, tests, and troubleshooting, see
[Getting started on Windows](getting-started.md).
