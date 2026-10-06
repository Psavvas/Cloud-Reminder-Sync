# Native Windows frontend

The desktop client is a C# WinUI 3 application using the Windows App SDK. Its
shell is composed from Windows controls including `NavigationView`, `InfoBar`,
`CommandBar`, `ListView`, `ContentDialog`, `CalendarDatePicker`, `TimePicker`,
`ToggleSwitch`, `InfoBadge`, and the platform theme resources.

No web frontend is built or shipped. The interface is implemented entirely with
Windows UI controls and verified through Windows UI Automation.

## Integrated C# backend

Reminders.Core runs in the WinUI process. SyncClient makes asynchronous calls
and forwards authentication, sync and conflict events onto the UI dispatcher.
SQLite and authentication work run off the UI thread. The service cancels outstanding
work before disposing its cache and HTTP client.

The SQLite schema and Windows Credential Manager target names remain compatible.
There is no executable discovery, child process or JSON pipe. Unpackaged data is
under %LOCALAPPDATA%\RemindersSync; Windows may virtualize it for an MSIX app.

## Demo mode

`--demo` is a first-class, side-effect-free UI testing mode. It does not start
the backend or access the real cache. Sample lists and reminders live only in
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

Drag the divider all the way right to collapse reminder details. Selecting a
reminder then opens its details across the content area with **Back to reminders**,
just as automatic sizing does in a narrow window. Back restores the list and
collapsed divider. Drag the divider left, double-click it, or choose Automatic
detail sizing in Settings to restore a split view. A custom detail share of 0%
uses the same behavior, including after restarting the app.

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
- Collapse details while a reminder is selected, then select that same reminder:
  full-width details and Back must be available. Back returns to the list with
  the divider still collapsed. Reopen the divider and check split-view editing.
- Repeat with Settings' Custom detail share set to 0%, then check automatic
  detail sizing in a narrow window and a nonzero custom split.

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
