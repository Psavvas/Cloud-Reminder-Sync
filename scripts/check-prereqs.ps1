[CmdletBinding()]
param(
    [switch]$Demo,
    [ValidateSet('x64', 'ARM64')]
    [string]$Architecture = 'x64',
    [switch]$Msix
)

$ErrorActionPreference = 'Stop'
$problems = [Collections.Generic.List[string]]::new()

function Test-Command([string]$Label, [string]$Command) {
    $found = Get-Command $Command -ErrorAction SilentlyContinue
    if ($found) {
        $version = (& $Command --version 2>&1 | Select-Object -First 1)
        Write-Host "  [ok]      $Label - $version" -ForegroundColor Green
    } else {
        Write-Host "  [missing] $Label" -ForegroundColor Red
        $problems.Add($Label)
    }
}

Write-Host "`nBuild prerequisites`n" -ForegroundColor Cyan
Test-Command '.NET SDK' 'dotnet'
Write-Host '  [not needed] Node.js / npm' -ForegroundColor DarkGray

if ($Demo) {
    if ($problems.Count) { Write-Host "`nMissing: $($problems -join ', ')" -ForegroundColor Red; exit 1 }
    Write-Host "`nAll good. Launch the account-free UI with .\scripts\run-windows.ps1 -Demo`n" -ForegroundColor Green
    exit 0
}

$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (Test-Path $vswhere) {
    $visualStudio = & $vswhere -latest -products * -property installationPath
    if ($visualStudio) { Write-Host "  [ok]      Visual Studio - $visualStudio" -ForegroundColor Green }
    else { Write-Host '  [missing] Visual Studio / Build Tools' -ForegroundColor Red; $problems.Add('Visual Studio') }
} else {
    Write-Host '  [missing] Visual Studio / MSVC discovery' -ForegroundColor Red
    $problems.Add('Visual Studio')
}

if ($Msix) {
    $sdkRoot = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
    $makeAppx = Get-ChildItem $sdkRoot -Recurse -Filter makeappx.exe -ErrorAction SilentlyContinue | Where-Object FullName -Match '\\x64\\makeappx\.exe$' | Select-Object -First 1
    if ($makeAppx) { Write-Host "  [ok]      MSIX packaging - $($makeAppx.FullName)" -ForegroundColor Green }
    else { Write-Host '  [missing] MakeAppx.exe' -ForegroundColor Red; $problems.Add('Windows SDK MSIX tools') }
}

if ($problems.Count) { Write-Host "`nMissing: $($problems -join ', ')" -ForegroundColor Red; exit 1 }
Write-Host "`nAll good. Run .\scripts\build-windows.ps1 for a production build.`n" -ForegroundColor Green
