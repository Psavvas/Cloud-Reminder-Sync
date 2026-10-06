[CmdletBinding()]
param(
    [ValidateSet('Debug', 'Release')]
    [string]$Configuration = 'Release',
    [ValidateSet('x64', 'ARM64')]
    [string]$Architecture = 'x64',
    [switch]$LockedRestore
)

$ErrorActionPreference = 'Stop'
$repository = Split-Path -Parent $PSScriptRoot
$project = Join-Path $repository 'src\Reminders.WinUI\Reminders.WinUI.csproj'
$runtime = if ($Architecture -eq 'ARM64') { 'win-arm64' } else { 'win-x64' }
$output = if ($Architecture -eq 'ARM64') { Join-Path $repository 'dist-windows-arm64' } else { Join-Path $repository 'dist-windows' }

$restoreArguments = @('restore', $project, '--runtime', $runtime, "-p:Platform=$Architecture")
if ($LockedRestore) { $restoreArguments += '--locked-mode' }
dotnet @restoreArguments
if ($LASTEXITCODE -ne 0) { throw 'The .NET build metadata restore failed.' }

$repositoryPath = [IO.Path]::GetFullPath($repository).TrimEnd('\')
$outputPath = [IO.Path]::GetFullPath($output).TrimEnd('\')
if (-not $outputPath.StartsWith($repositoryPath + '\', [StringComparison]::OrdinalIgnoreCase) -or $outputPath -eq $repositoryPath) {
    throw "Refusing to clean unsafe publish path: $outputPath"
}
if (Test-Path -LiteralPath $outputPath) {
    Remove-Item -LiteralPath $outputPath -Recurse -Force
}

dotnet publish $project --configuration $Configuration --runtime $runtime --no-restore --self-contained true --output $output -p:Platform=$Architecture
if ($LASTEXITCODE -ne 0) { throw 'The native Windows frontend build failed.' }

if (-not (Test-Path -LiteralPath (Join-Path $output 'Reminders.Core.dll'))) {
    throw 'The published app does not contain the integrated C# backend.'
}

Write-Host "Native Windows $Architecture app: $output\Reminders.exe"
