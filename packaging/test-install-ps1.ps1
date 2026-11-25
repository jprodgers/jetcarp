<#
.SYNOPSIS
The Windows installer refuses a download that does not match the release's SHA256SUMS.

.DESCRIPTION
SECURITY.md, "Release files". This makes a release folder of its own, its archive holding
-Program as jetcarp.exe and its SHA256SUMS in a release's format, and runs the installer from it
(-DownloadUrl FOLDER), each case into a scratch folder:

  1. intact: it installs, and the program adds JETCARP to the Start menu (#105);
  2. one byte of the archive changed: refused, and nothing installed;
  3. the archive missing from SHA256SUMS: refused, and nothing installed;
  4. -Uninstall, after case 1: the Start menu entry and the program's folder are removed;
  5. another JETCARP, Scoop's (its folder and its shim on the PATH): named, with what removes it
     (#348);
  6. -Uninstall after `jetcarp uninstall --settings --data`: the program the installer runs first
     makes neither %APPDATA%\jetcarp nor %LOCALAPPDATA%\jetcarp again, and a folder holding only
     empty folders is not named as kept (#350).

"Nothing installed" means the scratch folder is still empty afterwards: no program folder, and
no folder made on the way to it. The installer runs with -NoModifyPath, so the user PATH is not
touched; the test checks that too. APPDATA and LOCALAPPDATA point into each case's scratch
folder, so the Start menu entry is made there and not in the user's own Start menu, and so do the
program's own folders (JETCARP_CONFIG_DIR and the rest, as its defaults would be there: the
program finds its default folders through Windows itself, not these variables), so nothing of
the user's is read or deleted.

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
foreach ($v in "JETCARP_VERSION", "JETCARP_INSTALL_DIR", "JETCARP_NO_MODIFY_PATH", "JETCARP_NO_PACKS", "JETCARP_NO_LAUNCHER", "JETCARP_DOWNLOAD_URL", "JETCARP_USE_GH", "SCOOP") {
    Remove-Item -Path "Env:$v" -ErrorAction SilentlyContinue
}
# The program's folders, which each case points into its scratch folder (`Use-Home`).
$FolderVars = @("JETCARP_CONFIG_DIR", "JETCARP_DATA_DIR", "JETCARP_RUNTIME_DIR", "JETCARP_LIBRARY", "JETCARP_OUT")
$FoldersBefore = @{}
foreach ($v in $FolderVars) { $FoldersBefore[$v] = [Environment]::GetEnvironmentVariable($v, "Process") }

$Version = "0.0.0"
$AppDataBefore = $env:APPDATA
$LocalBefore = $env:LOCALAPPDATA

# Points APPDATA, LOCALAPPDATA and the program's folders into the scratch folder $Home_, laid
# out as Windows lays out the defaults: the settings in AppData\Roaming\jetcarp, the data and the
# runtime files in AppData\Local\jetcarp, the pictures in Pictures\JETCARP.
function Use-Home([string]$Home_) {
    $env:APPDATA = Join-Path $Home_ "AppData\Roaming"
    $env:LOCALAPPDATA = Join-Path $Home_ "AppData\Local"
    $env:JETCARP_CONFIG_DIR = Join-Path $env:APPDATA "jetcarp"
    $env:JETCARP_DATA_DIR = Join-Path $env:LOCALAPPDATA "jetcarp"
    $env:JETCARP_RUNTIME_DIR = Join-Path $env:LOCALAPPDATA "jetcarp"
    $env:JETCARP_OUT = Join-Path $Home_ "Pictures\JETCARP"
    Remove-Item -Path "Env:JETCARP_LIBRARY" -ErrorAction SilentlyContinue
}

# Puts them back as they were.
function Restore-Env {
    $env:APPDATA = $AppDataBefore
    $env:LOCALAPPDATA = $LocalBefore
    foreach ($v in $FolderVars) { [Environment]::SetEnvironmentVariable($v, $FoldersBefore[$v], "Process") }
}
# The windowless launcher, beside the program when it was built (--features windows-launcher):
# with it, the Start menu entry starts a copy of it in a folder of JETCARP's own.
$Launcher = Join-Path (Split-Path -Parent $Program) "jetcarpw.exe"
$WithLauncher = Test-Path -LiteralPath $Launcher
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
    if ($WithLauncher) { Copy-Item -Path $Launcher -Destination (Join-Path $Build "jetcarpw.exe") }
    $Zip = Join-Path $Rel "$Name.zip"
    Compress-Archive -Path $Build -DestinationPath $Zip
    $Hash = (Get-FileHash -Algorithm SHA256 -Path $Zip).Hash.ToLowerInvariant()
    [System.IO.File]::WriteAllText((Join-Path $Rel "SHA256SUMS"), "$Hash  $Name.zip`n")
    return $Rel
}

# Run the installer from release folder $Rel into a fresh scratch folder, $Work\home-$Case, with
# APPDATA (where the Start menu is) and LOCALAPPDATA (where the launcher's copy goes) inside it,
# and any further options. Its output is shown and kept in $script:Out; its exit code is returned.
function Invoke-Installer([string]$Rel, [string]$Case, [string[]]$More = @()) {
    $Home_ = Join-Path $Work "home-$Case"
    New-Item -ItemType Directory -Force -Path $Home_ | Out-Null
    $ErrorActionPreference = "Continue"
    Use-Home $Home_
    $script:Out = powershell -NoProfile -ExecutionPolicy Bypass -File $Installer -Version $Version `
        -InstallDir (Join-Path $Home_ "lib\jetcarp") -DownloadUrl $Rel -NoModifyPath -NoPacks @More 2>&1 | Out-String
    $Code = $LASTEXITCODE
    Restore-Env
    $ErrorActionPreference = "Stop"
    $script:Out -split "`r?`n" | Where-Object { $_ } | ForEach-Object { Write-Host "    | $_" }
    return $Code
}

# Where case $Case's Start menu entry goes.
function Get-Shortcut([string]$Case) {
    return Join-Path $Work "home-$Case\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\JETCARP.lnk"
}

# Where case $Case's copy of the launcher, which the entry starts, goes.
function Get-StableCopy([string]$Case) {
    return Join-Path $Work "home-$Case\AppData\Local\jetcarp\launcher"
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
    if (Test-Path -LiteralPath (Get-Shortcut "intact")) {
        Ok "intact: JETCARP is in the Start menu"
    } else {
        Bad "intact: no Start menu entry at $(Get-Shortcut 'intact')"
    }
    if ($WithLauncher) {
        $Record = Join-Path (Get-StableCopy "intact") "jetcarp-program"
        if (-not (Test-Path -LiteralPath (Join-Path (Get-StableCopy "intact") "jetcarpw.exe"))) {
            Bad "intact: no copy of the launcher in $(Get-StableCopy 'intact')"
        } elseif (-not (Test-Path -LiteralPath $Record) -or (Get-Content -LiteralPath $Record -TotalCount 1).Trim() -notlike "*\lib\jetcarp\jetcarp.exe") {
            Bad "intact: the launcher's record does not name the installed program"
        } else {
            Ok "intact: the entry starts a copy of the launcher that names the installed program"
        }
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

    Write-Host "4. -Uninstall, after case 1"
    $Code = Invoke-Installer (Join-Path $Work "intact") "intact" @("-Uninstall")
    if ($Code -ne 0) {
        Bad "uninstall: the installer failed (exit code $Code)"
    } elseif (Test-Path -LiteralPath (Join-Path $Work "home-intact\lib\jetcarp")) {
        Bad "uninstall: the program's folder is still there"
    } elseif (Test-Path -LiteralPath (Get-Shortcut "intact")) {
        Bad "uninstall: the Start menu entry is still there"
    } elseif (Test-Path -LiteralPath (Get-StableCopy "intact")) {
        Bad "uninstall: the launcher's copy is still there"
    } else {
        Ok "uninstall: the Start menu entry, the launcher's copy and the program's folder removed"
    }

    Write-Host "5. another JETCARP: Scoop's"
    $Scoop = Join-Path $Work "home-other\scoop"
    New-Item -ItemType Directory -Force -Path (Join-Path $Scoop "apps\jetcarp\current"), (Join-Path $Scoop "shims") | Out-Null
    foreach ($f in "apps\jetcarp\current\jetcarp.exe", "shims\jetcarp.exe") {
        [System.IO.File]::WriteAllText((Join-Path $Scoop $f), "a stand-in for Scoop's JETCARP")
    }
    [System.IO.File]::WriteAllText((Join-Path $Scoop "shims\jetcarp.shim"), "path = `"$(Join-Path $Scoop 'apps\jetcarp\current\jetcarp.exe')`"`n")
    $env:SCOOP = $Scoop
    $ProcessPath = $env:Path
    $env:Path = "$(Join-Path $Scoop 'shims');$env:Path"
    try {
        $Code = Invoke-Installer (Join-Path $Work "intact") "other"
    } finally {
        $env:Path = $ProcessPath
        Remove-Item -Path "Env:SCOOP" -ErrorAction SilentlyContinue
    }
    $Said = ($script:Out -split "`r?`n") | Where-Object { $_ -match "Scoop's" }
    if ($Code -ne 0) {
        Bad "another: the installer failed (exit code $Code)"
    } elseif ($script:Out -notmatch "(another JETCARP is|other JETCARPs are) installed on this computer as well") {
        Bad "another: Scoop's JETCARP is not named"
    } elseif (@($Said).Count -ne 1 -or ($Said -notmatch [regex]::Escape("(Scoop's): scoop uninstall jetcarp"))) {
        Bad "another: Scoop's JETCARP is not named once, with what removes it"
    } else {
        Ok "another: Scoop's JETCARP named once, with what removes it"
    }

    Write-Host "6. -Uninstall after ``jetcarp uninstall --settings --data``"
    $Code = Invoke-Installer (Join-Path $Work "intact") "gone"
    $Home_ = Join-Path $Work "home-gone"
    $Settings = Join-Path $Home_ "AppData\Roaming\jetcarp"
    $Data = Join-Path $Home_ "AppData\Local\jetcarp"
    $Exe = Join-Path $Home_ "lib\jetcarp\jetcarp.exe"
    if ($Code -ne 0) {
        Bad "gone: the installer failed (exit code $Code)"
    } else {
        Use-Home $Home_
        $ErrorActionPreference = "Continue"
        try {
            # `jetcarp uninstall --settings --data --yes`, when all it would undo is the Start menu
            # entry and what is in the scratch folder: elsewhere it would undo the rest in this
            # computer's own settings, as starting at login. Else its folders are deleted here, as
            # it would.
            $Plan = (& $Exe --json uninstall --settings --data 2>$null | Out-String | ConvertFrom-Json)
            $Steps = @($Plan.steps | ForEach-Object { [string]$_.what })
            $Outside = @($Steps | Where-Object { -not $_.Contains("Start menu") -and -not $_.Contains($Home_) })
            if ($LASTEXITCODE -eq 0 -and $Outside.Count -eq 0) {
                & $Exe uninstall --settings --data --yes 2>&1 | ForEach-Object { Write-Host "    | $_" }
            } else {
                Write-Host "    (not run here: it would also $($Outside -join '; '))"
                & $Exe launcher remove 2>&1 | Out-Null
                Remove-Item -Recurse -Force -LiteralPath $Settings, $Data -ErrorAction SilentlyContinue
            }
        } finally {
            $ErrorActionPreference = "Stop"
            Restore-Env
        }
        if ((Test-Path -LiteralPath $Settings) -or (Test-Path -LiteralPath $Data)) {
            Bad "gone: ``jetcarp uninstall --settings --data`` left $Settings or $Data"
        } else {
            # A settings folder of empty folders, as a program run since could leave: not named.
            New-Item -ItemType Directory -Force -Path (Join-Path $Settings "tune\screen-1") | Out-Null
            $Code = Invoke-Installer (Join-Path $Work "intact") "gone" @("-Uninstall")
            $Files = @(Get-ChildItem -LiteralPath $Settings -Recurse -Force -File -ErrorAction SilentlyContinue)
            if ($Code -ne 0) {
                Bad "gone: -Uninstall failed (exit code $Code)"
            } elseif (Test-Path -LiteralPath $Data) {
                Bad "gone: -Uninstall made $Data again: $(@(Get-ChildItem -LiteralPath $Data -Recurse -Force).FullName -join ', ')"
            } elseif ($Files.Count -gt 0) {
                Bad "gone: -Uninstall wrote in $Settings`: $($Files.FullName -join ', ')"
            } elseif ($script:Out -match [regex]::Escape($Settings) -or $script:Out -match [regex]::Escape($Data)) {
                Bad "gone: -Uninstall names a folder that is gone, or holds nothing, as kept"
            } else {
                Ok "gone: -Uninstall made no folder of JETCARP's again, and named none that holds nothing"
            }
        }
    }

    if ((Get-UserPath) -ne $PathBefore) { Bad "the user PATH changed, with -NoModifyPath" }
} finally {
    Remove-Item -Recurse -Force $Work -ErrorAction SilentlyContinue
}

if ($script:Failures -gt 0) {
    Write-Host "$($script:Failures) check(s) failed"
    exit 1
}
Write-Host "all six cases passed"
exit 0
