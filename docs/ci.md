# CI performance and trust boundaries

The Native Windows build workflow dispatches to one reusable pipeline. Rust
tests/lint, dependency auditing, x64 packaging, and ARM64 packaging run in
parallel. Both architectures still produce and verify the installer, portable
ZIP, and MSIX on every build. Releases wait for the entire pipeline to pass.

## What was slow

Main run [36655722881](https://github.com/Psavvas/iCloud-Reminders-for-Windows/actions/runs/36655722881)
took 14m18s on x64. Its serial steps included 2m45s of release-mode tests,
42s of debug clippy, 5m45s compiling cargo-audit, and 1m54s building the release
sidecar again in an explicit target directory. ARM64 finished in 6m04s.
The matrix jobs used the same Rust cache key, so the first save won and the
other architecture's dependencies were not reliably retained. NuGet packages
were not cached.

The new pipeline tests and lints against shared debug dependencies in a
separate Windows job, builds release binaries in parallel, and downloads the
official cargo-audit 0.22.2 archive with a committed SHA-256 checked before
extraction. Advisories are fetched fresh every run. Already-compressed release
files upload without a redundant artifact compression pass.

## Cache policy

| Trigger | Token cache access | Restore | Save |
| --- | --- | --- | --- |
| Pull request | Read | Trusted main caches | Never |
| Manual build | Read | Available CI caches | Never |
| Push to main | Write | CI caches | After successful dependency/build steps |
| Release tag | None | Never | Never |
| Release publisher | None | Never | Never |

`cache-mode` is declared statically on each caller job. GitHub enforces that
limit on reusable workflows and cache tokens, rather than trusting only a
`save-if` in a cache action. The reusable workflow does not override it.
No `pull_request_target`, `workflow_run`, or privileged PR checkout is used.

Rust keys use a new namespace, target architecture, and separate check/package
purposes, plus rust-cache's toolchain/manifest/lockfile hashes. Cargo's bin
directory is excluded. Only main pushes may save. Existing mixed caches are
not restored under the new namespace.

NuGet caches contain only `.nuget/packages`, never the workspace, credentials,
configuration, app output, or signing material. Exact keys include architecture
and the project/architecture-specific lockfile hashes; there are no broad
restore prefixes. CI uses locked NuGet and Cargo restores. Update both
`packages.win-x64.lock.json` and `packages.win-arm64.lock.json` when changing
package references:

```powershell
dotnet restore src-windows/Reminders.WinUI/Reminders.WinUI.csproj -r win-x64 -p:Platform=x64 --force-evaluate
dotnet restore src-windows/Reminders.WinUI/Reminders.WinUI.csproj -r win-arm64 -p:Platform=ARM64 --force-evaluate
```

A tag build starts on fresh GitHub-hosted runners with caches disabled at both
the token and step level. Even a poisoned CI cache cannot enter the release
build through these cache steps. Production dependencies download anew and
build from the tag's checked-out source and committed lockfiles. A release tag
must match the app/backend version and point to a commit reachable from main.
Only the publisher has `contents: write`; it checks out no source, executes no
application code, and downloads only matching artifacts from its own run.
All external actions are pinned to full commit SHAs and checkout credentials
are not persisted.

This addresses the cache-poisoning path; it does not make compromised
maintainer credentials, malicious changes approved into main, a compromised
upstream dependency, or GitHub runner compromise harmless. Protect main and
release tags with review/access rules. At implementation time the repository
had no repository rulesets; this change does not modify repository access.
New reusable-workflow check names may need updating in existing branch
protection rules.

## Validation

CI runs `scripts/test_ci_policy.py`: policy checks plus negative cases reject
release cache restores, PR cache writes, privileged builders, bypassed release
gates, mutable action tags, and artifacts from other runs. Python is used only
for CI policy tests; the native app still has no Python runtime requirement.
The test dependency is version/hash locked and pip caching is disabled.

Run locally with Python 3.14 on Windows or 3.12 on Linux:

```powershell
python -m pip install --only-binary=:all: --require-hashes -r scripts/ci-policy-requirements.txt
python scripts/test_ci_policy.py
```

Actionlint 1.7.12 validates both workflows with
`-ignore 'unexpected key "cache-mode"'`: that linter release predates the new
GitHub key, so only its unknown-key diagnostic is suppressed. The policy test
checks the exact cache modes, and GitHub's workflow parser is the final
validation for this feature.

References: [GitHub cache restrictions and token modes](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching),
[workflow cache-mode syntax](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#cache-mode),
[rust-cache inputs and key behavior](https://github.com/Swatinem/rust-cache), and
[official cargo-audit 0.22.2 release](https://github.com/rustsec/rustsec/releases/tag/cargo-audit/v0.22.2).
