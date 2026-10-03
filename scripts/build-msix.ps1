[CmdletBinding()]
param(
    [ValidateSet('x64', 'ARM64')]
    [string]$Architecture = 'x64',
    [ValidatePattern('^[A-Za-z0-9.-]{3,50}$')]
    [string]$IdentityName = 'RemindersForWindows',
    [string]$Publisher = 'CN=paulsavvas.com',
    [string]$PublisherDisplayName,
    [string]$CertificateThumbprint,
    [switch]$Store,
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
if ($Store) {
    foreach ($requiredIdentity in @('IdentityName', 'Publisher', 'PublisherDisplayName')) {
        if (-not $PSBoundParameters.ContainsKey($requiredIdentity) -or [string]::IsNullOrWhiteSpace($PSBoundParameters[$requiredIdentity])) {
            throw "Store submissions require -$requiredIdentity from Partner Center > Product identity."
        }
    }
    if ($CertificateThumbprint) { throw 'Store submissions are signed by Microsoft. Omit -CertificateThumbprint.' }
    if ($Publisher -notmatch '^CN=' -or $Publisher.Contains('OID.2.25.311729368913984317654407730594956997722')) {
        throw 'Use the exact Package/Identity/Publisher assigned by Partner Center for Store submission.'
    }
}
$repository = Split-Path -Parent $PSScriptRoot
$portableOutput = if ($Architecture -eq 'ARM64') { Join-Path $repository 'dist-windows-arm64' } else { Join-Path $repository 'dist-windows' }
$packageOutput = Join-Path $repository $(if ($Store) { 'dist-store' } else { 'dist-msix' })
$project = Join-Path $repository 'src-windows\Reminders.WinUI\Reminders.WinUI.csproj'
$sdkRoot = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
$makeAppx = Get-ChildItem $sdkRoot -Recurse -Filter makeappx.exe -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -match '\\x64\\makeappx\.exe$' } |
    Sort-Object { [version]$_.Directory.Parent.Name } -Descending | Select-Object -First 1
if (-not $makeAppx) { throw 'MakeAppx.exe was not found in the installed Windows SDK.' }

if (-not $SkipBuild) {
    & (Join-Path $PSScriptRoot 'build-windows.ps1') -Architecture $Architecture
    if ($LASTEXITCODE -ne 0) { throw "The $Architecture production build failed." }
}
if (-not (Test-Path (Join-Path $portableOutput 'Reminders.exe'))) {
    throw "The $Architecture portable build is missing. Run .\scripts\build-windows.ps1 -Architecture $Architecture first."
}

if ($CertificateThumbprint) {
    $certificate = Get-Item "Cert:\CurrentUser\My\$CertificateThumbprint" -ErrorAction Stop
    $Publisher = $certificate.Subject
}

[xml]$projectXml = Get-Content -Raw $project
$sourceVersion = [version]$projectXml.Project.PropertyGroup.Version
$packageVersion = "$($sourceVersion.Major).$($sourceVersion.Minor).$($sourceVersion.Build).0"
$processorArchitecture = if ($Architecture -eq 'ARM64') { 'arm64' } else { 'x64' }
$staging = Join-Path ([IO.Path]::GetTempPath()) "reminders-msix-$([guid]::NewGuid().ToString('N'))"

try {
    New-Item -ItemType Directory -Path $staging, (Join-Path $staging 'Assets'), $packageOutput -Force | Out-Null
    Copy-Item -Path (Join-Path $portableOutput '*') -Destination $staging -Recurse -Force
    Get-ChildItem $staging -Filter '*.pdb' -File -ErrorAction SilentlyContinue | Remove-Item -Force
    $packageProject = Join-Path $repository 'src-windows\Reminders.Package'
    Copy-Item -Path (Join-Path $packageProject 'Assets\*.png') -Destination (Join-Path $staging 'Assets') -Force
    [xml]$manifestXml = Get-Content -LiteralPath (Join-Path $packageProject 'Package.appxmanifest') -Raw
    $manifestXml.Package.Identity.SetAttribute('Name', $IdentityName)
    $manifestXml.Package.Identity.SetAttribute('Publisher', $Publisher)
    $manifestXml.Package.Identity.SetAttribute('Version', $packageVersion)
    $manifestXml.Package.Identity.SetAttribute('ProcessorArchitecture', $processorArchitecture)
    if ($Store) { $manifestXml.Package.Properties.PublisherDisplayName = $PublisherDisplayName }
    $manifestXml.Package.Applications.Application.SetAttribute('Executable', 'Reminders.exe')
    $manifestXml.Package.Applications.Application.SetAttribute('EntryPoint', 'Windows.FullTrustApplication')
    $manifestXml.Save((Join-Path $staging 'AppxManifest.xml'))
    $package = Join-Path $packageOutput "Reminders-for-Windows-$Architecture.msix"
    $packOutput = @(& $makeAppx.FullName pack /o /h SHA256 /d $staging /p $package 2>&1)
    if ($LASTEXITCODE -ne 0) { $packOutput | Write-Host; throw 'MakeAppx failed to create the MSIX package.' }
    $packOutput | Select-Object -Last 1 | Write-Host

    if ($CertificateThumbprint) {
        $signTool = Get-ChildItem $sdkRoot -Recurse -Filter signtool.exe -ErrorAction SilentlyContinue |
            Where-Object { $_.FullName -match '\\x64\\signtool\.exe$' } |
            Sort-Object { [version]$_.Directory.Parent.Name } -Descending | Select-Object -First 1
        if (-not $signTool) { throw 'SignTool.exe was not found in the installed Windows SDK.' }
        & $signTool.FullName sign /sha1 $CertificateThumbprint /fd SHA256 $package
        if ($LASTEXITCODE -ne 0) { throw 'SignTool failed to sign the MSIX package.' }
    }
    elseif ($Store) {
        Write-Host 'Store submission package created. Upload it to Partner Center; Microsoft signs the approved distribution. This file is not a trusted installer yet.'
    }
    else {
        Write-Warning 'The MSIX is unsigned. Sign it with -CertificateThumbprint or submit it to the Microsoft Store before installation.'
    }
    Write-Host "MSIX package: $package" -ForegroundColor Green
}
finally {
    $resolvedStaging = [IO.Path]::GetFullPath($staging)
    $resolvedTemp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
    if ($resolvedStaging.StartsWith($resolvedTemp, [StringComparison]::OrdinalIgnoreCase) -and (Test-Path -LiteralPath $resolvedStaging)) {
        Remove-Item -LiteralPath $resolvedStaging -Recurse -Force
    }
}
