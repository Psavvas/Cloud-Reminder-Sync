<div align="center">

<img src="src-windows/Reminders.WinUI/Assets/icon-v2.png" width="112" alt="Reminders app icon">

# Reminders for Windows

**A native WinUI 3 client for iCloud Reminders, backed by Rust.**

</div>

Apple does not ship Reminders for Windows. This project provides a fast,
offline-capable Windows client with background sync and due-date notifications.
The interface is C# and WinUI 3 with no embedded browser or web frontend.
Python, Node.js and npm are not required. Authentication and iCloud communication
run in the bundled native Rust connector.

> This independent project is not authorized, sponsored, endorsed, or otherwise
> approved by Apple Inc. It uses private, undocumented iCloud interfaces that
> may change or stop working without notice. Read [LEGAL.md](LEGAL.md) before
> using or distributing the software.

## Quick start: demo mode

From the repository root, run:

```powershell
.\scripts\run-windows.ps1 -Demo
```

This launches the complete native UI with in-memory sample data. It does not
build or start the Rust sidecar, read the application cache, request
credentials, or connect to iCloud. Only the .NET 8 SDK is needed.

After creating a production build, demo mode can also be launched directly:

```powershell
.\dist-windows\Reminders.exe --demo
```

See [Getting started on Windows](docs/getting-started.md) for every supported
development, production, testing, and troubleshooting command.

## Build and run the complete application

Production prerequisites are:

- Windows 10 version 1809 or newer on x64;
- .NET 8 SDK or newer;
- Rust with the MSVC toolchain;
- Visual Studio 2022 or Build Tools with the Windows SDK and MSVC x64 tools.

The project restores its Windows App SDK dependencies through NuGet. It does
not require Node.js, npm, JavaScript packages, or a browser runtime.

```powershell
.\scripts\check-prereqs.ps1
.\scripts\build-windows.ps1
.\dist-windows\Reminders.exe
```

For a normal Debug development launch:

```powershell
.\scripts\run-windows.ps1
```

The production build is a self-contained x64 folder under `dist-windows`.
Keep that folder together because the native WinUI resources, Windows App SDK
runtime files, application icon, and Rust sidecar are all required.

To build an installer instead, with [Inno Setup 6](https://jrsoftware.org/isdl.php)
installed:

```powershell
.\scripts\build-installer.ps1 -Architecture x64
```

That writes `dist-installer\Reminders-for-Windows-x64-Setup.exe`, which installs
per-user with no admin prompt. ARM64 and MSIX are supported too:

```powershell
.\scripts\build-windows.ps1 -Architecture ARM64
.\scripts\build-installer.ps1 -Architecture ARM64
.\scripts\build-msix.ps1 -Architecture x64
```

ARM64 portable output is written to `dist-windows-arm64` and MSIX packages to
`dist-msix`. Both the installer and the MSIX are unsigned unless a certificate
thumbprint is provided — but an unsigned installer still installs after a
SmartScreen prompt, whereas Windows refuses an unsigned MSIX outright. See the
getting-started guide for signing and Store identity options.

## App updates

Installer builds check GitHub for updates on launch and every six hours while
running. A banner offers **Install and restart** when a newer stable release is
available. You can also use **Settings → Check for updates**. The app downloads
the matching x64 or ARM64 installer, checks its size and GitHub SHA-256 digest,
then upgrades the existing installation and reopens. Reminder data, queued
changes, preferences and sign-in credentials are kept. Save unfinished edits
before restarting. Failed checks never prevent normal app use.

Install a build containing the updater once to enable it. Portable, development
and MSIX builds offer a link to the release instead of running an installer.

To ship an update, increase `<Version>` in the WinUI project and `version` in
`sidecar/Cargo.toml` together, commit the changes, and push a matching tag (for
example `v0.4.0`). CI creates a draft with both installers. Review and publish
that draft as a stable GitHub release; drafts and prereleases are ignored by
the updater. Committing code alone does not distribute an update.

## What works

| Feature | Support |
|---|---|
| Lists and reminders | Read and write |
| Title, notes, due date, priority, completion and flags | Read and write |
| Smart lists | Today, Upcoming, All, Completed and Deleted |
| Search and sorting | Local SQLite queries; the network is not on the click path |
| Tags | Read and filter only (an upstream iCloud limitation) |
| Offline use | Full reads; edits queue for the next successful sync |
| Automatic sync | After local edits, on launch/return, and every 10 minutes by default |
| Authentication | Apple ID, 2FA by text message, terms acceptance and session restoration |
| Conflicts | Preserved and explicitly resolved instead of overwritten |
| Windows integration | Native theme, controls, badges and due notifications |
| Sidebar | Persisted open/collapsed state with temporary hover expansion |

Creating or changing reminder lists is not supported because records created
through CloudKit Web Services do not propagate to Apple devices. The live
protocol findings are documented in
[docs/protocol-findings.md](docs/protocol-findings.md).

Two-factor sign-in uses a texted code. Apple verifies the code shown in a
trusted-device prompt through an HSA2 websocket bridge that this connector does
not implement yet, so an account with no trusted phone number cannot currently
finish signing in -- see the protocol findings for what porting it involves.

## Sync behavior and troubleshooting

Changes made in Windows are saved locally and immediately queued for upload.
Sync also runs when you launch the app, when you return to its window (with a
30-second throttle), and every **10 minutes** by default while the app is
running. The interval is configurable in Settings. Changes made on an iPhone
arrive on the next sync; use **Sync now** to check immediately.

After the initial download, routine sync fetches changes since the saved iCloud
cursor. Lists and tags are refreshed when their records change, avoiding a full
collection download on every check. Edits made during a sync receive a follow-up
upload pass, and newer queued edits are preserved when an earlier upload finishes.

The native connector reads and writes Apple's compressed title and notes
documents. Completion updates include a completion timestamp and Apple's
field-level merge metadata so other Apple clients can apply them. Deleted list
records are excluded from navigation. Protocol upgrades that require rebuilding
cached data trigger a fresh download automatically.

The sync banner shows the current stage, including uploads, list downloads, and
changed reminders. Upload failures and conflicts are surfaced in the app;
failed uploads remain queued. Sync has request and pagination limits and a
10-minute timeout per pass so a stalled request does not leave it running forever.

For debugging, inspect `%LOCALAPPDATA%\RemindersSync\logs\app.log`. Sync
request diagnostics include the operation, HTTP status, response size, page
counts, and elapsed time without logging reminder text or session tokens in those
request entries. Include the app version and the relevant sync error when
reporting a problem; review logs before sharing them.

## Architecture

```text
WinUI 3 / C#
  native window, navigation, dialogs, theming and Windows integration
          |
          | newline-delimited JSON over private stdio
          v
Rust sidecar
  authentication, CloudKit, sync, conflicts and notification planning
          +-- SQLite cache and outbox
          +-- iCloud CloudKit Web Services
```

The sidecar remains a separate supervised process. A connector crash cannot
corrupt the UI process, and reminder reads are served by the local SQLite
cache. Normal application data is stored under
`%LOCALAPPDATA%\RemindersSync`; demo mode does not access it.

See [the native frontend notes](docs/native-windows-frontend.md) for the process
boundary and native control architecture.

## Tests

```powershell
cargo test --manifest-path .\sidecar\Cargo.toml --locked
dotnet run --project .\src-windows\AppUpdater.Tests\AppUpdater.Tests.csproj -c Release
dotnet build .\src-windows\Reminders.WinUI\Reminders.WinUI.csproj -c Release -p:Platform=x64
```

The Rust tests are account-free. Live iCloud interoperability testing should
use a disposable account and follow [SECURITY_AUDIT.md](SECURITY_AUDIT.md).

## Repository layout

```text
sidecar/                       Rust iCloud connector and offline engine
src-windows/Reminders.WinUI/  Native WinUI 3 frontend
scripts/                       Build, run and prerequisite checks
docs/                          Setup, protocol and architecture notes
```

## Credits

Maintained by Paul Savvas. Package publisher:
[paulsavvas.com](https://paulsavvas.com).

Apple and iCloud are trademarks of Apple Inc., registered in the U.S. and other
countries and regions. Apple product names are used only to describe
compatibility. See the [legal and service notice](LEGAL.md).
