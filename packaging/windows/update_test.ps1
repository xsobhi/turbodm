# The in-app update, end to end on a real Windows desktop (CI): the installed TurboDM pretends
# to be old, finds the latest release on GitHub, and "Update now" must install it and reopen it.
param([string]$Exe = "$env:ProgramFiles\TurboDM\bin\turbodm.exe", [string]$Installer)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
function Shot($file) {
    $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $image = New-Object System.Drawing.Bitmap $b.Width, $b.Height
    [System.Drawing.Graphics]::FromImage($image).CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size)
    $image.Save("$PWD\$file")
}
$key = "HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{6B0C7A52-3E0B-4B7E-9C1D-7A4D2F1B9E21}_is1"
# the version to end up with: this build's installer (-Installer), or the latest release
$latest = if ($Installer) { ([regex]'TurboDM-|-windows.*').Replace((Split-Path $Installer -Leaf), "") }
          else { (Invoke-RestMethod https://api.github.com/repos/xsobhi/turbodm/releases/latest).tag_name.TrimStart("v") }
# mark the installed copy as old, so only a real update brings back the release's version
Set-ItemProperty $key DisplayVersion "0.0.1-test"
Write-Host "latest release $latest"
Get-Process turbodm -ErrorAction SilentlyContinue | Stop-Process
$env:TURBODM_PRETEND_VERSION = "0.0.1"
if ($Installer) {
    # instead of the release's: this build's installer, served from this machine (TurboDM
    # downloads it as it would the release's), with GitHub's answer about it (GitHub limits how
    # often a test machine may ask)
    $dir = Split-Path $Installer
    $base = "http://127.0.0.1:8766"
    @{ tag_name = "v$latest"; html_url = "$base/"; body = "## What's new`n`n- This build";
       assets = @(@{ name = (Split-Path $Installer -Leaf); browser_download_url = "$base/$(Split-Path $Installer -Leaf)" }) } |
        ConvertTo-Json -Depth 4 | Set-Content -Encoding utf8 "$dir\release.json"
    $server = Start-Process python -ArgumentList "-m", "http.server", "8766", "--bind", "127.0.0.1",
        "--directory", $dir -PassThru -WindowStyle Hidden
    Start-Sleep 2
    $env:TURBODM_TEST_RELEASE = "$base/release.json"
}
Start-Process $Exe
Remove-Item Env:\TURBODM_PRETEND_VERSION, Env:\TURBODM_TEST_RELEASE -ErrorAction SilentlyContinue
$shell = New-Object -ComObject WScript.Shell
$deadline = (Get-Date).AddSeconds(30)
while (-not $shell.AppActivate("TurboDM update")) {
    if ((Get-Date) -gt $deadline) { Shot "update-0-no-dialog.png"; throw "no update dialog" }
    Start-Sleep 1
}
Start-Sleep 1
Shot "update-1-dialog.png"
$shell.SendKeys("{ENTER}") # Update now
$clicked = Get-Date
Start-Sleep 3
Shot "update-2-downloading.png" # (the installer, downloaded by TurboDM in its progress window)
$deadline = (Get-Date).AddSeconds(180)
while ((Get-ItemProperty $key).DisplayVersion -ne $latest) {
    if ((Get-Date) -gt $deadline) {
        Shot "update-3-stuck.png"
        Get-ChildItem $env:TEMP -Filter "TurboDM*" | Format-Table Name, Length, LastWriteTime | Out-Host
        Get-Process turbodm, *setup* -ErrorAction SilentlyContinue | Format-Table Name, Id, StartTime | Out-Host
        Get-Content "$env:TEMP\TurboDM-update.log" -ErrorAction SilentlyContinue | Select-Object -Last 40 | Out-Host
        throw "still $((Get-ItemProperty $key).DisplayVersion) after updating"
    }
    Start-Sleep 2
}
Write-Host "updated to $latest"
Start-Sleep 8
$reopened = Get-Process turbodm -ErrorAction SilentlyContinue | Where-Object { $_.StartTime -gt $clicked }
if (-not $reopened) { throw "TurboDM wasn't reopened" }
Write-Host "and TurboDM is running again"
Get-Process turbodm | Stop-Process
if ($server) { Stop-Process -Id $server.Id -ErrorAction SilentlyContinue }
