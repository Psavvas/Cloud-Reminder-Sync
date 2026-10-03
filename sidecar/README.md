# Retired Rust backend

The application now uses `src-windows/Reminders.Core`, a C# library running in
the WinUI process. This directory is retained as a migration reference only.
The application, build scripts, Visual Studio solution, and CI do not build,
launch, or package this code. New fixes belong in the C# backend.
