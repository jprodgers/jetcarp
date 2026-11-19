<#
.SYNOPSIS
The Windows installer refuses a download that does not match the release's SHA256SUMS.

.DESCRIPTION
SECURITY.md, "Release files". This makes a release folder of its own, its archive holding
-Program as jetcarp.exe and its SHA256SUMS in a release's format, and runs the installer from it
(-DownloadUrl FOLDER) three times, each into a scratch folder:

  1. intact: it installs;
  2. one byte of the archive changed: refused, and nothing installed;
  3. the archive missing from SHA256SUMS: refused, and nothing installed.

"Nothing installed" means the scratch folder is still empty afterwards: no program folder, and
no folder made on the way to it. The installer runs with -NoModifyPath, so the user PATH is not
touched; the test checks that too.

Run it in Windows PowerShell 5.1, as people run the installer:

  powershell -NoProfile -ExecutionPolicy Bypass -File packaging\test-install-ps1.ps1 -Program target\release\jetcarp.exe

.PARAMETER Installer
The installer to test (default: packaging\install.ps1). CI tests the one a release ships, made by
`package.py installers`.

.PARAMETER Program
A jetcarp.exe to put in the archive: the installer runs it (`--version`) once it is in place.
#>
param(
    [string]$Installer = (Join-Path $PSScriptRoot "install.ps1"),
    [Parameter(Mandatory = $true)][string]$Program
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
$Installer = (Resolve-Path $Installer).Path
$Program = (Resolve-Path $Program).Path
# Only what this script passes may steer the installer.
foreach ($v in "JETCARP_VERSION", "JETCARP_INSTALL_DIR", "JETCARP_NO_MODIFY_PATH", "JETCARP_NO_PACKS", "JETCARP_DOWNLOAD_URL", "JETCARP_USE_GH") {
    Remove-Item -Path "Env:$v" -ErrorAction SilentlyContinue
}

$Version = "0.0.0"
$Target = "x86_64-pc-windows-msvc"
$Name = "jetcarp-$Version-$Target"
$Work = Join-Path ([System.IO.Path]::GetTempPath()) ("jetcarp-installer-test-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $Work | Out-Null
$script:Failures = 0

function Ok([string]$Message) { Write-Host "ok: $Message" }
function Bad([string]$Message) {
    Write-Host "FAILED: $Message"
    $script:Failures++
}

function Get-UserPath {
    $key = Get-Item -Path "HKCU:\Environment" -ErrorAction SilentlyContinue
    if (-not $key) { return "" }
    return [string]$key.GetValue("Path", "", [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
}

# A release folder, $Work\$Case, with the archive and SHA256SUMS as a release has them.
function New-Release([string]$Case) {
    $Rel = Join-Path $Work $Case
    $Build = Join-Path $Work "build-$Case\$Name"
    New-Item -ItemType Directory -Force -Path $Rel, $Build | Out-Null
    Copy-Item -Path $Program -Destination (Join-Path $Build "jetcarp.exe")
    $Zip = Join-Path $Rel "$Name.zip"
    Compress-Archive -Path $Build -DestinationPath $Zip
    $Hash = (Get-FileHash -Algorithm SHA256 -Path $Zip).Hash.ToLowerInvariant()
    [System.IO.File]::WriteAllText((Join-Path $Rel "SHA256SUMS"), "$Hash  $Name.zip`n")
    return $Rel
}

# Run the installer from release folder $Rel into a fresh scratch folder, $Work\home-$Case.
# Its output is shown and kept in $script:Out; its exit code is returned.
function Invoke-Installer([string]$Rel, [string]$Case) {
    $Home_ = Join-Path $Work "home-$Case"
    New-Item -ItemType Directory -Force -Path $Home_ | Out-Null
    $ErrorActionPreference = "Continue"
    $script:Out = powershell -NoProfile -ExecutionPolicy Bypass -File $Installer -Version $Version `
        -InstallDir (Join-Path $Home_ "lib\jetcarp") -DownloadUrl $Rel -NoModifyPath -NoPacks 2>&1 | Out-String
    $Code = $LASTEXITCODE
    $ErrorActionPreference = "Stop"
    $script:Out -split "`r?`n" | Where-Object { $_ } | ForEach-Object { Write-Host "    | $_" }
    return $Code
}

# Case $Case must have been refused with a message holding $Want, its folder left empty.
function Test-Refused([string]$Case, [int]$Code, [string]$Want) {
    $Home_ = Join-Path $Work "home-$Case"
    $Left = @(Get-ChildItem -Force -Recurse -LiteralPath $Home_)
    if ($Code -eq 0) {
        Bad "${Case}: the installer accepted it"
    } elseif ($script:Out -notmatch [regex]::Escape($Want)) {
        Bad "${Case}: the refusal does not say `"$Want`""
    } elseif ($script:Out -match "CategoryInfo|FullyQualifiedErrorId") {
        Bad "${Case}: the refusal is not one clean line"
    } elseif ($Left.Count -gt 0) {
        Bad "${Case}: refused, but something was left behind: $($Left.FullName -join ', ')"
    } else {
        Ok "${Case}: refused (`"$Want`"); nothing installed"
    }
}

try {
    Write-Host "testing $Installer"
    $PathBefore = Get-UserPath

    Write-Host "1. intact"
    $Rel = New-Release "intact"
    $Code = Invoke-Installer $Rel "intact"
    $Exe = Join-Path $Work "home-intact\lib\jetcarp\jetcarp.exe"
    if ($Code -ne 0) {
        Bad "intact: the installer failed on an intact release (exit code $Code)"
    } elseif (-not (Test-Path -LiteralPath $Exe) -or $script:Out -notmatch "against SHA256SUMS") {
        Bad "intact: the installer succeeded but $Exe is missing, or it did not say it checked the archive"
    } else {
        Ok "intact: checked against SHA256SUMS and installed"
    }

    Write-Host "2. one byte of the archive changed"
    $Rel = New-Release "tampered"
    $Zip = Join-Path $Rel "$Name.zip"
    $Bytes = [System.IO.File]::ReadAllBytes($Zip)
    $At = [int]($Bytes.Length / 2)
    $Bytes[$At] = [byte](($Bytes[$At] + 1) % 256)
    [System.IO.File]::WriteAllBytes($Zip, $Bytes)
    $Code = Invoke-Installer $Rel "tampered"
    Test-Refused "tampered" $Code "does not match SHA256SUMS"

    Write-Host "3. the archive missing from SHA256SUMS"
    $Rel = New-Release "unlisted"
    [System.IO.File]::WriteAllText((Join-Path $Rel "SHA256SUMS"), ("0" * 64) + "  jetcarp-$Version-README.txt`n")
    $Code = Invoke-Installer $Rel "unlisted"
    Test-Refused "unlisted" $Code "SHA256SUMS does not list"

    if ((Get-UserPath) -ne $PathBefore) { Bad "the user PATH changed, with -NoModifyPath" }
} finally {
    Remove-Item -Recurse -Force $Work -ErrorAction SilentlyContinue
}

if ($script:Failures -gt 0) {
    Write-Host "$($script:Failures) check(s) failed"
    exit 1
}
Write-Host "all three cases passed"
exit 0
