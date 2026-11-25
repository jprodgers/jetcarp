<#
.SYNOPSIS
JETCARP's field test for Windows: test a JETCARP release on this computer, and get one report
file to send by hand to whoever asked you to.

.DESCRIPTION
In PowerShell, save it and run it as a file:

  Invoke-WebRequest https://raw.githubusercontent.com/jprodgers/jetcarp/main/testing/field/jetcarp-field-test.ps1 -OutFile "$env:TEMP\jetcarp-field-test.ps1"
  powershell -ExecutionPolicy Bypass -File "$env:TEMP\jetcarp-field-test.ps1"

(Microsoft Defender can flag the one-line form that pipes a downloaded script into iex; run as a
file, the same script raises no alert.)

It asks which release to test (the newest, release candidates included, unless you choose
another) and for how long (Quick, a few minutes; Suite, 10 to 20; Extended, 30 to 60). It
downloads that release and its SHA256SUMS from GitHub, checks the archive against them as
JETCARP's installer does, unpacks it into a temporary folder and runs it from there, with
JETCARP's settings, data and output folders inside that folder too: your own JETCARP, its
settings and its library are never touched. Every test is a command-line run, each started with
no window: no app window, no background host, nothing drawn on the screen, so nothing can flash.
The tests themselves are in tests.json beside this script (or, run on its own, from the JETCARP
repository).

At the end it writes the report (JSON, with a short summary at the top), shows you what is in it
and where it is, and asks whether to remove everything it added (the default) or keep this
version, installed by JETCARP's own installer. Nothing is uploaded: JETCARP sends nothing
anywhere, and neither does this script. It only downloads: the list of releases, the release and
the test data. Ctrl-C stops the running test, cleans up and still writes a report, marked as
stopped. Windows PowerShell 5.1 and PowerShell 7 both run it.

.PARAMETER Release
Test this release (0.4.3-rc.2, v0.4.2, latest) instead of choosing one.

.PARAMETER Tier
quick (a few minutes), suite (10 to 20) or extended (30 to 60), instead of choosing.

.PARAMETER Yes
Ask nothing: the newest release, the quick tier, and remove everything at the end, unless the
other options say otherwise.

.PARAMETER Keep
At the end, install the tested version with JETCARP's installer.

.PARAMETER Remove
At the end, remove everything this script added (the default).

.PARAMETER Report
Write the report here (default: jetcarp-field-report-....json in this folder, or in your home
folder if this one can't be written).

.PARAMETER Data
Where tests.json and the test data are: a folder or a URL (default: beside this script, else the
JETCARP repository on GitHub).
#>
param(
    [string]$Release = "",
    [ValidateSet("", "quick", "suite", "extended")]
    [string]$Tier = "",
    [switch]$Yes,
    [switch]$Keep,
    [switch]$Remove,
    [string]$Report = "",
    [string]$Data = ""
)

$ErrorActionPreference = "Stop"
$FtVersion = 1
$Repo = "jprodgers/jetcarp"
$DataUrl = "https://raw.githubusercontent.com/$Repo/main/testing/field"
$ApiUrl = "https://api.github.com/repos/$Repo/releases"
$Inv = [Globalization.CultureInfo]::InvariantCulture
# A debugging aid: keep the temporary folder at the end.
$KeepWork = [bool]$env:FIELD_TEST_KEEP_WORK

# ---- How it looks ---------------------------------------------------------------------------

$Live = $false
try { $Live = -not [Console]::IsOutputRedirected } catch { }
$script:AskOk = $Live -and -not $Yes
try { if ([Console]::IsInputRedirected) { $script:AskOk = $false } } catch { $script:AskOk = $false }
$UseColor = $Live -and -not $env:NO_COLOR
$CodePage = 0
try { $CodePage = [Console]::OutputEncoding.CodePage } catch { }
# Block characters where the console's code page has them (the old console's OEM pages do).
if (@(65001, 437, 850, 852, 857, 858) -contains $CodePage) {
    $Full = [string][char]0x2588; $Empty = [string][char]0x2591
} else {
    $Full = "#"; $Empty = "-"
}
$Spin = @("|", "/", "-", "\")
$script:TickN = 0
$script:StatusTop = -1

function Get-Width {
    try { $w = [Console]::WindowWidth; if ($w -lt 40) { return 40 }; return $w } catch { return 80 }
}

function Write-Part([string]$Text, $Color) {
    if ($UseColor -and $Color) {
        $old = [Console]::ForegroundColor
        [Console]::ForegroundColor = $Color
        [Console]::Write($Text)
        [Console]::ForegroundColor = $old
    } else {
        [Console]::Write($Text)
    }
}

# The two live lines (this test, and the whole run) are taken off the screen before anything
# else is written.
function Clear-Status {
    if ($script:StatusTop -lt 0) { return }
    try {
        $w = (Get-Width) - 1
        [Console]::SetCursorPosition(0, $script:StatusTop)
        [Console]::Write(" " * $w)
        [Console]::SetCursorPosition(0, $script:StatusTop + 1)
        [Console]::Write(" " * $w)
        [Console]::SetCursorPosition(0, $script:StatusTop)
    } catch { }
    $script:StatusTop = -1
}

function Get-Fit([string]$s, [int]$m) {
    if ($m -lt 4) { $m = 4 }
    if ($s.Length -gt $m) { return $s.Substring(0, $m - 3) + "..." }
    return $s
}
function Get-Bar([int]$f, [int]$m) {
    $k = [int][Math]::Floor($f * $m / 1000.0 + 0.5)
    if ($k -gt $m) { $k = $m }; if ($k -lt 0) { $k = 0 }
    return ($Full * $k) + ($Empty * ($m - $k))
}

# Show-Status LABEL TEST_PERMILLE TEXT ALL_PERMILLE ALL_TEXT: the two live lines, a permille
# below 0 drawing no bar.
function Show-Status([string]$Label, [int]$Pm, [string]$Count, [int]$All, [string]$AllText) {
    if (-not $Live) { return }
    $script:TickN++
    $w = (Get-Width) - 1
    $bw = 12; if ($w -ge 72) { $bw = 24 }
    $avail = $w - 4 - 2; if ($Pm -ge 0) { $avail = $w - 4 - $bw - 2 }
    $ll = [Math]::Min($Label.Length, 24)
    if ($Count.Length -gt $avail - $ll) { $Count = Get-Fit $Count ($avail - $ll) }
    $Label = Get-Fit $Label ($avail - $Count.Length)
    $sp = $Spin[$script:TickN % $Spin.Count]
    try {
        if ($script:StatusTop -ge 0) { [Console]::SetCursorPosition(0, $script:StatusTop) }
        Write-Part "  " $null; Write-Part $sp "Cyan"; Write-Part " $Label" $null
        $n = 4 + $Label.Length
        if ($Pm -ge 0) {
            Write-Part " " $null; Write-Part (Get-Bar $Pm $bw) "Cyan"; Write-Part " $Count" $null
            $n += 2 + $bw + $Count.Length
        } elseif ($Count) {
            Write-Part "  $Count" $null; $n += 2 + $Count.Length
        }
        if ($n -lt $w) { [Console]::Write(" " * ($w - $n)) }
        [Console]::Write("`n")
        $n = 0
        if ($All -ge 0) {
            $at = Get-Fit $AllText ($w - 10 - $bw)
            Write-Part "    all  " $null; Write-Part (Get-Bar $All $bw) "Cyan"; Write-Part " $at" $null
            $n = 10 + $bw + $at.Length
        }
        if ($n -lt $w) { [Console]::Write(" " * ($w - $n)) }
        $script:StatusTop = [Console]::CursorTop - 1
        [Console]::SetCursorPosition(0, $script:StatusTop)
    } catch { }
}

function Say([string]$t) { Clear-Status; Write-Host $t }
function Info([string]$t) { Clear-Status; Write-Host "  $t" }
function Dim([string]$t) { Clear-Status; if ($UseColor) { Write-Host "  $t" -ForegroundColor DarkGray } else { Write-Host "  $t" } }
function Warn([string]$t) { Clear-Status; if ($UseColor) { Write-Host "  $t" -ForegroundColor Yellow } else { Write-Host "  $t" } }
function Heading([string]$t) { Clear-Status; Write-Host ""; if ($UseColor) { Write-Host $t -ForegroundColor Cyan } else { Write-Host $t } }
function Stop-Field([string]$t) {
    Clear-Status
    if ($UseColor) { Write-Host "jetcarp-field-test: $t" -ForegroundColor Red } else { Write-Host "jetcarp-field-test: $t" }
    throw [System.OperationCanceledException]::new("field test: $t")
}
function Get-PlainTag([string]$r) {
    switch ($r) { "pass" { "[ ok ]" } "warn" { "[warn]" } "fail" { "[FAIL]" } "stop" { "[stop]" } default { "[skip]" } }
}
function Write-Tag([string]$r) {
    $c = switch ($r) { "pass" { "Green" } "warn" { "Yellow" } "fail" { "Red" } "stop" { "Yellow" } default { "DarkGray" } }
    if ($UseColor) { Write-Host -NoNewline (Get-PlainTag $r) -ForegroundColor $c } else { Write-Host -NoNewline (Get-PlainTag $r) }
}
function Write-Result([string]$Indent, [string]$r, [string]$Text, [string]$After) {
    Clear-Status
    Write-Host -NoNewline $Indent; Write-Tag $r; Write-Host -NoNewline " $Text"
    if ($After) { if ($UseColor) { Write-Host " $After" -ForegroundColor DarkGray } else { Write-Host " $After" } } else { Write-Host "" }
}

# Ask PROMPT DEFAULT: the line typed (Enter: DEFAULT); DEFAULT with no one to ask.
function Ask([string]$Prompt, [string]$Default) {
    if (-not $script:AskOk) { return $Default }
    Clear-Status
    if ($UseColor) { Write-Host -NoNewline "$Prompt " -ForegroundColor White } else { Write-Host -NoNewline "$Prompt " }
    $a = [Console]::ReadLine()
    if ($null -eq $a) { $script:AskOk = $false; Write-Host ""; return $Default }
    if ($a -eq "") { return $Default }
    return $a.Trim()
}

# ---- Small helpers --------------------------------------------------------------------------

function Get-Commas($n) { return [string]::Format($Inv, "{0:N0}", [double]$n) }
function Get-Dur($s) {
    $s = [int][Math]::Floor([double]$s + 0.5)
    if ($s -lt 60) { return "$s s" }
    if ($s -lt 3600) { return [string]::Format($Inv, "{0} min {1:D2} s", [Math]::Floor($s / 60), $s % 60) }
    return [string]::Format($Inv, "{0} h {1:D2} min", [Math]::Floor($s / 3600), [Math]::Floor(($s % 3600) / 60))
}
function Get-Left($s) {
    if ($s -lt 45) { return "under a minute left" }
    if ($s -lt 5400) { return "about $([int][Math]::Floor($s / 60 + 0.5)) min left" }
    return [string]::Format($Inv, "about {0:0.0} h left", $s / 3600)
}
function Get-Human($b) {
    $b = [double]$b
    if ($b -ge 1GB) { return [string]::Format($Inv, "{0:0.0} GB", $b / 1GB) }
    if ($b -ge 1MB) { return [string]::Format($Inv, "{0:0.0} MB", $b / 1MB) }
    if ($b -ge 1KB) { return [string]::Format($Inv, "{0:0} KB", $b / 1KB) }
    return "$b bytes"
}
function Get-Tilde([string]$p) {
    $h = [Environment]::GetFolderPath("UserProfile")
    if ($h -and $p.StartsWith($h, [StringComparison]::OrdinalIgnoreCase)) { return "~" + $p.Substring($h.Length) }
    return $p
}
# A key that sorts releases in order, a release candidate before its release.
function Get-VKey([string]$v) {
    $v = $v.TrimStart("v")
    if ($v -notmatch '^(\d+)\.(\d+)\.(\d+)(?:-(rc)\.(\d+)|-(.+))?$') { return "00000000000000000000" }
    $p = 99999
    if ($Matches[4] -eq "rc") { $p = [int]$Matches[5] } elseif ($Matches[6]) { $p = 0 }
    return "{0:D5}{1:D5}{2:D5}{3:D5}" -f [int]$Matches[1], [int]$Matches[2], [int]$Matches[3], $p
}
function Test-InRange([string]$v, [string]$from, [string]$to) {
    $k = Get-VKey $v
    if ($from -and [string]::CompareOrdinal($k, (Get-VKey $from)) -lt 0) { return $false }
    if ($to -and [string]::CompareOrdinal($k, (Get-VKey $to)) -gt 0) { return $false }
    return $true
}
# The entry of a list whose from..to holds the release being tested (the latest from).
function Select-Range($list) {
    $best = $null; $bk = ""
    foreach ($e in @($list)) {
        if ($null -eq $e) { continue }
        if (Test-InRange $script:V ([string]$e.from) ([string]$e.to)) {
            $k = Get-VKey ([string]$e.from)
            if ($null -eq $best -or [string]::CompareOrdinal($k, $bk) -gt 0) { $best = $e; $bk = $k }
        }
    }
    return $best
}
# A value at a path such as "progress.by_class.still life" or "progress.groups[0].settled".
function Get-JPath($o, [string]$path) {
    foreach ($m in [regex]::Matches($path, '([^.\[\]]+)|\[(\d+)\]')) {
        if ($null -eq $o) { return $null }
        if ($m.Groups[2].Success) {
            $i = [int]$m.Groups[2].Value
            $a = @($o); if ($i -ge $a.Count) { return $null }; $o = $a[$i]
        } else {
            $p = $o.PSObject.Properties[$m.Groups[1].Value]
            if ($null -eq $p) { return $null }; $o = $p.Value
        }
    }
    return $o
}
function ConvertFrom-JsonFile([string]$f) {
    try {
        $t = [IO.File]::ReadAllText($f, [Text.Encoding]::UTF8)
        if (-not $t.Trim()) { return $null }
        return ($t | ConvertFrom-Json)
    } catch { return $null }
}
function Get-DataPath($p) { return ([string]$p).Replace("/", [string][IO.Path]::DirectorySeparatorChar) }
function Get-Sha256([string]$f) { return (Get-FileHash -Algorithm SHA256 -Path $f).Hash.ToLowerInvariant() }
function Save-Url([string]$Url, [string]$File) {
    $wc = New-Object System.Net.WebClient
    $wc.Headers["User-Agent"] = "jetcarp-field-test"
    try { $wc.DownloadFile($Url, $File) } finally { $wc.Dispose() }
}
function Write-Utf8([string]$File, [string]$Text) {
    [IO.File]::WriteAllText($File, $Text, (New-Object System.Text.UTF8Encoding($false)))
}

# ---- Running JETCARP ------------------------------------------------------------------------

# A command line Windows splits back into these arguments.
function Join-Args([string[]]$Arguments) {
    $parts = foreach ($a in $Arguments) {
        if ($a -eq "") { '""'; continue }
        if ($a -notmatch '[\s"]') { $a; continue }
        $b = New-Object System.Text.StringBuilder; $bs = 0
        foreach ($ch in $a.ToCharArray()) {
            if ($ch -eq '\') { $bs++; continue }
            if ($ch -eq '"') { [void]$b.Append('\' * ($bs * 2 + 1)).Append('"'); $bs = 0; continue }
            [void]$b.Append('\' * $bs).Append($ch); $bs = 0
        }
        '"' + $b.ToString() + ('\' * ($bs * 2)) + '"'
    }
    return ($parts -join " ")
}

# The scratch JETCARP with these arguments, on the GPU named by GpuName and GpuApi, with no
# window of its own; its two streams read line by line as they come.
function Start-Jet([string[]]$Arguments, [string]$GpuName = "", [string]$GpuApi = "") {
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $script:Jet
    $psi.Arguments = Join-Args $Arguments
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true
    $psi.RedirectStandardInput = $true
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.StandardOutputEncoding = [Text.Encoding]::UTF8
    $psi.StandardErrorEncoding = [Text.Encoding]::UTF8
    $psi.WorkingDirectory = $script:Work
    foreach ($n in @("JETCARP_GPU", "WGPU_BACKEND")) { if ($psi.EnvironmentVariables.ContainsKey($n)) { $psi.EnvironmentVariables.Remove($n) } }
    if (-not $script:OnWindows) { foreach ($n in @("DISPLAY", "WAYLAND_DISPLAY", "WAYLAND_SOCKET")) { if ($psi.EnvironmentVariables.ContainsKey($n)) { $psi.EnvironmentVariables.Remove($n) } } }
    if ($GpuName) { $psi.EnvironmentVariables["JETCARP_GPU"] = $GpuName }
    if ($GpuApi) { $psi.EnvironmentVariables["WGPU_BACKEND"] = $GpuApi }
    $p = [System.Diagnostics.Process]::Start($psi)
    $p.StandardInput.Close()
    $j = [pscustomobject]@{
        P = $p; Out = (New-Object System.Collections.Generic.List[string]); Err = (New-Object System.Collections.Generic.List[string])
        OutTask = $p.StandardOutput.ReadLineAsync(); ErrTask = $p.StandardError.ReadLineAsync(); Rc = $null; TimedOut = $false; Window = $false
    }
    $script:Running = $j
    return $j
}
function Read-Jet($j) {
    while ($null -ne $j.OutTask -and $j.OutTask.IsCompleted) {
        $l = $j.OutTask.Result
        if ($null -eq $l) { $j.OutTask = $null } else { $j.Out.Add($l); $j.OutTask = $j.P.StandardOutput.ReadLineAsync() }
    }
    while ($null -ne $j.ErrTask -and $j.ErrTask.IsCompleted) {
        $l = $j.ErrTask.Result
        if ($null -eq $l) { $j.ErrTask = $null } else { $j.Err.Add($l); $j.ErrTask = $j.P.StandardError.ReadLineAsync() }
    }
}
# Wait-Jet JOB MODE ARG TIMEOUT: wait for it, drawing its progress (MODE: count WORD, the last
# "N/M WORD" on its error stream; runs, ARG seconds and the last "N runs"; time, ARG seconds and
# its last line; fit, host tune's measured sizes; none; quiet, nothing drawn). It is stopped past
# TIMEOUT seconds. With SampleRss set, its memory is read every 5 seconds. Returns its exit status.
function Wait-Jet($j, [string]$Mode = "none", $Arg = $null, [int]$Timeout = 0) {
    $sw = [Diagnostics.Stopwatch]::StartNew(); $lastPlain = 0; $lastRss = -5
    while (-not $j.P.WaitForExit(200)) {
        Read-Jet $j
        $el = $sw.Elapsed.TotalSeconds
        try { $j.P.Refresh(); if ($j.P.MainWindowHandle -ne [IntPtr]::Zero) { $j.Window = $true } } catch { }
        if ($script:SampleRss -and $el - $lastRss -ge 5) {
            $lastRss = $el
            try { $script:Rss.Add(@([int]$el, [Math]::Round($j.P.WorkingSet64 / 1MB, 1))); $script:RssNow = "$(Get-Human $j.P.WorkingSet64) in use" } catch { }
        }
        if ($Mode -ne "quiet") {
            $pm = -1; $tx = Get-Dur $el
            switch ($Mode) {
                "count" {
                    for ($i = $j.Err.Count - 1; $i -ge [Math]::Max(0, $j.Err.Count - 4); $i--) {
                        $ms = [regex]::Matches($j.Err[$i], "([0-9][0-9,]*)/([0-9][0-9,]*) $Arg")
                        if ($ms.Count -gt 0) {
                            $m = $ms[$ms.Count - 1]; $a = [double]($m.Groups[1].Value -replace ",", ""); $b = [double]($m.Groups[2].Value -replace ",", "")
                            if ($b -gt 0) { $pm = [int]($a * 1000 / $b); $tx = "$(Get-Commas $a) of $(Get-Commas $b) $Arg" }
                            break
                        }
                    }
                }
                "runs" {
                    $pm = [Math]::Min(999, [int]($el * 1000 / $Arg))
                    $r = ""
                    for ($i = $j.Err.Count - 1; $i -ge [Math]::Max(0, $j.Err.Count - 3); $i--) { if ($j.Err[$i] -match '([0-9][0-9,]* runs)') { $r = $Matches[1]; break } }
                    $tx = "$(Get-Dur $el) of $(Get-Dur $Arg)"; if ($r) { $tx += ", $r" }; if ($script:RssNow) { $tx += ", $($script:RssNow)" }
                }
                "time" {
                    $pm = [Math]::Min(990, [int]($el * 1000 / $Arg))
                    $tx = ""; if ($j.Err.Count -gt 0) { $tx = Get-Fit $j.Err[$j.Err.Count - 1] 48 }
                }
                "fit" {
                    $rungs = 0; $meas = 0
                    foreach ($l in $j.Out) { if ($l -match '"event":"rung".*"rungs":(\d+)') { $rungs = [int]$Matches[1] }; if ($l -match '"event":"measured"') { $meas++ } }
                    if ($rungs -gt 0) { $pm = [Math]::Min(999, [int]($meas * 1000 / $rungs)); $tx = "$meas sizes measured" }
                }
            }
            Show-Tick $pm $tx $el
            if (-not $Live -and $el - $lastPlain -ge 30) { $lastPlain = $el; Write-Host "    ... $tx" }
        }
        if ($Timeout -gt 0 -and $el -gt $Timeout) {
            $j.TimedOut = $true
            Stop-Jet $j
            $j.Rc = 124
            return 124
        }
    }
    $j.P.WaitForExit()
    if ($null -ne $j.OutTask) { [void]$j.OutTask.Wait(10000) }
    if ($null -ne $j.ErrTask) { [void]$j.ErrTask.Wait(10000) }
    Read-Jet $j
    while ($null -ne $j.OutTask -or $null -ne $j.ErrTask) {
        if ($null -ne $j.OutTask) { [void]$j.OutTask.Wait(10000) }
        if ($null -ne $j.ErrTask) { [void]$j.ErrTask.Wait(10000) }
        Read-Jet $j
    }
    $j.Rc = $j.P.ExitCode
    $script:Running = $null
    if ($j.Window) { $script:WindowsSeen++ }
    return $j.Rc
}
function Stop-Jet($j) {
    try { if (-not $j.P.HasExited) { $j.P.Kill() } } catch { }
    try { [void]$j.P.WaitForExit(15000) } catch { }
    $script:Running = $null
}
function Get-JetText($j) { return ($j.Out -join "`n") }
function Save-JetErr($j, [string]$File) { Write-Utf8 $File (($j.Err -join "`n") + "`n") }
function Get-Tail([string[]]$Lines, [int]$n = 40) {
    $a = @($Lines); if ($a.Count -gt $n) { $a = $a[($a.Count - $n)..($a.Count - 1)] }
    return ($a -join "`n")
}

# The overall line's numbers.
function Show-Tick([int]$Pm, [string]$Text, $Elapsed = 0) {
    if ($Pm -ge 0) { $cd = [int]($script:EstCur * $Pm / 1000) } else {
        $cd = [int]($Elapsed * 1000 / $script:Factor); $cap = [int]($script:EstCur * 0.95); if ($cd -gt $cap) { $cd = $cap }
    }
    $all = 0; if ($script:EstTotal -gt 0) { $all = [int](($script:EstDone + $cd) * 1000 / $script:EstTotal) }
    $lft = ($script:EstTotal - $script:EstDone - $cd) * $script:Factor / 1000; if ($lft -lt 0) { $lft = 0 }
    Show-Status $script:TName $Pm $Text $all "test $($script:TestNo) of $($script:NPlan), $(Get-Left $lft)"
}

# ---- A test's record ------------------------------------------------------------------------

function Get-Est($t) {
    $a = 0
    switch ([string]$t.adapters) { "chosen" { $a = 1 } "devices" { $a = $script:NDevices } "others" { $a = $script:NOthers } }
    $s = 0; if ($t.seconds) { $s = [int]$t.seconds }
    $p = 0; if ($t.per_adapter) { $p = [int]$t.per_adapter }
    return $s + $p * $a
}
function Start-Test($t) {
    $script:TestNo++; $script:Frag++
    $script:T = $t; $script:TName = [string]$t.name
    $script:Cur = Join-Path (Join-Path $script:Work "tests") ("{0}-{1}" -f $script:Frag, $t.id)
    New-Item -ItemType Directory -Force -Path $script:Cur | Out-Null
    $script:Numbers = [ordered]@{}; $script:Details = [ordered]@{}; $script:Problems = New-Object System.Collections.ArrayList
    $script:TCmd = ""; $script:TClock = [Diagnostics.Stopwatch]::StartNew(); $script:EstCur = Get-Est $t
    $script:Current = $t
    Heading "[$($script:TestNo)/$($script:NPlan)] $($t.name)"
    Dim ([string]$t.about)
}
function Add-Problem([string]$What, [string[]]$Output = $null) {
    $p = [ordered]@{ what = $What }
    if ($Output -and @($Output).Count -gt 0) { $p.output = Get-Tail $Output }
    [void]$script:Problems.Add($p)
}
function Complete-Test([string]$Result, [string]$Summary) {
    $secs = [int]$script:TClock.Elapsed.TotalSeconds
    [void]$script:Results.Add([ordered]@{
        id = [string]$script:T.id; name = [string]$script:T.name; kind = [string]$script:T.kind
        result = $Result; summary = $Summary; seconds = $secs; command = $script:TCmd
        numbers = $script:Numbers; details = $script:Details; problems = @($script:Problems)
    })
    switch ($Result) { "pass" { $script:NPass++ } "warn" { $script:NWarn++ } "fail" { $script:NFail++ } }
    Write-Result "  " $Result $Summary "($(Get-Dur $secs))"
    [void]$script:Summary.Add("$(Get-PlainTag $Result) $($script:T.name): $Summary")
    $script:EstDone += $script:EstCur
    if (@("score", "explore", "busy") -notcontains [string]$script:T.kind -and $script:EstCur -gt 0) {
        $script:RSum += $secs; $script:ESum += $script:EstCur
        $script:Factor = [Math]::Max(250, [Math]::Min(6000, [int]($script:RSum * 1000 / $script:ESum)))
    }
    $script:Current = $null
}
function Skip-Test($t, [string]$Why) {
    $script:Frag++; $script:NSkip++
    [void]$script:Results.Add([ordered]@{ id = [string]$t.id; name = [string]$t.name; kind = [string]$t.kind; result = "skip"; summary = $Why })
    [void]$script:Summary.Add("[skip] $($t.name): $Why")
}

# The test's arguments, its placeholders filled in.
function Get-TestArgs($t, [string]$RunDir) {
    $out = @()
    foreach ($a in @($t.args)) {
        $a = [string]$a
        $a = $a.Replace("{work}", $RunDir)
        if ($a -match '^\{scene:(\d+)\}$') { $a = Join-Path $script:Scenes "$($Matches[1]).json" }
        if ($a -eq "{minutes}") { $a = [string]$script:XMinutes }
        if ($a -eq "{max_rss_gb}") { $a = $script:XRss }
        $out += $a
    }
    return , $out
}
# The command as the report and the screen show it: no folders.
function Get-ShownCmd([string[]]$a) {
    $shown = foreach ($x in $a) { if ($x.StartsWith($script:Work, [StringComparison]::OrdinalIgnoreCase)) { "<temp>" + [IO.Path]::DirectorySeparatorChar + (Split-Path -Leaf $x) } else { $x } }
    return "jetcarp " + ($shown -join " ")
}
# The devices a test runs on: objects with Flag (cpu, gpu, none), Label, GpuName, GpuApi.
function Get-Modes([bool]$CpuToo, [string]$Adapters) {
    $m = @()
    if ($CpuToo) { $m += [pscustomobject]@{ Flag = "cpu"; Label = "the processor"; GpuName = ""; GpuApi = "" } }
    $role = switch ($Adapters) { "chosen" { "chosen" } "devices" { "device" } "others" { "other" } default { "" } }
    if (-not $role) { return , $m }
    if ($script:AdapterList.Count -eq 0 -and -not $script:AdaptersProbed) {
        $m += [pscustomobject]@{ Flag = "gpu"; Label = "the GPU JETCARP picks"; GpuName = ""; GpuApi = "" }
        return , $m
    }
    foreach ($a in $script:AdapterList) {
        if ($a.role -eq $role -and $a.used) { $m += [pscustomobject]@{ Flag = "gpu"; Label = $a.label; GpuName = $a.gpu; GpuApi = $a.api } }
    }
    return , $m
}

# ---- The tests ------------------------------------------------------------------------------

function Test-Doctor($t) {
    $a = Get-TestArgs $t $script:Cur
    $script:TCmd = Get-ShownCmd $a
    Dim "Running: $($script:TCmd)"
    $j = Start-Jet $a
    $rc = Wait-Jet $j "none" $null 300
    $d = $null; if ($rc -eq 0) { try { $d = (Get-JetText $j) | ConvertFrom-Json } catch { } }
    if ($null -eq $d) {
        Add-Problem "jetcarp doctor failed (exit status $rc)" $j.Err
        Complete-Test "fail" "jetcarp doctor did not work here"; return
    }
    $script:Doctor = $d
    $hw = Get-JPath $d "machine.hardware"
    $script:Threads = $d.threads
    if ($hw) { $script:CpuName = [string]$hw.cpu; $script:Cores = $hw.cores; $script:Mem = $hw.memory_total }
    if (-not $script:CpuName) { try { $script:CpuName = [string](@(Get-CimInstance Win32_Processor)[0].Name).Trim() } catch { } }
    if (-not $script:Mem) { try { $script:Mem = (Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory } catch { } }
    $script:Gpus = @()
    foreach ($g in @($d.gpu)) { if ($g) { $script:Gpus += [pscustomobject]@{ name = [string]$g.name; backend = [string]$g.backend; kind = [string]$g.kind; driver = [string]$g.driver; problem = $g.problem } } }
    Info "JETCARP $($d.version) on $($script:System), $($script:Arch)"
    $cpuLine = "Processor: $(if ($script:CpuName) { $script:CpuName } else { 'not said' })"
    if ($script:Cores) { $cpuLine += ", $($script:Cores) cores" }; if ($script:Threads) { $cpuLine += ", $($script:Threads) threads" }
    Info "$cpuLine; memory: $(Get-Human $script:Mem)"
    foreach ($g in $script:Gpus) {
        if ($g.problem) { Dim "Graphics: $($g.name) ($($g.backend), $($g.kind), $($g.driver)): $($g.problem)" } else { Info "Graphics: $($g.name) ($($g.backend), $($g.kind), $($g.driver))" }
    }
    if ($script:Gpus.Count -eq 0) { Info "Graphics: no adapter found" }
    $script:NDevices = @($script:Gpus | Where-Object { -not $_.problem -and $_.kind -ne "Cpu" } | Select-Object -ExpandProperty name -Unique).Count
    $script:Numbers.threads = $script:Threads; $script:Numbers.memory_bytes = $script:Mem; $script:Numbers.adapters = $script:Gpus.Count
    $script:Details.doctor = $d
    $sum = "$($script:System); $(if ($script:CpuName) { $script:CpuName } else { 'a processor' }); $(Get-Human $script:Mem); $($script:Gpus.Count) graphics adapter(s)"
    $bad = @(@((Get-JPath $d "gpu_policy.choices")) | Where-Object { $_ -and $_.self_test -and $_.self_test.passed -eq $false })
    if ($bad.Count -gt 0) {
        $note = (@($bad | ForEach-Object { [string]$_.self_test.note } | Select-Object -Unique) -join "; ")
        Add-Problem "the GPU self-test failed: $note"
        Warn "The GPU self-test failed: $note"
        Complete-Test "warn" "$sum; the GPU self-test failed"
    } else {
        Complete-Test "pass" $sum
    }
}

function Test-Adapters($t) {
    $script:TCmd = "jetcarp run <scene> --gens 1 --gpu --json, once for each graphics adapter"
    Dim "Running: $($script:TCmd)"
    $script:AdapterList = New-Object System.Collections.ArrayList
    $used = @{}
    $total = $script:Gpus.Count + 1; $n = 0
    $probe = {
        param($name, $api, $role, $label)
        $n++
        Show-Tick ([int]($n * 1000 / $total)) $label
        $j = Start-Jet @("run", (Join-Path $script:Scenes "0.json"), "--gens", "1", "--gpu", "--json") $name $api
        [void](Wait-Jet $j "quiet")
        $b = ""; try { $b = [string]((Get-JetText $j) | ConvertFrom-Json).backend } catch { }
        if ($b -like "gpu*") {
            $lab = ($b -replace ';.*$', '' -replace '^gpu \(', '' -replace '\)$', '')
            $usedApi = ($lab -replace '^.*, ', '').ToLowerInvariant()
            $note = ""; if ($b -match '^[^;]*; (.*)$') { $note = $Matches[1] }
            if ($role -eq "device") { $used["$name|$usedApi"] = $true }
            [void]$script:AdapterList.Add([pscustomobject]@{ role = $role; used = $true; label = $lab; gpu = $name; api = $api; note = $note })
            if ($role -eq "chosen") { Info "JETCARP picks: $lab" } else { Info "Runs on: $lab" }
        } else {
            $why = ""; if ($b -match 'no GPU: (.*)$') { $why = $Matches[1] }
            if (-not $why -and $j.Err.Count -gt 0) { $why = $j.Err[$j.Err.Count - 1] }
            if (-not $why) { $why = "it ran on the processor" }
            [void]$script:AdapterList.Add([pscustomobject]@{ role = $role; used = $false; label = $label; gpu = $name; api = $api; note = $why })
            Dim "Not used: $($label): $why"
        }
    }
    . $probe "" "" "chosen" "the GPU JETCARP picks"
    foreach ($dn in @($script:Gpus | Where-Object { -not $_.problem -and $_.kind -ne "Cpu" } | Select-Object -ExpandProperty name -Unique)) { . $probe $dn "" "device" $dn }
    foreach ($g in $script:Gpus) {
        $api = $g.backend.ToLowerInvariant()
        if ($used.ContainsKey("$($g.name)|$api")) { continue }
        $used["$($g.name)|$api"] = $true
        . $probe $g.name $api "other" "$($g.name), $($g.backend)"
    }
    $script:AdaptersProbed = $true
    $script:NDevices = @($script:AdapterList | Where-Object { $_.role -eq "device" -and $_.used }).Count
    $script:NOthers = @($script:AdapterList | Where-Object { $_.role -eq "other" -and $_.used }).Count
    $script:Details.adapters = @($script:AdapterList)
    $script:Numbers.gpus_used = $script:NDevices; $script:Numbers.other_apis_used = $script:NOthers
    $c = @($script:AdapterList | Where-Object { $_.role -eq "chosen" })[0]
    $chosen = if ($c.used) { $c.label } else { "none: $($c.note)" }
    if ($script:NDevices -eq 0) { Complete-Test "warn" "no GPU JETCARP can use here: the tests run on the processor ($chosen)" }
    else { Complete-Test "pass" "JETCARP runs on $($script:NDevices) GPU(s), $($script:NOthers) more through other APIs; it picks $chosen" }
}

function Get-Busy {
    $b = [ordered]@{ Progs = ""; Load = $null; Busy = $false; Battery = $false }
    $ps = @(Get-Process -Name jetcarp, jetcarpw -ErrorAction SilentlyContinue)
    if ($ps.Count -gt 0) { $b.Progs = "$($ps.Count) program$(if ($ps.Count -gt 1) { 's' }): process$(if ($ps.Count -gt 1) { 'es' }) $(($ps | ForEach-Object { $_.Id }) -join ', ')" }
    try { $b.Load = [int](@(Get-CimInstance Win32_Processor) | Measure-Object -Property LoadPercentage -Average).Average } catch { }
    if ($null -ne $b.Load -and $b.Load -gt 30) { $b.Busy = $true }
    try { $bat = @(Get-CimInstance Win32_Battery -ErrorAction SilentlyContinue); if ($bat.Count -gt 0 -and $bat[0].BatteryStatus -eq 1) { $b.Battery = $true } } catch { }
    return $b
}
function Test-Busy($t) {
    $script:TCmd = "(this script: the list of programs, the load on the processor, the power supply)"
    while ($true) {
        $b = Get-Busy
        if (-not $b.Progs -and -not $b.Busy -and -not $b.Battery) { break }
        if ($b.Progs) { Warn "JETCARP is already running here ($($b.Progs)): close the app, and stop the background host (jetcarp host stop), for numbers that compare." }
        if ($b.Busy) { Warn "The processor is busy ($($b.Load)% in use): close games, video, builds and other heavy programs." }
        if ($b.Battery) { Warn "This laptop is on battery: plug it in, if you can, for numbers that compare." }
        if (-not $script:AskOk) { break }
        $a = Ask "Press Enter to look again, or type g to go on as it is:" ""
        if ($a -match '^(g|go)$') { break }
        if (-not $script:AskOk) { break }
    }
    $script:Numbers.load_percent = $b.Load; $script:Numbers.threads = $script:Threads; $script:Numbers.on_battery = $b.Battery; $script:Numbers.other_jetcarp = $b.Progs
    if (-not $b.Progs -and -not $b.Busy -and -not $b.Battery) { Complete-Test "pass" "nothing found: the processor $($b.Load)% in use" }
    else {
        $w = @(); if ($b.Progs) { $w += "JETCARP already running ($($b.Progs))" }; if ($b.Busy) { $w += "the processor $($b.Load)% in use" }; if ($b.Battery) { $w += "on battery" }
        Complete-Test "warn" "measured anyway: $($w -join '; ')"
    }
}

function Test-Score($t) {
    $a = Get-TestArgs $t $script:Cur
    $script:TCmd = Get-ShownCmd $a
    $rep = ([string]$t.report).Replace("{work}", $script:Cur)
    Dim "Running: $($script:TCmd)"
    $err = @()
    if ($script:AskOk -and $t.attach) {
        Dim "It says what it measures as it goes; if something would lower the score, it says so and asks first."
        Clear-Status
        $prev = $ErrorActionPreference; $ErrorActionPreference = "Continue"
        try { & $script:Jet @a; $rc = $LASTEXITCODE } finally { $ErrorActionPreference = $prev }
    } else {
        $j = Start-Jet $a
        $rc = Wait-Jet $j "time" 70 600
        $err = $j.Err
    }
    $r = $null; if (Test-Path -LiteralPath $rep) { $r = ConvertFrom-JsonFile $rep }
    if ($rc -ne 0 -or $null -eq $r) {
        if (@($err | Where-Object { $_ -match 'not measured' }).Count -gt 0) { Complete-Test "warn" "not measured: stopped at its question"; return }
        Add-Problem "jetcarp score failed (exit status $rc)" $err
        Complete-Test "fail" "jetcarp score did not work here"; return
    }
    $sc = $r.score.score; $load = $r.score.conditions.under_load
    $wrong = @(@($r.score.parts) | Where-Object { $_.correct -eq $false }).Count
    $script:Numbers.score = $sc; $script:Numbers.under_load = $load; $script:Numbers.wrong_answers = $wrong; $script:Numbers.seconds = $r.score.seconds
    $script:Details.report = $r
    $scs = Get-Commas $sc
    if ($wrong -gt 0) { Add-Problem "$wrong of the score's workloads gave a wrong answer" $err; Complete-Test "fail" "$scs, but $wrong workload(s) gave a wrong answer" }
    elseif ($load -eq $true) { Complete-Test "warn" "$scs, taken under load" }
    else { Complete-Test "pass" "$scs, taken with the computer quiet" }
}

# Which scenes a pick names: all, every K, A-B, or N, separated by commas.
function Test-Pick([int]$n, [string]$spec) {
    foreach ($it in $spec.Split(",")) {
        $s = $it.Trim()
        if ($s -eq "all") { return $true }
        if ($s -match '^every (\d+)$') { if ([int]$Matches[1] -gt 0 -and $n % [int]$Matches[1] -eq 0) { return $true }; continue }
        if ($s -match '^(\d+)-(\d+)$') { if ($n -ge [int]$Matches[1] -and $n -le [int]$Matches[2]) { return $true }; continue }
        if ($s -match '^\d+$' -and $n -eq [int]$s) { return $true }
    }
    return $false
}
function Read-Tsv([string]$File) {
    $rows = New-Object System.Collections.ArrayList
    foreach ($l in [IO.File]::ReadAllLines($File, [Text.Encoding]::UTF8)) {
        if (-not $l) { continue }
        $f = $l.Split([char[]]"`t", 3)
        [void]$rows.Add([pscustomobject]@{ n = [int]$f[0]; id = $f[1]; c = $(if ($f.Count -gt 2) { $f[2] } else { "" }) })
    }
    return , $rows
}

function Test-Parity($t) {
    $gens = [int]$t.gens
    $exp = $script:ParShort; $ids = $script:ScenesTsv
    if ($t.set -eq "long") { $exp = $script:ParLong; $ids = $(if ($script:ParLong) { $script:ParLong } else { $script:ParLongAny }) }
    $expected = @{}
    if ($exp) { foreach ($r in (Read-Tsv $exp)) { $expected[$r.n] = $r.c } }
    $list = @((Read-Tsv $ids) | Where-Object { Test-Pick $_.n ([string]$t.pick) })
    $modes = Get-Modes ($t.cpu -eq $true) ([string]$t.adapters)
    $script:TCmd = "jetcarp run <scene> --gens $gens --json, with --cpu on the processor and --gpu on each GPU"
    if ($modes.Count -eq 0) { Complete-Test "skip" "no adapters of this kind here"; $script:NSkip++; return }
    Dim "Running: $($script:TCmd)"
    if (-not $exp) { Warn "No expected hashes for JETCARP $($script:V): the GPUs are compared with the processor instead." }
    $nogpu = (@($modes | Where-Object { $_.Flag -eq "cpu" }).Count -gt 0) -and (@($modes | Where-Object { $_.Flag -eq "gpu" }).Count -eq 0)
    if ($nogpu) { Warn "No GPU here that JETCARP uses: the processor alone." }
    $total = $list.Count * $modes.Count; $done = 0; $allOk = $true; $fell = @(); $cpuHash = @{}; $ok = 0; $of = 0
    $modeOut = New-Object System.Collections.ArrayList
    foreach ($m in $modes) {
        $runs = New-Object System.Collections.ArrayList; $mis = New-Object System.Collections.ArrayList
        $en = 0; $mok = 0; $cn = 0; $agree = 0; $fully = 0; $some = 0; $errs = 0; $secs = 0.0; $first = ""
        foreach ($s in $list) {
            Show-Tick ([int]($done * 1000 / $total)) "$done of $total runs, $($m.Label)"
            $j = Start-Jet @("run", (Join-Path $script:Scenes "$($s.n).json"), "--gens", "$gens", "--json", "--$($m.Flag)") $m.GpuName $m.GpuApi
            $rc = Wait-Jet $j "quiet"
            $hash = "ERROR"; $sec = 0; $back = ""
            if ($rc -eq 0) { try { $o = (Get-JetText $j) | ConvertFrom-Json; $hash = [string]$o.hash; $sec = [double]$o.seconds; $back = [string]$o.backend } catch { } }
            if ($hash -eq "ERROR") { $errs++; if ($script:Problems.Count -lt 5) { Add-Problem "scene $($s.n) ($($s.id)) failed on $($m.Label) (exit status $rc)" $j.Err } }
            $g = 0
            if ($m.Flag -eq "gpu") {
                if ($back -match '(\d+) of (\d+) generations on the cpu') { $g = [int]$Matches[2] - [int]$Matches[1] } elseif ($back -like "gpu*") { $g = $gens }
                if ($g -eq $gens) { $fully++ }; if ($g -gt 0) { $some++ }
            } else { $cpuHash[$s.n] = $hash }
            if (-not $first) { $first = $back }
            $bad = $false; $want = $expected[$s.n]
            if ($want) { $en++; if ($hash -eq $want) { $mok++ } else { $bad = $true } }
            if ($m.Flag -eq "gpu" -and $cpuHash.ContainsKey($s.n)) { $cn++; if ($cpuHash[$s.n] -eq $hash) { $agree++ } elseif (-not $want) { $bad = $true } }
            if ($bad -and $mis.Count -lt 50) { [void]$mis.Add([ordered]@{ n = $s.n; id = $s.id; got = $hash; expected = $want; processor = $cpuHash[$s.n] }) }
            $secs += $sec
            [void]$runs.Add(@($s.n, $hash, $sec, $g))
            $done++
        }
        $line = if ($en -gt 0) { "$mok of $en hashes as expected" } elseif ($cn -gt 0) { "$agree of $cn agree with the processor" } else { "$($list.Count) runs" }
        if ($m.Flag -eq "gpu") { $line += "; $fully of $($list.Count) all on the GPU" }
        $nbad = $mis.Count
        if ($nbad -gt 0 -or $errs -gt 0) {
            $allOk = $false
            Write-Result "    " "fail" "$($m.Label): $line" "($(Get-Dur $secs))"
            if ($nbad -gt 0) { Add-Problem "$nbad scene(s) on $($m.Label) did not reach the hash they should (the first are under details.modes)" }
        } else { Write-Result "    " "pass" "$($m.Label): $line" "($(Get-Dur $secs))" }
        if ($m.Flag -eq "gpu" -and $some -eq 0) { $fell += $m.Label }
        $ok += $mok; $of += $(if ($en -gt 0) { $en } else { $list.Count })
        [void]$modeOut.Add([ordered]@{ device = $m.Label; flag = "--$($m.Flag)"; gpu = $m.GpuName; api = $m.GpuApi; count = $list.Count; expected = $en; as_expected = $mok
                compared_with_processor = $cn; agree_with_processor = $agree; all_on_gpu = $fully; errors = $errs; seconds = [Math]::Round($secs, 3); backend = $first
                mismatches = @($mis); runs = @($runs) })
    }
    $script:Details.modes = @($modeOut)
    $script:Details.expected_set = $(if ($script:ParNote) { $script:ParNote } else { "none for this release" })
    $script:Details.generations = $gens; $script:Details.scenes = $list.Count
    $script:Numbers.runs = $total; $script:Numbers.as_expected = $ok
    $ngm = @($modes | Where-Object { $_.Flag -eq "gpu" }).Count
    $on = "the processor"; if ($ngm -gt 0) { $on += " and $ngm GPU(s)" }; if (@($modes | Where-Object { $_.Flag -eq "cpu" }).Count -eq 0) { $on = "$ngm adapter(s)" }
    $sum = "$($list.Count) scenes x $(Get-Commas $gens) generations on $($on): $ok of $of hashes as they should be"
    if (-not $exp) { $sum = "$($list.Count) scenes x $(Get-Commas $gens) generations on $on, no expected hashes for this release" }
    if ($nogpu) { $sum += " (no GPU to compare)" }
    if (-not $allOk) { Complete-Test "fail" $sum }
    elseif ($fell.Count -gt 0) { Add-Problem "every run on $($fell -join ', ') fell back to the processor"; Complete-Test "warn" "$sum; $($fell -join ', ') ran on the processor" }
    else { Complete-Test "pass" $sum }
}

function Test-Check($t) {
    $ex = Select-Range $t.expect
    $same = @($t.same | Where-Object { $_ })
    $word = [string]$t.progress; $say = [string]$t.say
    $devs = @($t.devices | Where-Object { $_ })
    if ($devs.Count -gt 0) {
        $modes = Get-Modes ($devs -contains "cpu") $(if ($devs -contains "gpu") { [string]$t.adapters } else { "none" })
    } else {
        $modes = @([pscustomobject]@{ Flag = "none"; Label = "as written"; GpuName = ""; GpuApi = "" })
    }
    $script:TCmd = Get-ShownCmd (Get-TestArgs $t "<temp>")
    if (@($modes | Where-Object { $_.Flag -eq "cpu" }).Count -gt 0) { $script:TCmd += ", with --cpu on the processor and --gpu on each GPU" }
    Dim "Running: $($script:TCmd)"
    if ($null -eq $ex -and @($t.expect).Count -gt 0 -and $t.expect) { Warn "No known results for JETCARP $($script:V): the runs are compared with each other." }
    $ri = 0; $bad = $false; $errs = $false; $firstSame = $null
    $runs = New-Object System.Collections.ArrayList
    foreach ($m in $modes) {
        $ri++
        $rd = Join-Path $script:Cur "run-$ri"; New-Item -ItemType Directory -Force -Path $rd | Out-Null
        $a = Get-TestArgs $t $rd
        if ($m.Flag -ne "none") { $a += "--$($m.Flag)" }
        $j = Start-Jet $a $m.GpuName $m.GpuApi
        if ($word) { $rc = Wait-Jet $j "count" $word 1800 } else { $rc = Wait-Jet $j "none" $null 1800 }
        $o = $null; if ($rc -eq 0) { try { $o = (Get-JetText $j) | ConvertFrom-Json } catch { } }
        if ($null -eq $o) {
            $errs = $true
            $why = "the run on $($m.Label) failed (exit status $rc"; if ($j.TimedOut) { $why += ", stopped: it ran too long" }
            Add-Problem "$why)" $j.Err
            Write-Result "    " "fail" "$($m.Label): exit status $rc" ""
            [void]$runs.Add([ordered]@{ device = $m.Label; exit = $rc }); continue
        }
        $mm = @()
        if ($ex) {
            foreach ($p in $ex.values.PSObject.Properties) {
                $got = Get-JPath $o $p.Name
                if ("$got" -ne "$($p.Value)") { $mm += "$($p.Name) is $(if ($null -eq $got) { 'missing' } else { $got }), not $($p.Value)" }
            }
        }
        $sv = ($same | ForEach-Object { ConvertTo-Json -Compress -Depth 20 -InputObject (Get-JPath $o $_) }) -join "|"
        if ($null -eq $firstSame) { $firstSame = $sv } elseif ($sv -ne $firstSame) { $mm += "its results differ from the first run's" }
        [void]$runs.Add([ordered]@{ device = $m.Label; exit = 0; output = $o })
        if ($mm.Count -gt 0) {
            $bad = $true
            Add-Problem "on $($m.Label): $($mm -join '; ')"
            Write-Result "    " "fail" "$($m.Label): $($mm -join '; ')" ""
        } else {
            $shown = ""; if ($say) { $shown = [string](Get-JPath $o $say) }
            $shown = Get-Fit (($shown -replace '^[a-z]* [0-9]*: ', '') -replace ' \| .*$', '') 120
            $lab = $m.Label; if ($m.Flag -eq "none") { $lab = $say }
            Write-Result "    " "pass" "$($lab): $shown" ""
        }
    }
    $script:Details.runs = @($runs); $script:Numbers.runs = $ri
    if ($ex) { $script:Details.known_results_from = [string]$ex.from }
    if ($errs) { Complete-Test "fail" "a run failed" }
    elseif ($bad) { Complete-Test "fail" "the results are not the known ones" }
    elseif ($ex) { Complete-Test "pass" "$ri run(s), every result the known one" }
    else { Complete-Test "pass" "$ri run(s), the same results each time (no known results for this release)" }
}

function Test-Fit($t) {
    $a = Get-TestArgs $t $script:Cur
    $script:TCmd = Get-ShownCmd $a
    Dim "Running: $($script:TCmd)"
    Dim "Each run is started with no window, and the script checks that none opens one."
    $j = Start-Jet $a
    $rc = Wait-Jet $j "fit" $null 900
    $events = @($j.Out | Where-Object { $_ -match '^\s*\{' } | ForEach-Object { try { $_ | ConvertFrom-Json } catch { } })
    $script:Details.events = $events
    $done = @($events | Where-Object { $_.event -eq "done" }) | Select-Object -Last 1
    $failed = @($events | Where-Object { $_.event -eq "failed" }) | Select-Object -Last 1
    $meas = @($events | Where-Object { $_.event -eq "measured" })
    if ($rc -ne 0 -or $null -eq $done) {
        $why = "the fit failed (exit status $rc)"; if ($failed) { $why += ": $($failed.error)" }
        Add-Problem $why $j.Err
        Complete-Test "fail" "Fit to this computer did not finish"; return
    }
    $script:Numbers.seconds = $done.seconds; $script:Numbers.sizes_measured = $meas.Count; $script:Numbers.scenes = $done.scenes
    $fps = (@($meas | ForEach-Object { [string][Math]::Round([double]$_.rung.fps) } | Select-Object -Unique) -join ", ")
    $s = "fitted $($done.scenes) scenes in $(Get-Dur $done.seconds), $($meas.Count) size(s) measured"; if ($fps) { $s += " at $fps frames a second" }
    Complete-Test "pass" $s
}

function Test-Speed($t) {
    $modes = Get-Modes $true ([string]$t.adapters)
    $script:TCmd = "jetcarp run <scene> --gens N --json, with --cpu on the processor and --gpu on each GPU"
    Dim "Running: $($script:TCmd)"
    $rows = New-Object System.Collections.ArrayList; $bad = $false; $saved = $script:TName
    foreach ($sc in @($t.scenes)) {
        $rel = Get-DataPath $sc.scene
        $file = Join-Path $script:DataDir $rel; $g = [int]$sc.gens
        $line = @(); $first = $null
        foreach ($m in $modes) {
            $script:TName = "$saved`: $($sc.name), $($m.Label)"
            $j = Start-Jet @("run", $file, "--gens", "$g", "--json", "--$($m.Flag)") $m.GpuName $m.GpuApi
            $rc = Wait-Jet $j "none" $null 900
            $script:TName = $saved
            $o = $null; if ($rc -eq 0) { try { $o = (Get-JetText $j) | ConvertFrom-Json } catch { } }
            if ($null -eq $o) { Add-Problem "$($sc.name) on $($m.Label) failed (exit status $rc)" $j.Err; $bad = $true; continue }
            $s = [double]$o.seconds; $rate = 0; if ($s -gt 0) { $rate = [Math]::Round($g / $s, 1) }
            [void]$rows.Add([ordered]@{ scene = [string]$sc.name; device = $m.Label; flag = "--$($m.Flag)"; gens = $g; seconds = $o.seconds; cells = $o.cells
                    gens_per_second = $rate; cell_gens_per_second = $(if ($s -gt 0) { [Math]::Round([double]$o.cells * $g / $s) } else { 0 }); hash = [string]$o.hash; backend = [string]$o.backend })
            $line += "$($m.Label) $(Get-Commas $rate)/s"
            if ($null -eq $first) { $first = [string]$o.hash } elseif ([string]$o.hash -ne $first) { $bad = $true; Add-Problem "$($sc.name) on $($m.Label) reached a different hash from the processor's: $($o.hash), not $first" }
        }
        Info "$($sc.name), generations a second: $($line -join '; ')"
    }
    $script:Details.runs = @($rows)
    $best = (@($rows | Where-Object { $_.flag -eq "--cpu" } | ForEach-Object { "$($_.scene) $($_.gens_per_second.ToString($Inv))/s" }) -join ", ")
    if ($bad) { Complete-Test "fail" "a run failed or reached another hash" } else { Complete-Test "pass" "on the processor: $best; the same hash on every device" }
}

function Test-Explore($t) {
    $script:XMinutes = [int]$t.minutes
    $share = 0.25; if ($t.max_rss_share) { $share = [double]$t.max_rss_share }
    $g = [double]$script:Mem * $share / 1GB; if ($g -lt 1) { $g = 1 }
    $script:XRss = $g.ToString("0.0", $Inv)
    $a = Get-TestArgs $t $script:Cur
    $script:TCmd = Get-ShownCmd $a
    Dim "Running: $($script:TCmd)"
    Dim "It keeps every core but two busy for $($script:XMinutes) minutes, and stops itself past $($script:XRss) GB."
    $script:Rss = New-Object System.Collections.ArrayList; $script:SampleRss = $true; $script:RssNow = ""
    $j = Start-Jet $a
    $rc = Wait-Jet $j "runs" ($script:XMinutes * 60) ($script:XMinutes * 60 + 600)
    $script:SampleRss = $false
    $script:Details.memory_mb_every_5_seconds = @($script:Rss)
    $vals = @($script:Rss | ForEach-Object { [double]$_[1] }); $n = $vals.Count
    $peak = 0; if ($n -gt 0) { $peak = ($vals | Measure-Object -Maximum).Maximum }
    $script:Numbers.peak_mb = $peak; $script:Numbers.cap_gb = [double]$script:XRss
    $o = $null; if ($rc -eq 0) { try { $o = (Get-JetText $j) | ConvertFrom-Json } catch { } }
    if ($null -eq $o) {
        $why = "the search failed (exit status $rc"; if ($j.TimedOut) { $why += ", stopped: it ran far past its time" }
        Add-Problem "$why)" $j.Err; Complete-Test "fail" "the search did not finish"; return
    }
    $script:Details.output = $o
    $p = $o.progress
    $script:Numbers.runs = $p.screened; $script:Numbers.runs_per_second = $p.per_s
    $climb = $false
    if ($n -ge 8) {
        $q2 = ($vals[[int][Math]::Floor($n / 4)..([int][Math]::Floor($n / 2) - 1)] | Measure-Object -Average).Average
        $q4 = ($vals[[int][Math]::Floor(3 * $n / 4)..($n - 1)] | Measure-Object -Average).Average
        if ($q2 -gt 0 -and $q4 -gt $q2 * 1.5 -and $q4 - $q2 -gt 256) { $climb = $true }
    }
    $sum = "$(Get-Commas $p.screened) runs, $(Get-Commas $p.per_s) a second; memory at most $(Get-Human ($peak * 1MB))"
    if ($p.error -or ($p.panics -and $p.panics -ne 0)) { Add-Problem "the search reported a problem: $(if ($p.error) { $p.error } else { "$($p.panics) panic(s)" })" $j.Err; Complete-Test "fail" "$sum; it reported a problem" }
    elseif ($o.stopped -and $o.stopped -ne "budget") { Add-Problem "the search stopped early: $($o.stopped)" $j.Err; Complete-Test "fail" "$sum; it stopped early ($($o.stopped))" }
    elseif ($climb) { Add-Problem ("its memory climbed: about {0:0} MB in its second quarter, {1:0} MB in its last" -f $q2, $q4); Complete-Test "warn" "$sum; its memory climbed" }
    elseif ($p.gpu_error) { Add-Problem "the GPU reported a problem: $($p.gpu_error)"; Complete-Test "warn" "$sum; the GPU reported a problem" }
    else { Complete-Test "pass" "$sum, steady" }
}

# ---- The report -----------------------------------------------------------------------------

# What identifies this computer, each with what stands for it: the longest first. A word is
# replaced only where it stands alone.
function Get-ScrubRules {
    $rules = New-Object System.Collections.ArrayList
    $add = { param($from, $to, $word) if ($from -and $from.Length -ge 3) { [void]$rules.Add([pscustomobject]@{ From = $from; To = $to; Word = $word }) } }
    $paths = @($script:Work)
    try { $paths += (Get-Item -LiteralPath $script:Work).FullName } catch { }
    foreach ($p in $paths) { & $add $p.TrimEnd("\") "[scratch]" $false }
    $userHome = [Environment]::GetFolderPath("UserProfile")
    if ($userHome) { & $add $userHome.TrimEnd("\") "~" $false }
    $generic = @("root", "user", "admin", "administrator", "runner", "runneradmin", "guest", "owner", "test", "default", "public")
    foreach ($u in @($env:USERNAME, [Environment]::UserName)) { if ($u -and $generic -notcontains $u.ToLowerInvariant()) { & $add $u "[user]" $true } }
    foreach ($h in @($env:COMPUTERNAME, [Environment]::MachineName, [Net.Dns]::GetHostName())) { if ($h -and @("localhost", "desktop", "laptop", "pc") -notcontains $h.ToLowerInvariant()) { & $add $h "[machine]" $true } }
    try {
        foreach ($ip in [Net.NetworkInformation.NetworkInterface]::GetAllNetworkInterfaces() | ForEach-Object { $_.GetIPProperties().UnicastAddresses } | ForEach-Object { $_.Address.ToString() -replace '%.*$', '' }) {
            if ($ip -and $ip -ne "127.0.0.1" -and $ip -ne "::1") { & $add $ip "[ip]" $true }
        }
    } catch { }
    $seen = @{}
    return @($rules | Sort-Object { $_.From.Length } -Descending | Where-Object { $k = $_.From.ToLowerInvariant(); if ($seen.ContainsKey($k)) { $false } else { $seen[$k] = 1; $true } })
}
function Get-Scrubbed([string]$Text) {
    foreach ($r in Get-ScrubRules) {
        # In the JSON text a backslash is written twice.
        foreach ($form in @($r.From.Replace("\", "\\"), $r.From) | Select-Object -Unique) {
            $pat = [regex]::Escape($form)
            if ($r.Word) { $pat = "(?<![A-Za-z0-9_])$pat(?![A-Za-z0-9_])" }
            $Text = [regex]::Replace($Text, $pat, $r.To.Replace('$', '$$'), [Text.RegularExpressions.RegexOptions]::IgnoreCase)
        }
    }
    return $Text
}

function Write-Report {
    $result = "pass"; if ($script:NWarn -gt 0) { $result = "warn" }; if ($script:NFail -gt 0) { $result = "fail" }; if ($script:Stopped) { $result = "stopped" }
    $secs = [int]$script:RunClock.Elapsed.TotalSeconds
    $head = "JETCARP $($script:V)$(if ($script:Pre) { ' (release candidate)' }) on $($script:System), $($script:Arch): $($script:TierName), $(Get-Dur $secs), $((Get-Date).ToString('yyyy-MM-dd HH:mm', $Inv))"
    $counts = "$($script:NPass) passed, $($script:NWarn) with warnings, $($script:NFail) failed, $($script:NSkip) skipped$(if ($script:Stopped) { '; stopped before the end' })"
    $r = [ordered]@{
        format = "jetcarp-field-report"; format_version = 1
        summary = @($head, $counts) + @($script:Summary)
        result = $result; stopped = [bool]$script:Stopped
        counts = [ordered]@{ pass = $script:NPass; warn = $script:NWarn; fail = $script:NFail; skip = $script:NSkip }
        release = [ordered]@{ tag = "v$($script:V)"; version = $script:V; prerelease = [bool]$script:Pre; archive = $script:Archive; sha256 = $script:ArchiveSha; program = $script:ProgramVersion }
        tier = $script:TierId
        started = $script:RunStarted; ended = (Get-Date).ToString("yyyy-MM-ddTHH:mm:sszzz", $Inv); seconds = $secs
        platform = [ordered]@{ os = $script:OsName; system = $script:System; kernel = [Environment]::OSVersion.VersionString; arch = $script:Arch; target = $script:Target
            terminal = $Live; colour = $UseColor; powershell = $PSVersionTable.PSVersion.ToString(); edition = [string]$PSVersionTable.PSEdition; windows_seen = $script:WindowsSeen }
        machine = [ordered]@{ cpu = $script:CpuName; cores = $script:Cores; threads = $script:Threads; memory_bytes = $script:Mem; gpus = @($script:Gpus) }
        script = [ordered]@{ name = "jetcarp-field-test.ps1"; version = $FtVersion; data = $script:DataShown }
        tests = @($script:Results)
        privacy = [ordered]@{ sent = "nothing: this file stays on this computer until you send it"
            taken_out = @("the temporary test folder, shown as [scratch]", "your home folder, shown as ~", "your user name, shown as [user]", "the name of this computer, shown as [machine]", "its network addresses, shown as [ip]") }
    }
    $json = ConvertTo-Json -InputObject $r -Depth 40
    Write-Utf8 $script:ReportPath ((Get-Scrubbed $json) + "`n")
}

# ---- From start to end ----------------------------------------------------------------------

function Initialize-Data {
    $here = $PSScriptRoot
    $src = ""
    if ($Data) {
        if ($Data -match '^https?://') { $src = $Data.TrimEnd("/") }
        elseif (Test-Path -LiteralPath (Join-Path $Data "tests.json")) { $script:DataDir = (Resolve-Path -LiteralPath $Data).Path }
        else { Stop-Field "$Data has no tests.json" }
    } elseif ($here -and (Test-Path -LiteralPath (Join-Path $here "tests.json")) -and (Test-Path -LiteralPath (Join-Path (Join-Path $here "parity") "scenes.tsv"))) {
        $script:DataDir = $here
    } else { $src = $DataUrl }
    if ($src) {
        Info "Getting the test data from $src"
        $script:DataDir = Join-Path $script:Work "data"
        New-Item -ItemType Directory -Force -Path $script:DataDir | Out-Null
        try { Save-Url "$src/tests.json" (Join-Path $script:DataDir "tests.json") } catch { Stop-Field "could not get tests.json from $($src): $($_.Exception.Message)" }
        $tj = ConvertFrom-JsonFile (Join-Path $script:DataDir "tests.json")
        $files = @([string]$tj.parity.scenes) + @($tj.parity.sets | ForEach-Object { [string]$_.short; [string]$_.long }) + @($tj.tests | ForEach-Object { $_.scenes } | Where-Object { $_ } | ForEach-Object { [string]$_.scene })
        foreach ($f in $files | Where-Object { $_ } | Select-Object -Unique) {
            if ($f -match '^[\\/]' -or $f -match '\.\.' -or $f -match ':') { Stop-Field "tests.json names a file outside its folder: $f" }
            $to = Join-Path $script:DataDir (Get-DataPath $f)
            New-Item -ItemType Directory -Force -Path (Split-Path -Parent $to) | Out-Null
            try { Save-Url "$src/$f" $to } catch { Stop-Field "could not get $f from $($src): $($_.Exception.Message)" }
        }
        $script:DataShown = $src
    } else {
        $script:DataShown = "the folder beside the script"
        Dim "The test data: $(Get-Tilde $script:DataDir)"
    }
    $script:Tests = ConvertFrom-JsonFile (Join-Path $script:DataDir "tests.json")
    if ($null -eq $script:Tests -or $script:Tests.format -ne 1) { Stop-Field "tests.json is format $(if ($script:Tests) { $script:Tests.format } else { 'unknown' }), and this script reads format 1: get the newest jetcarp-field-test.ps1" }
    $script:Scenes = Join-Path $script:Work "scenes"
    New-Item -ItemType Directory -Force -Path $script:Scenes | Out-Null
    $script:ScenesTsv = Join-Path $script:DataDir (Get-DataPath $script:Tests.parity.scenes)
    foreach ($row in (Read-Tsv $script:ScenesTsv)) { Write-Utf8 (Join-Path $script:Scenes "$($row.n).json") ($row.c + "`n") }
}

function Get-Releases {
    Info "Getting the list of JETCARP releases from GitHub"
    $script:Releases = New-Object System.Collections.ArrayList
    $script:RelErr = ""
    for ($pg = 1; $pg -le 10; $pg++) {
        $f = Join-Path $script:Work "releases-$pg.json"
        try { Save-Url "$($ApiUrl)?per_page=30&page=$pg" $f } catch { $script:RelErr = "GitHub's list of releases could not be read ($($_.Exception.Message))"; break }
        # Windows PowerShell 5.1 hands a JSON array on as one object: enumerated here.
        $page = @((ConvertFrom-JsonFile $f) | ForEach-Object { $_ })
        foreach ($r in $page) {
            if ($null -eq $r -or $r.draft) { continue }
            $v = ([string]$r.tag_name).TrimStart("v")
            $asset = @($r.assets | Where-Object { $_.name -eq "jetcarp-$v-$($script:Target)$($script:Ext)" })
            [void]$script:Releases.Add([pscustomobject]@{ v = $v; pre = [bool]$r.prerelease; date = ([string]$r.published_at).Substring(0, [Math]::Min(10, ([string]$r.published_at).Length)); size = $(if ($asset.Count -gt 0) { [long]$asset[0].size } else { 0 }) })
        }
        if ($page.Count -lt 30) { break }
    }
}

function Select-Release {
    $n = $script:Releases.Count
    $row = $null
    if ($Release) {
        $want = $Release.TrimStart("v")
        if ($want -eq "latest") { if ($n -eq 0) { Stop-Field "could not find the newest release ($($script:RelErr))" }; $want = $script:Releases[0].v }
        $row = @($script:Releases | Where-Object { $_.v -eq $want })[0]
        if ($null -eq $row -and $n -gt 0) { Stop-Field "there is no release $Release (the newest is $($script:Releases[0].v))" }
        if ($null -eq $row) { $row = [pscustomobject]@{ v = $want; pre = ($want -like "*-*"); date = ""; size = 0 } }
    } elseif ($n -eq 0) {
        Stop-Field "$(if ($script:RelErr) { $script:RelErr } else { 'GitHub lists no releases' }); name one with -Release (such as 0.4.3-rc.2)"
    } elseif ($script:AskOk) {
        Heading "Which release?"
        for ($i = 0; $i -lt [Math]::Min($n, 25); $i++) {
            $r = $script:Releases[$i]
            $note = ""; if ($i -eq 0) { $note = "   the newest" }; if (-not $r.size) { $note += "   (no build for this computer)" }
            Write-Host ("  {0,3}) {1,-16} {2,-18} " -f ($i + 1), $r.v, $(if ($r.pre) { "release candidate" } else { "release" })) -NoNewline
            if ($UseColor) { Write-Host "$($r.date)$note" -ForegroundColor DarkGray } else { Write-Host "$($r.date)$note" }
        }
        while ($true) {
            $a = Ask "Press Enter for 1 (the newest), or type a number:" "1"
            if ($a -match '^\d+$' -and [int]$a -ge 1 -and [int]$a -le $n) { $row = $script:Releases[[int]$a - 1]; break }
            Warn "Type a number from the list."
        }
    } else { $row = $script:Releases[0] }
    $script:V = $row.v; $script:Pre = $row.pre; $script:Size = $row.size
    if (-not $script:Size -and $n -gt 0) { Stop-Field "JETCARP $($script:V) has no build for this computer ($($script:Target))" }
    if ($script:V -notmatch '^\d+\.\d+\.\d+') { Stop-Field "$($script:V) is not a JETCARP version" }
}

function Select-Tier {
    $tiers = @($script:Tests.tiers)
    $id = $Tier
    if (-not $id -and $script:AskOk) {
        Heading "How long?"
        for ($i = 0; $i -lt $tiers.Count; $i++) {
            Write-Host ("  {0}) {1,-9} {2}" -f ($i + 1), $tiers[$i].name, $tiers[$i].minutes)
            if ($UseColor) { Write-Host "       $($tiers[$i].about)" -ForegroundColor DarkGray } else { Write-Host "       $($tiers[$i].about)" }
        }
        while ($true) {
            $a = Ask "Press Enter for 1 ($($tiers[0].name)), or type a number:" "1"
            if ($a -match '^\d$' -and [int]$a -ge 1 -and [int]$a -le $tiers.Count) { $id = [string]$tiers[[int]$a - 1].id; break }
            Warn "Type 1, 2 or 3."
        }
    }
    if (-not $id) { $id = "quick" }
    $t = @($tiers | Where-Object { $_.id -eq $id })[0]
    if ($null -eq $t) { Stop-Field "tests.json has no tier $id" }
    $script:TierId = $id; $script:TierName = [string]$t.name
}

function New-Plan {
    $script:Plan = New-Object System.Collections.ArrayList; $script:NPlan = 0
    foreach ($t in @($script:Tests.tests)) {
        if (@($t.tiers) -notcontains $script:TierId) { continue }
        if (Test-InRange $script:V ([string]$t.from) ([string]$t.to)) { [void]$script:Plan.Add([pscustomobject]@{ T = $t; Run = $true; Why = "" }); $script:NPlan++ }
        elseif ($t.to -and -not (Test-InRange $script:V "" ([string]$t.to))) { [void]$script:Plan.Add([pscustomobject]@{ T = $t; Run = $false; Why = "not for JETCARP after $($t.to)" }) }
        else { [void]$script:Plan.Add([pscustomobject]@{ T = $t; Run = $false; Why = "needs JETCARP $($t.from) or later" }) }
    }
    $set = Select-Range $script:Tests.parity.sets
    $script:ParLongAny = Join-Path $script:DataDir (Get-DataPath @($script:Tests.parity.sets)[0].long)
    if ($set) {
        $script:ParShort = Join-Path $script:DataDir (Get-DataPath $set.short)
        $script:ParLong = Join-Path $script:DataDir (Get-DataPath $set.long)
        $script:ParNote = "from $($set.from)$(if ($set.to) { " to $($set.to)" }): $($set.hash)"
    }
    $script:EstTotal = 0
    foreach ($p in $script:Plan) { if ($p.Run) { $script:EstTotal += Get-Est $p.T } }
}

function Show-Plan {
    Heading "The plan: JETCARP $($script:V), $($script:TierName)"
    $k = 0
    foreach ($p in $script:Plan) {
        if ($p.Run) { $k++; Write-Host ("  {0,2}  {1,-46} " -f $k, $p.T.name) -NoNewline; $x = "~$(Get-Dur (Get-Est $p.T))" }
        else { Write-Host ("   -  {0,-46} " -f $p.T.name) -NoNewline; $x = "skipped: $($p.Why)" }
        if ($UseColor) { Write-Host $x -ForegroundColor DarkGray } else { Write-Host $x }
    }
    Info "About $(Get-Dur $script:EstTotal) on a typical laptop, longer with several GPUs."
    Info "Nothing is drawn on screen, nothing is sent anywhere, and your own JETCARP, its settings and"
    Info "its library are not touched: this version runs from a temporary folder, with folders of its own."
}

function Install-Scratch {
    Heading "Getting JETCARP $($script:V)"
    $script:Base = "https://github.com/$Repo/releases/download/v$($script:V)"
    $script:Archive = "jetcarp-$($script:V)-$($script:Target)$($script:Ext)"
    $dl = Join-Path $script:Work "download"; New-Item -ItemType Directory -Force -Path $dl | Out-Null
    Info "From $($script:Base)"
    try { Save-Url "$($script:Base)/SHA256SUMS" (Join-Path $dl "SHA256SUMS") } catch { Stop-Field "could not get SHA256SUMS for $($script:V) (is v$($script:V) published?)" }
    $zip = Join-Path $dl $script:Archive
    $wc = New-Object System.Net.WebClient
    $wc.Headers["User-Agent"] = "jetcarp-field-test"
    $task = $wc.DownloadFileTaskAsync("$($script:Base)/$($script:Archive)", $zip)
    while ($true) {
        # A failed download ends the wait with an exception: it is reported below.
        try { if ($task.Wait(200)) { break } } catch { break }
        $got = 0; if (Test-Path -LiteralPath $zip) { $got = (Get-Item -LiteralPath $zip).Length }
        if ($script:Size -gt 0) { Show-Status "Downloading JETCARP $($script:V)" ([int]($got * 1000 / $script:Size)) "$(Get-Human $got) of $(Get-Human $script:Size)" -1 "" }
        else { Show-Status "Downloading JETCARP $($script:V)" -1 (Get-Human $got) -1 "" }
    }
    $wc.Dispose()
    if ($task.IsFaulted) { Stop-Field "could not download $($script:Archive): $($task.Exception.InnerException.Message)" }
    Clear-Status
    $script:ArchiveSha = Get-Sha256 $zip
    $want = $null
    foreach ($line in [IO.File]::ReadAllLines((Join-Path $dl "SHA256SUMS"))) {
        $f = $line -split "\s+", 2
        if ($f.Count -eq 2 -and $f[1].TrimStart("*") -eq $script:Archive) { $want = $f[0].ToLowerInvariant() }
    }
    if (-not $want) { Stop-Field "SHA256SUMS does not list $($script:Archive)" }
    if ($want -ne $script:ArchiveSha) { Stop-Field "$($script:Archive) does not match SHA256SUMS (expected $want, got $($script:ArchiveSha)): nothing was run" }
    Info "Downloaded $($script:Archive) ($(Get-Human (Get-Item -LiteralPath $zip).Length)); it matches the release's SHA256SUMS"
    $un = Join-Path $script:Work "jetcarp"
    New-Item -ItemType Directory -Force -Path $un | Out-Null
    if ($script:Ext -eq ".zip") {
        try {
            Add-Type -AssemblyName System.IO.Compression.FileSystem
            [IO.Compression.ZipFile]::ExtractToDirectory($zip, $un)
        } catch { Expand-Archive -Path $zip -DestinationPath $un -Force }
    } else {
        & tar -xzf $zip -C $un
        if ($LASTEXITCODE -ne 0) { Stop-Field "could not unpack $($script:Archive)" }
    }
    $script:Jet = Join-Path (Join-Path $un "jetcarp-$($script:V)-$($script:Target)") $script:ExeName
    if (-not (Test-Path -LiteralPath $script:Jet)) { Stop-Field "$($script:Archive) has no $($script:ExeName)" }
    # JETCARP's own folders, inside the temporary one.
    $f = Join-Path $script:Work "folders"
    $env:JETCARP_CONFIG_DIR = Join-Path $f "settings"
    $env:JETCARP_DATA_DIR = Join-Path $f "data"
    $env:JETCARP_OUT = Join-Path $f "output"
    $env:JETCARP_RUNTIME_DIR = Join-Path $f "runtime"
    $env:JETCARP_LIBRARY = Join-Path $env:JETCARP_DATA_DIR "library.sqlite"
    foreach ($d in @($env:JETCARP_CONFIG_DIR, $env:JETCARP_DATA_DIR, $env:JETCARP_OUT, $env:JETCARP_RUNTIME_DIR)) { New-Item -ItemType Directory -Force -Path $d | Out-Null }
    $j = Start-Jet @("--version")
    $rc = Wait-Jet $j "quiet" $null 120
    if ($rc -ne 0) { Stop-Field "JETCARP $($script:V)'s program does not run here: $(Get-Tail $j.Err 5)" }
    $script:ProgramVersion = (Get-JetText $j).Trim()
    Info "Unpacked $($script:ProgramVersion) into $(Get-Tilde $script:Work), with its settings, data and output folders inside it"
}

function Invoke-Tests {
    $script:Started = $true
    $script:RunClock = [Diagnostics.Stopwatch]::StartNew()
    $script:RunStarted = (Get-Date).ToString("yyyy-MM-ddTHH:mm:sszzz", $Inv)
    for ($i = 0; $i -lt $script:Plan.Count; $i++) {
        $p = $script:Plan[$i]
        if (-not $p.Run) {
            Heading ([string]$p.T.name)
            Write-Result "  " "skip" $p.Why ""
            Skip-Test $p.T $p.Why
            continue
        }
        Start-Test $p.T
        switch ([string]$p.T.kind) {
            "doctor" { Test-Doctor $p.T }
            "adapters" { Test-Adapters $p.T }
            "busy" { Test-Busy $p.T }
            "score" { Test-Score $p.T }
            "parity" { Test-Parity $p.T }
            "check" { Test-Check $p.T }
            "fit" { Test-Fit $p.T }
            "speed" { Test-Speed $p.T }
            "explore" { Test-Explore $p.T }
            default { Add-Problem "this script doesn't know tests of the kind $($p.T.kind): get the newest jetcarp-field-test.ps1"; Complete-Test "skip" "not known to this script"; $script:NSkip++ }
        }
        # After the doctor, the GPUs are known: the estimates follow.
        if (@("doctor", "adapters") -contains [string]$p.T.kind) {
            $rest = 0; for ($k = $i + 1; $k -lt $script:Plan.Count; $k++) { if ($script:Plan[$k].Run) { $rest += Get-Est $script:Plan[$k].T } }
            $script:EstTotal = $script:EstDone + $rest
        }
    }
}

function Restore-Env {
    foreach ($n in @("JETCARP_CONFIG_DIR", "JETCARP_DATA_DIR", "JETCARP_OUT", "JETCARP_RUNTIME_DIR", "JETCARP_LIBRARY")) { Remove-Item "env:$n" -ErrorAction SilentlyContinue }
    foreach ($k in $script:SavedEnv.Keys) { Set-Item "env:$k" $script:SavedEnv[$k] }
}

function Complete-Run {
    Write-Report
    $secs = [int]$script:RunClock.Elapsed.TotalSeconds
    Heading "Finished in $(Get-Dur $secs): $($script:NPass) passed, $($script:NWarn) with warnings, $($script:NFail) failed, $($script:NSkip) skipped"
    foreach ($l in $script:Summary) {
        $c = $null
        if ($l.StartsWith("[FAIL]")) { $c = "Red" } elseif ($l.StartsWith("[warn]")) { $c = "Yellow" } elseif ($l.StartsWith("[skip]")) { $c = "DarkGray" }
        if ($UseColor -and $c) { Write-Host "  $l" -ForegroundColor $c } else { Write-Host "  $l" }
    }
    Heading "The report"
    Info "$(Get-Tilde $script:ReportPath) ($(Get-Human (Get-Item -LiteralPath $script:ReportPath).Length))"
    Info "It holds the release, this computer's system, processor, memory and graphics as JETCARP sees"
    Info "them, each test's result, numbers and timings, and the output of anything that went wrong."
    Info "Taken out: your user name, your home folder (shown as ~), this computer's name and its"
    Info "network addresses. Nothing was sent: send this one file by hand to whoever asked you to test."
    $a = Ask "Show the whole file now? [Y/n]" "y"
    if ($script:AskOk -and $a -match '^[Yy]') {
        try { Get-Content -LiteralPath $script:ReportPath -Encoding UTF8 | Out-Host -Paging } catch { Get-Content -LiteralPath $script:ReportPath -Encoding UTF8 | Write-Host }
    }
    $script:Finished = $true
    Complete-Choice
    Say ""
    Say "The report: $(Get-Tilde $script:ReportPath)"
}

function Complete-Choice {
    $c = ""; if ($Keep) { $c = "keep" } elseif ($Remove) { $c = "remove" }
    $marker = ""; $where = "in ~/.local/lib/jetcarp, linked from ~/.local/bin/jetcarp"
    if ($script:OnWindows) { $marker = Join-Path $env:LOCALAPPDATA "Programs\jetcarp\.jetcarp-installer"; $where = "in %LOCALAPPDATA%\Programs\jetcarp, on your PATH" }
    elseif ($HOME) { $marker = Join-Path $HOME ".local/lib/jetcarp/.jetcarp-installer" }
    if (-not $c -and $script:AskOk) {
        if ($marker -and (Test-Path -LiteralPath $marker)) { $where = "in place of the $(((Get-Content -LiteralPath $marker -TotalCount 1) -split ' ')[0..1] -join ' ') installed there (your settings and library stay)" }
        Heading "Keep JETCARP $($script:V)?"
        Info "1) Remove everything this script added: the temporary folder, with the program and its test"
        Info "   folders (the default)"
        Info "2) Keep JETCARP $($script:V): install it with JETCARP's own installer, $where"
        $a = Ask "Press Enter for 1, or type 2:" "1"
        if ($a -match '^(2|k|keep)$') { $c = "keep" } else { $c = "remove" }
    }
    if (-not $c) { $c = "remove" }
    Restore-Env
    if ($c -eq "keep") {
        Heading "Installing JETCARP $($script:V)"
        $dl = Join-Path $script:Work "download"
        $iname = "jetcarp-installer.ps1"; if (-not $script:OnWindows) { $iname = "jetcarp-installer.sh" }
        $inst = Join-Path $dl $iname
        try {
            Save-Url "$($script:Base)/$iname" $inst
            $want = $null
            foreach ($line in [IO.File]::ReadAllLines((Join-Path $dl "SHA256SUMS"))) { $f = $line -split "\s+", 2; if ($f.Count -eq 2 -and $f[1].TrimStart("*") -eq $iname) { $want = $f[0].ToLowerInvariant() } }
            if (-not $want -or $want -ne (Get-Sha256 $inst)) { Warn "The installer does not match SHA256SUMS: nothing was installed." }
            else {
                Info "With the release's own installer (it matches SHA256SUMS): $iname, version $($script:V)"
                $shell = (Get-Process -Id $PID).Path
                $prev = $ErrorActionPreference; $ErrorActionPreference = "Continue"
                try {
                    if ($script:OnWindows) { & $shell -NoProfile -ExecutionPolicy Bypass -File $inst -Version $script:V -DownloadUrl $dl 2>&1 | ForEach-Object { Write-Host "    $_" } }
                    else { & sh $inst --version $script:V --url $dl 2>&1 | ForEach-Object { Write-Host "    $_" } }
                    $irc = $LASTEXITCODE
                } finally { $ErrorActionPreference = $prev }
                if ($irc -ne 0) { Warn "The installer stopped (exit status $irc): nothing of this version was kept." }
            }
        } catch { Warn "Could not get JETCARP's installer: nothing was installed." }
    }
    if ($KeepWork) { Info "Keeping the temporary folder, as FIELD_TEST_KEEP_WORK asks: $(Get-Tilde $script:Work)" }
    else {
        Remove-Item -Recurse -Force -LiteralPath $script:Work -ErrorAction SilentlyContinue
        Info "Removed the temporary folder ($(Get-Tilde $script:Work)): nothing of the test is left but the report."
        $script:Work = $null
    }
}

# ---- State, and the run ---------------------------------------------------------------------

$script:Work = $null; $script:Jet = $null; $script:Running = $null; $script:Started = $false; $script:Stopped = $false; $script:Finished = $false
$script:Current = $null; $script:TestNo = 0; $script:Frag = 0; $script:NPass = 0; $script:NWarn = 0; $script:NFail = 0; $script:NSkip = 0
$script:EstTotal = 0; $script:EstDone = 0; $script:EstCur = 0; $script:Factor = 1000; $script:RSum = 0; $script:ESum = 0
$script:Threads = $null; $script:Mem = $null; $script:CpuName = ""; $script:Cores = $null; $script:Gpus = @()
$script:NDevices = 1; $script:NOthers = 1; $script:AdapterList = New-Object System.Collections.ArrayList; $script:AdaptersProbed = $false
$script:ParShort = ""; $script:ParLong = ""; $script:ParNote = ""; $script:SampleRss = $false; $script:WindowsSeen = 0
$script:Results = New-Object System.Collections.ArrayList; $script:Summary = New-Object System.Collections.ArrayList
$script:Archive = ""; $script:ArchiveSha = ""; $script:ProgramVersion = ""; $script:TName = ""
# Windows is what this script is for; PowerShell 7 on Linux or macOS runs it too (for testing the
# script itself: jetcarp-field-test.sh is the one for those systems).
$script:OnWindows = ($PSVersionTable.PSEdition -ne "Core") -or $IsWindows
$script:OsName = "windows"; $script:Target = "x86_64-pc-windows-msvc"; $script:Ext = ".zip"; $script:ExeName = "jetcarp.exe"
$script:Arch = if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64") { "aarch64 (running x86_64)" } else { "x86_64" }
$script:System = "Windows"
if ($script:OnWindows) {
    try { $os = Get-CimInstance Win32_OperatingSystem; $script:System = "$($os.Caption) $($os.Version)".Trim() } catch { }
} else {
    $m = (& uname -m).Trim(); $script:Arch = if ($m -match '^(arm64|aarch64)$') { "aarch64" } else { "x86_64" }
    $script:Ext = ".tar.gz"; $script:ExeName = "jetcarp"
    if ($IsMacOS) { $script:OsName = "macos"; $script:Target = "$($script:Arch)-apple-darwin"; $script:System = "macOS $((& sw_vers -productVersion).Trim())" }
    else {
        $script:OsName = "linux"; $script:Target = "$($script:Arch)-unknown-linux-gnu"; $script:System = "Linux"
        try { $script:System = ((Get-Content /etc/os-release | Where-Object { $_ -like "PRETTY_NAME=*" }) -replace '^PRETTY_NAME=', '' -replace '"', '') } catch { }
    }
}
# The JETCARP and wgpu variables of this session, kept for the end and cleared: the tests run with
# their own.
$script:SavedEnv = @{}
Get-ChildItem env: | Where-Object { $_.Name -like "JETCARP_*" -or $_.Name -like "WGPU_*" } | ForEach-Object { $script:SavedEnv[$_.Name] = $_.Value }
foreach ($envName in @($script:SavedEnv.Keys)) { Remove-Item "env:$envName" }
$ProgressPreference = "SilentlyContinue"
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
$exitCode = 0
try {
    if ($script:OnWindows -and $env:PROCESSOR_ARCHITECTURE -ne "AMD64" -and $env:PROCESSOR_ARCHITECTURE -ne "ARM64") { Stop-Field "JETCARP has no build for $($env:PROCESSOR_ARCHITECTURE) processors" }
    if ($UseColor) { Write-Host "JETCARP field test " -ForegroundColor Cyan -NoNewline; Write-Host "(script $FtVersion)" -ForegroundColor DarkGray } else { Write-Host "JETCARP field test (script $FtVersion)" }
    Say "This tests a JETCARP release on this computer, and writes one report file for you to send."
    Say "It shows nothing on screen (nothing can flash), sends nothing anywhere, and leaves your own"
    Say "JETCARP alone. Ctrl-C stops it at any point."
    $script:Work = Join-Path ([IO.Path]::GetTempPath()) ("jetcarp-field." + [Guid]::NewGuid().ToString("N").Substring(0, 8))
    New-Item -ItemType Directory -Path $script:Work | Out-Null
    $script:Work = (Get-Item -LiteralPath $script:Work).FullName.TrimEnd("\")
    Heading "Getting ready"
    Initialize-Data
    Get-Releases
    Select-Release
    Select-Tier
    if ($Report) { $script:ReportPath = [IO.Path]::GetFullPath((Join-Path (Get-Location).ProviderPath $Report)) }
    else {
        $dir = (Get-Location).ProviderPath
        $win = [Environment]::GetFolderPath("Windows")
        $ok = $false
        if ($dir -and -not ($win -and $dir.StartsWith($win, [StringComparison]::OrdinalIgnoreCase))) {
            try { $probe = Join-Path $dir ("jetcarp-field-check-" + $PID); [IO.File]::WriteAllText($probe, ""); Remove-Item -LiteralPath $probe; $ok = $true } catch { }
        }
        if (-not $ok) { $dir = [Environment]::GetFolderPath("UserProfile") }
        $script:ReportPath = Join-Path $dir ("jetcarp-field-report-{0}-{1}-{2}.json" -f $script:V, $script:TierId, (Get-Date).ToString("yyyyMMdd-HHmm", $Inv))
    }
    try { $probe = "$($script:ReportPath).check.$PID"; [IO.File]::WriteAllText($probe, ""); Remove-Item -LiteralPath $probe } catch { Stop-Field "can't write the report to $($script:ReportPath) (choose another place with -Report)" }
    New-Plan
    Show-Plan
    $a = Ask "Start? [Y/n]" "y"
    if ($a -match '^[Nn]') { Say "Nothing was run."; return }
    Install-Scratch
    Invoke-Tests
    Complete-Run
    if ($script:NFail -gt 0) { $exitCode = 1 }
} catch [System.OperationCanceledException] {
    $exitCode = 1
} catch {
    Clear-Status
    Write-Host "jetcarp-field-test: $($_.Exception.Message)" -ForegroundColor Red
    Write-Host ($_.ScriptStackTrace)
    $exitCode = 1
    if ($script:Started -and -not $script:Finished) {
        try { if ($script:Current) { Add-Problem "the script itself failed here: $($_.Exception.Message)"; Complete-Test "fail" "the script failed while this test ran" }; Write-Report; Say "A report of what had run: $(Get-Tilde $script:ReportPath)" } catch { }
    }
} finally {
    # Ctrl-C, or the window closing: stop the running test, write what there is, clean up.
    if ($script:Started -and -not $script:Finished -and $exitCode -eq 0) {
        Clear-Status
        Write-Host ""
        Write-Host "  Stopped." -ForegroundColor Yellow
        $script:Stopped = $true
        if ($script:Running) { Stop-Jet $script:Running }
        try {
            if ($script:Current) { Add-Problem "stopped while this test ran"; Complete-Test "stop" "stopped part way" }
            Write-Report
            Write-Host "A report of what had run, marked as stopped: $(Get-Tilde $script:ReportPath)"
        } catch { }
        $exitCode = 130
    } elseif ($script:Running) { Stop-Jet $script:Running }
    if ($script:Work -and (Test-Path -LiteralPath $script:Work)) {
        if ($KeepWork) { Write-Host "Kept the temporary folder: $($script:Work)" } else { Remove-Item -Recurse -Force -LiteralPath $script:Work -ErrorAction SilentlyContinue }
    }
    Restore-Env
    # A Ctrl-C ends the script here, after this block, with the status a stop gives.
    if ($script:Stopped) { exit 130 }
}
exit $exitCode
