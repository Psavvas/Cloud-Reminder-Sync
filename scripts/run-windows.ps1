[CmdletBinding()]
param(
    [switch]$Demo,
    [ValidateSet('Auto', 'x64', 'ARM64')]
    [string]$Architecture = 'Auto'
)

$ErrorActionPreference = 'Stop'
$repository = Split-Path -Parent $PSScriptRoot
$resolvedArchitecture = if ($Architecture -ne 'Auto') { $Architecture } elseif ([Runtime.InteropServices.RuntimeInformation]::OSArchitecture -eq [Runtime.InteropServices.Architecture]::Arm64) { 'ARM64' } else { 'x64' }
$runtime = if ($resolvedArchitecture -eq 'ARM64') { 'win-arm64' } else { 'win-x64' }
$project = Join-Path $repository 'src-windows\Reminders.WinUI\Reminders.WinUI.csproj'

if ($Demo) { dotnet run --project $project --configuration Debug --runtime $runtime -p:Platform=$resolvedArchitecture -- --demo }
else { dotnet run --project $project --configuration Debug --runtime $runtime -p:Platform=$resolvedArchitecture }
