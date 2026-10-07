# The in-app update, end to end on a real Windows desktop (CI): the installed TurboDM pretends
# to be old, finds the latest release on GitHub, and "Update now" must install it and reopen it.
param([string]$Exe = "$env:ProgramFiles\TurboDM\bin\turbodm.exe")
$ErrorActionPreference = "Stop"
$key = "HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{6B0C7A52-3E0B-4B7E-9C1D-7A4D2F1B9E21}_is1"
$latest = (Invoke-RestMethod https://api.github.com/repos/xsobhi/turbodm/releases/latest).tag_name.TrimStart("v")
$before = (Get-ItemProperty $key).DisplayVersion
Write-Host "installed $before, latest release $latest"
Get-Process turbodm -ErrorAction SilentlyContinue | Stop-Process
$env:TURBODM_PRETEND_VERSION = "0.0.1"
Start-Process $Exe
Remove-Item Env:\TURBODM_PRETEND_VERSION
$shell = New-Object -ComObject WScript.Shell
$deadline = (Get-Date).AddSeconds(30)
while (-not $shell.AppActivate("TurboDM update")) {
    if ((Get-Date) -gt $deadline) { throw "no update dialog" }
    Start-Sleep 1
}
Start-Sleep 1
$shell.SendKeys("{ENTER}") # Update now
$deadline = (Get-Date).AddSeconds(180)
while ((Get-ItemProperty $key).DisplayVersion -ne $latest) {
    if ((Get-Date) -gt $deadline) {
        Get-Content "$env:TEMP\TurboDM-update.log" -ErrorAction SilentlyContinue | Select-Object -Last 40
        throw "still $((Get-ItemProperty $key).DisplayVersion) after updating"
    }
    Start-Sleep 2
}
Write-Host "updated to $latest"
Start-Sleep 8
if (-not (Get-Process turbodm -ErrorAction SilentlyContinue)) { throw "TurboDM wasn't reopened" }
Write-Host "and TurboDM is running again"
Get-Process turbodm | Stop-Process
