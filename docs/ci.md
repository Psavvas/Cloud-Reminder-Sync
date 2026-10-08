# Continuous integration

CI runs account-free C# backend tests, checks app/MSIX versions and release ancestry,
validates workflow trust boundaries, and builds x64/ARM64 applications. Each
architecture produces a portable ZIP, optional Inno installer, and unsigned MSIX.
Packaging verifies that `resources.pri` has the final package identity and contains
the startup XAML. The x64 job also registers the extracted MSIX in Developer Mode,
activates it through Windows, checks that its window opens and stays alive, and
removes the test registration. ARM64 packages receive the resource checks; a native
ARM64 launch still needs an ARM64 machine.
Tags create a draft release only after all checks pass. Publishing a stable draft is
a separate decision. MSIX artifacts require signing or Store ingestion to install.

NuGet restores use committed lockfiles. Cache access is read-only for PR/manual
builds, writable only on main pushes, and disabled for tags. Keys include architecture,
app/backend projects and lockfiles. Actions are pinned to full commit hashes.
Checkout does not persist credentials. Release jobs consume only their own run's
artifacts and cannot check out/build code with write permissions.

`scripts/test_ci_policy.py` includes mutation tests for these boundaries. Python is
a CI utility; the application is C#. Rust tooling/caches/audits were retired with
the port. Review NuGet lockfiles and dependency advisories alongside changes.
Live Apple interoperability is outside account-free CI and needs a disposable account.
