<#
.SYNOPSIS
JETCARP @VERSION@ installer for Windows.

.DESCRIPTION
Downloads JETCARP's release archive and the release's SHA256SUMS, checks the archive against it,
unpacks it into one folder (by default %LOCALAPPDATA%\Programs\jetcarp), and adds that folder to
your user PATH. Then the program adds JETCARP, with its icon, to your Start menu
(`jetcarp launcher install`). It changes nothing else, and needs no administrator rights.

  Invoke-WebRequest https://github.com/@REPO@/releases/latest/download/jetcarp-installer.ps1 -OutFile "$env:TEMP\jetcarp-installer.ps1"
  powershell -ExecutionPolicy Bypass -File "$env:TEMP\jetcarp-installer.ps1"

Save it and run it as a file, as above: Microsoft Defender can flag the one-line form that pipes a
downloaded script into iex. It takes the options below, or the environment variables named with
each (for example $env:JETCARP_NO_PACKS = "1" first).

.PARAMETER Version
Install release Version instead of @VERSION@, such as 0.4.0, v0.4.0 or 0.4.0-rc.2
(JETCARP_VERSION). Pre-releases are never "latest", so name them.

.PARAMETER InstallDir
The program's folder (JETCARP_INSTALL_DIR). Default: %LOCALAPPDATA%\Programs\jetcarp.

.PARAMETER NoModifyPath
Do not add the folder to the user PATH (JETCARP_NO_MODIFY_PATH=1).

.PARAMETER NoPacks
Leave out the starter pack (JETCARP_NO_PACKS=1).

.PARAMETER NoLauncher
Add no JETCARP entry to the Start menu (JETCARP_NO_LAUNCHER=1).

.PARAMETER DownloadUrl
Where to get the release files: a URL, or a local folder holding them (JETCARP_DOWNLOAD_URL).
Default: https://github.com/@REPO@/releases/download/v<version>

.PARAMETER UseGh
Download with the GitHub CLI, gh, signed in: for a private repository (JETCARP_USE_GH=1).

.PARAMETER Uninstall
Remove the folder this installer made and its Start menu entry, and take it off the user PATH.
#>
param(
    [string]$Version = $env:JETCARP_VERSION,
    [string]$InstallDir = $env:JETCARP_INSTALL_DIR,
    [switch]$NoModifyPath = ($env:JETCARP_NO_MODIFY_PATH -eq "1"),
    [switch]$NoPacks = ($env:JETCARP_NO_PACKS -eq "1"),
    [switch]$NoLauncher = ($env:JETCARP_NO_LAUNCHER -eq "1"),
    [string]$DownloadUrl = $env:JETCARP_DOWNLOAD_URL,
    [switch]$UseGh = ($env:JETCARP_USE_GH -eq "1"),
    [switch]$Uninstall
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
$Repo = "@REPO@"

function Say([string]$Message) { Write-Host "jetcarp-installer: $Message" }
# A refusal is one line and exit code 1, not PowerShell's error block (#117). The script runs as a
# file, so exit ends only the script; it still runs the finally that removes the temporary
# folder, and no catch takes it.
function Fail([string]$Message) {
    Write-Host "jetcarp-installer: $Message" -ForegroundColor Red
    exit 1
}

if (-not $Version) { $Version = "@VERSION@" }
$Version = $Version.TrimStart("v")
if ($Version -notmatch '^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$') {
    Fail "$Version is not a JETCARP version (such as 0.4.0 or 0.4.0-rc.2)"
}
$Target = "x86_64-pc-windows-msvc"
$Archive = "jetcarp-$Version-$Target.zip"

if (-not $InstallDir) { $InstallDir = Join-Path $env:LOCALAPPDATA "Programs\jetcarp" }
if (-not $DownloadUrl) { $DownloadUrl = "https://github.com/$Repo/releases/download/v$Version" }
$DownloadUrl = $DownloadUrl.TrimEnd("/").TrimEnd("\")
$InstallDir = [System.IO.Path]::GetFullPath($InstallDir)
$Marker = Join-Path $InstallDir ".jetcarp-installer"

# Never replace or delete a folder this installer did not make.
if ((Test-Path $InstallDir) -and -not (Test-Path $Marker)) {
    if (@(Get-ChildItem -Force $InstallDir).Count -gt 0) {
        Fail "$InstallDir exists and was not made by this installer; choose another with -InstallDir"
    }
}

# The user PATH as Windows stores it: unexpanded, so entries such as %PNPM_HOME%\bin stay as
# they are. ([Environment]::GetEnvironmentVariable expands them, and SetEnvironmentVariable writes
# a plain string, which would turn every %VAR% entry into a fixed path.)
function Get-UserPath {
    $key = Get-Item -Path "HKCU:\Environment" -ErrorAction SilentlyContinue
    if (-not $key) { return "" }
    $value = $key.GetValue("Path", "", [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    if ($null -eq $value) { return "" }
    return [string]$value
}

# Store the user PATH back as an expandable string, as Windows itself does, and tell running
# programs (Explorer, and the terminals it starts) that the environment changed.
function Set-UserPath([string]$Value) {
    if (-not (Test-Path "HKCU:\Environment")) { New-Item -Path "HKCU:\Environment" | Out-Null }
    Set-ItemProperty -Path "HKCU:\Environment" -Name "Path" -Value $Value -Type ExpandString
    # Setting a user variable through .NET broadcasts WM_SETTINGCHANGE; this one is set and
    # removed at once, only for that broadcast.
    [Environment]::SetEnvironmentVariable("JETCARP_INSTALLER_REFRESH", "1", "User")
    [Environment]::SetEnvironmentVariable("JETCARP_INSTALLER_REFRESH", $null, "User")
}

function Remove-FromUserPath([string]$Dir) {
    $parts = @((Get-UserPath) -split ";" | Where-Object { $_ -and ($_.TrimEnd("\") -ne $Dir.TrimEnd("\")) })
    Set-UserPath ($parts -join ";")
}

# The folders `New-Item -Force` would make for $Dir, deepest first.
function Get-MissingDirs([string]$Dir) {
    $out = @()
    $d = $Dir
    while ($d -and -not (Test-Path -LiteralPath $d)) {
        $out += $d
        $d = Split-Path -Parent $d
    }
    return $out
}

# Runs JETCARP's program with these arguments, and returns whether it succeeded and what it said,
# as text. Windows PowerShell stops at a program's standard error under "Stop", so not here; a
# version without the command asked for (before 0.4.3) only fails.
function Invoke-Program([string]$Program, [string[]]$Arguments) {
    $Prev = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        $Said = (& $Program @Arguments 2>&1 | ForEach-Object { "$_" }) -join "`n"
        return [pscustomobject]@{ Ok = ($LASTEXITCODE -eq 0); Said = $Said.Trim() }
    } catch {
        return [pscustomobject]@{ Ok = $false; Said = $_.Exception.Message }
    } finally {
        $ErrorActionPreference = $Prev
    }
}

# The folders an earlier install recorded as its own (`made` lines in the marker).
function Get-MadeBefore {
    if (-not (Test-Path -LiteralPath $Marker)) { return @() }
    return @(Get-Content -LiteralPath $Marker | Where-Object { $_ -like "made *" } | ForEach-Object { $_.Substring(5) })
}

if ($Uninstall) {
    if (-not (Test-Path $Marker)) { Fail "no JETCARP installed by this installer in $InstallDir" }
    if (Test-Path (Join-Path $InstallDir "jetcarp.exe")) {
        Say "removing the program; what JETCARP set up outside its folders (the host, starting at login, the screen saver) is undone by ``jetcarp uninstall``, not by this: if that is still to do, install again and run it"
    }
    # The Start menu entry the install asked the program for, taken out while the program is
    # still there.
    $Program = Join-Path $InstallDir "jetcarp.exe"
    if ((Test-Path $Program) -and (Invoke-Program $Program @("launcher", "status")).Ok) {
        $R = Invoke-Program $Program @("launcher", "remove")
        if ($R.Ok) {
            foreach ($l in ($R.Said -split "`r?`n")) { if ($l) { Say $l.Trim() } }
        } else {
            Say "could not remove JETCARP's Start menu entry: $($R.Said)"
        }
    }
    $Made = @(Get-MadeBefore)
    Remove-Item -Recurse -Force $InstallDir
    # Said only when the folder was on the PATH: after -NoModifyPath it never was (#128).
    $WasOnPath = @((Get-UserPath) -split ";" | Where-Object { $_ -and ($_.TrimEnd("\") -eq $InstallDir.TrimEnd("\")) }).Count -gt 0
    Remove-FromUserPath $InstallDir
    if ($WasOnPath) {
        Say "removed $InstallDir and took it off your PATH"
    } else {
        Say "removed $InstallDir (it wasn't on your PATH)"
    }
    # The folders the install made around it (such as %LOCALAPPDATA%\Programs), deepest first, if empty.
    foreach ($m in @($Made | Sort-Object -Unique -Descending)) {
        if ((Test-Path -LiteralPath $m -PathType Container) -and @(Get-ChildItem -Force -LiteralPath $m).Count -eq 0) {
            Remove-Item -LiteralPath $m -Force
            Say "removed $m (empty, made by the install)"
        }
    }
    # Named here, as `jetcarp paths` would have, since the program has gone: only the folders still
    # there, each with what it holds, so nothing is said of what `jetcarp uninstall --settings --data`
    # already took (#134, #142).
    $Pictures = Join-Path ([Environment]::GetFolderPath("MyPictures")) "JETCARP"
    $Kept = @(
        [pscustomobject]@{ Folder = (Join-Path $env:APPDATA 'jetcarp'); Holds = 'your settings' },
        [pscustomobject]@{ Folder = (Join-Path $env:LOCALAPPDATA 'jetcarp'); Holds = 'your library and data' },
        [pscustomobject]@{ Folder = $Pictures; Holds = 'your pictures and exports' }
    ) | Where-Object { Test-Path -LiteralPath $_.Folder }
    if (@($Kept).Count -gt 0) {
        Say "kept these folders; delete them by hand if you want them gone:"
        foreach ($k in @($Kept)) { Say "  $($k.Folder) ($($k.Holds))" }
    }
    return
}

if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64") {
    Say "this is an ARM computer: JETCARP's x86_64 build runs under Windows' emulation"
} elseif ($env:PROCESSOR_ARCHITECTURE -ne "AMD64") {
    Fail "no JETCARP build for $($env:PROCESSOR_ARCHITECTURE) processors"
}

# PowerShell 5.1 may still default to older TLS versions.
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$Tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("jetcarp-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $Tmp | Out-Null
function Get-ReleaseFile([string]$Name) {
    $Out = Join-Path $Tmp $Name
    try {
        if ($UseGh) {
            if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
                Fail "-UseGh needs the GitHub CLI (https://cli.github.com), signed in"
            }
            & gh release download "v$Version" --repo $Repo --pattern $Name --dir $Tmp --clobber
            if ($LASTEXITCODE -ne 0) { throw "gh exited with $LASTEXITCODE" }
        } elseif (Test-Path -PathType Container $DownloadUrl) {
            Copy-Item -Path (Join-Path $DownloadUrl $Name) -Destination $Out
        } else {
            Invoke-WebRequest -UseBasicParsing -Uri "$DownloadUrl/$Name" -OutFile $Out
        }
    } catch {
        $Hint = ""
        if ($Version -like "*-*") { $Hint = " (a pre-release: is v$Version published? If the repository is private, try -UseGh)" }
        Fail "could not get $Name from $From$Hint`: $($_.Exception.Message)"
    }
    return $Out
}

try {
    $From = if ($UseGh) { "$Repo's release v$Version, with gh" } else { $DownloadUrl }
    Say "getting JETCARP $Version for $Target from $From"
    $Sums = Get-ReleaseFile "SHA256SUMS"
    $Zip = Get-ReleaseFile $Archive

    $Want = $null
    foreach ($line in Get-Content $Sums) {
        $f = $line -split "\s+", 2
        if ($f.Count -eq 2 -and ($f[1].TrimStart("*") -eq $Archive)) { $Want = $f[0].ToLowerInvariant() }
    }
    if (-not $Want) { Fail "SHA256SUMS does not list $Archive" }
    $Got = (Get-FileHash -Algorithm SHA256 -Path $Zip).Hash.ToLowerInvariant()
    if ($Want -ne $Got) { Fail "$Archive does not match SHA256SUMS (expected $Want, got $Got)" }
    Say "checked $Archive against SHA256SUMS"

    $Unpacked = Join-Path $Tmp "unpacked"
    Expand-Archive -Path $Zip -DestinationPath $Unpacked -Force
    $Src = Join-Path $Unpacked "jetcarp-$Version-$Target"
    if (-not (Test-Path (Join-Path $Src "jetcarp.exe"))) { Fail "$Archive has no jetcarp.exe" }
    if ($NoPacks -and (Test-Path (Join-Path $Src "packs"))) { Remove-Item -Recurse -Force (Join-Path $Src "packs") }
    # Swap the new folder in, keeping the old one until the new one is in place. A running
    # jetcarp.exe holds its folder: stop it first (`jetcarp host stop`, and close the app).
    $Parent = Split-Path -Parent $InstallDir
    # The marker names the version, and every folder an install made, for -Uninstall to remove
    # when empty (an update keeps the ones the first install made).
    $Made = @(@(Get-MissingDirs $Parent) + @(Get-MadeBefore) | Sort-Object -Unique -Descending)
    $Lines = @("JETCARP $Version $Target") + @($Made | ForEach-Object { "made $_" })
    Set-Content -Path (Join-Path $Src ".jetcarp-installer") -Value $Lines
    New-Item -ItemType Directory -Force -Path $Parent | Out-Null
    $Old = "$InstallDir.old.$PID"
    if (Test-Path $InstallDir) {
        try { Move-Item -Path $InstallDir -Destination $Old }
        catch { Fail "could not replace $InstallDir; is JETCARP running? Close it (and run ``jetcarp host stop``), then try again" }
    }
    Move-Item -Path $Src -Destination $InstallDir
    if (Test-Path $Old) { Remove-Item -Recurse -Force $Old }
} finally {
    Remove-Item -Recurse -Force $Tmp -ErrorAction SilentlyContinue
}

$Exe = Join-Path $InstallDir "jetcarp.exe"
$Shown = & $Exe --version
Say "installed $Shown in $InstallDir"
if (Test-Path (Join-Path $InstallDir "packs")) { Say "the starter pack is in $(Join-Path $InstallDir 'packs')" }
# JETCARP in the Start menu, with its icon (#105). A version without the `launcher` command
# (before 0.4.3) is left without one, and nothing is said.
if (-not $NoLauncher -and (Invoke-Program $Exe @("launcher", "status")).Ok) {
    $R = Invoke-Program $Exe @("launcher", "install")
    if ($R.Ok) {
        Say $R.Said
    } else {
        Say "could not add JETCARP to the Start menu (``jetcarp launcher install`` tries again): $($R.Said)"
    }
}

if (-not $NoModifyPath) {
    $UserPath = Get-UserPath
    $Has = @($UserPath -split ";" | Where-Object { $_.TrimEnd("\") -eq $InstallDir.TrimEnd("\") }).Count -gt 0
    if (-not $Has) {
        # A PATH that ends in ";" would otherwise gain an empty entry.
        $Trimmed = $UserPath.TrimEnd(";")
        $New = if ($Trimmed) { "$Trimmed;$InstallDir" } else { $InstallDir }
        Set-UserPath $New
        Say "added $InstallDir to your user PATH"
    }
    if (-not (($env:Path -split ";") -contains $InstallDir)) { $env:Path = "$env:Path;$InstallDir" }
    Say "``jetcarp doctor`` shows what JETCARP sees; jetcarpw.exe starts it with no console window"
    # Last, on its own: this window can't see the new PATH, since the installer runs in a PowerShell
    # of its own, so `jetcarp` typed here next is not found (#144).
    Say "Open a new PowerShell window, then run ``jetcarp``"
} else {
    Say "``jetcarp doctor`` shows what JETCARP sees; jetcarpw.exe starts it with no console window"
    Say "left your PATH alone: run JETCARP as $Exe"
}
