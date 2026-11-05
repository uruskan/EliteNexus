# Off-screen UI screenshot of a debug build: the window opens at -20000,-20000 with no taskbar
# button, so it never appears over the game. Saves target\shots\<Out>.png and exits; never uploads.
#   powershell -File tools\dev\shot.ps1 -Page Exobiology -Tab untouched -Out bonus -Size 1300x900 -Delay 20
# -Page: a sidebar page (Dashboard, Exploration, Exobiology, Navigation, Market, ...).
# -Tab:  market sections (abnormal, database, ...), galaxy (whole-galaxy map), find / history /
#        unsold (Exploration), untouched / known (Exobiology > Find planets), where / where:Monazite /
#        where-route (Mining > Where to mine: near you, near you for that material, or along a route
#        towards Colonia). find, untouched, known and the where tabs start their search on the first
#        frame, so give them -Delay 15-25 for Spansh. routes (Market > Trade routes) searches the
#        local database on the first frame too, and station (Market > Station) opens four markets
#        side by side.
#        With -Page Hud: glare or cockpit (those tabs), or a preset name (mocha, royalpurple, ...)
#        to start the HUD colours tab from; add ,options (empireblue,options) to open the
#        protection's options. Set ELITE_NEXUS_GAME_DIR to show the page for a test copy of the
#        game folder instead of the real one (AGENTS.md says how to fake the installed hook).
#        With -Page Navigation: carrier, exact or neutron opens that planner; route-carrier opens
#        the fleet carrier planner and presses Route my ship here on the first frame (give it
#        -Delay 15-25 for Spansh).
#        With -Page Fleet: buy opens Where to buy with the current ship's build, every row
#        ticked (a second ship like it), and searches on the first frame (give it -Delay 15-25
#        for Spansh); buy-one searches for one module (a 6A frame shift drive) the same way;
#        buy-build opens A whole build with nothing loaded (the paste box and your ships);
#        plan opens Plan a loadout with the current ship, plan-fsd with its frame shift
#        drive's module list open and engineering planned for it (its materials show),
#        plan-new at its start (a hull, one of your ships or a pasted build), plan-hull
#        with a new Python as a shipyard sells it.
#        With -Page Statistics: career, activity, ships or all opens that tab.
# -JournalDir: read a test journal folder instead of the real one (test runs never upload).
# -Overlay: an overlay moment with sample content, compact and expanded (jump, hyperspace, arrival, approach,
#        sampling, close, mining, trade, docking, docked, carrier, two, alert, quiet).
# -Demo: a sample Ask conversation, an alert banner and sample alerts in the alert history; with
#        -Tab bell the history (the list under the toolbar's bell) is open.
# -Exe: another build (e.g. one from your own CARGO_TARGET_DIR). The data folder is the build's own
#        (target\debug\nexus-data).
param(
    [string]$Page = "Dashboard",
    [string]$Out = "shot",
    [string]$Size = "",
    [string]$Tab = "",
    [switch]$Demo,
    [double]$Delay = 0,
    [string]$Exe = "",
    [string]$JournalDir = "",
    [string]$Overlay = "",
    [int]$TimeoutSec = 120
)
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if ($Exe -eq "") {
    $target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
    $Exe = Join-Path $target 'debug\EliteNexus.exe'
}
[System.Diagnostics.Process]::GetCurrentProcess().PriorityClass = 'BelowNormal'
$shots = Join-Path $root 'target\shots'
New-Item -ItemType Directory -Force -Path $shots | Out-Null
$png = Join-Path $shots "$Out.png"
if (Test-Path $png) { Remove-Item $png -Force -Confirm:$false }
$cmd = "--page `"$Page`" --screenshot `"$png`" --mute"
if ($Size -ne "") { $cmd += " --size $Size" }
if ($Tab -ne "") { $cmd += " --tab $Tab" }
if ($Demo) { $cmd += " --demo" }
if ($Overlay -ne "") { $cmd += " --overlay $Overlay" }
if ($JournalDir -ne "") { $cmd += " --journal-dir `"$JournalDir`"" }
if ($Delay -gt 0) { $cmd += " --shot-delay $Delay" }
$p = Start-Process -FilePath $Exe -ArgumentList $cmd -PassThru -WindowStyle Hidden
$null = $p.Handle
$p.PriorityClass = 'BelowNormal'
if (-not $p.WaitForExit($TimeoutSec * 1000)) {
    Stop-Process -Id $p.Id -Force -Confirm:$false
    Write-Output "TIMEOUT"
    exit 1
}
if (-not (Test-Path $png)) { Write-Output "NO PNG (exit $($p.ExitCode))"; exit 1 }
Write-Output "saved $png"
# -Size is in points. A display scaled above 100 % saves a larger PNG of the same layout: say so, from the PNG's header.
if ($Size -match '^(\d+)x(\d+)$') {
    $rw = [double]$Matches[1]; $rh = [double]$Matches[2]
    $fs = [IO.File]::OpenRead($png); $b = New-Object byte[] 24; $null = $fs.Read($b, 0, 24); $fs.Close()
    $w = ([int]$b[16] -shl 24) -bor ([int]$b[17] -shl 16) -bor ([int]$b[18] -shl 8) -bor [int]$b[19]
    $h = ([int]$b[20] -shl 24) -bor ([int]$b[21] -shl 16) -bor ([int]$b[22] -shl 8) -bor [int]$b[23]
    if ($w -ne $rw -or $h -ne $rh) {
        $sx = $w / $rw; $sy = $h / $rh
        if ([math]::Abs($sx - $sy) -lt 0.02 -and $sx -gt 1) {
            Write-Output ("PNG is {0}x{1}: the display is scaled to {2:0} % and -Size is in points, so this is the layout for {3}." -f $w, $h, ($sx * 100), $Size)
        } else {
            Write-Output "PNG is $($w)x$($h), not $Size."
        }
    }
}
