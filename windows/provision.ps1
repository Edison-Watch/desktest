#Requires -RunAsAdministrator
<#
.SYNOPSIS
    Provision a Windows 11 golden image for desktest QEMU/KVM testing.

.DESCRIPTION
    Run by `desktest init-windows` during Stage 2 (SSH provisioning).
    Installs Python 3, PyAutoGUI, Pillow, uiautomation, WinFsp, deploys
    agent scripts, registers the VM agent as a scheduled task, and applies
    system tweaks for unattended desktop testing.

    This script assumes:
      - Running as the "tester" user with Administrator privileges
      - OpenSSH Server is enabled (set up during Stage 1 / Autounattend)
      - Internet access is available for downloading packages
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

Write-Host "=== desktest Windows golden image provisioning ==="

# --------------------------------------------------------------------------
# 1. Install Python 3
# --------------------------------------------------------------------------
Write-Host "`n--- Installing Python 3 ---"
$pythonVersion = "3.12.8"
$pythonInstaller = "$env:TEMP\python-installer.exe"
$pythonUrl = "https://www.python.org/ftp/python/$pythonVersion/python-$pythonVersion-amd64.exe"

Write-Host "Downloading Python $pythonVersion..."
Invoke-WebRequest -Uri $pythonUrl -OutFile $pythonInstaller -UseBasicParsing

Write-Host "Installing Python (silent)..."
Start-Process -Wait -FilePath $pythonInstaller -ArgumentList @(
    "/quiet",
    "InstallAllUsers=1",
    "PrependPath=1",
    "Include_pip=1",
    "Include_launcher=1"
)
Remove-Item $pythonInstaller -Force

# Refresh PATH for this session
$env:Path = [System.Environment]::GetEnvironmentVariable("Path", "Machine") + ";" +
            [System.Environment]::GetEnvironmentVariable("Path", "User")

# Verify Python
$pythonExe = (Get-Command python -ErrorAction SilentlyContinue).Source
if (-not $pythonExe) {
    Write-Error "Python installation failed — python not found in PATH"
    exit 1
}
Write-Host "Python installed: $pythonExe"
python --version

# --------------------------------------------------------------------------
# 2. Install Python packages
# --------------------------------------------------------------------------
Write-Host "`n--- Installing Python packages ---"
python -m pip install --upgrade pip
python -m pip install pyautogui Pillow uiautomation pyperclip

# Verify key packages
python -c "import pyautogui; print(f'PyAutoGUI {pyautogui.__version__}')"
python -c "import uiautomation; print('uiautomation OK')"

# --------------------------------------------------------------------------
# 3. Install WinFsp (required for VirtIO-FS shared directory)
# --------------------------------------------------------------------------
Write-Host "`n--- Installing WinFsp ---"
$winfspVersion = "2.0.23075"
$winfspMsi = "$env:TEMP\winfsp.msi"
$winfspUrl = "https://github.com/winfsp/winfsp/releases/download/v2.0/winfsp-$winfspVersion.msi"

Write-Host "Downloading WinFsp $winfspVersion..."
Invoke-WebRequest -Uri $winfspUrl -OutFile $winfspMsi -UseBasicParsing

Write-Host "Installing WinFsp (silent)..."
Start-Process -Wait msiexec -ArgumentList "/i `"$winfspMsi`" /qn /norestart"
Remove-Item $winfspMsi -Force
Write-Host "WinFsp installed."

# --------------------------------------------------------------------------
# 4. Configure VirtIO-FS to mount as Z:\ on boot
# --------------------------------------------------------------------------
Write-Host "`n--- Configuring VirtIO-FS mount ---"

# The VirtIO-FS driver (viofs) was installed during Stage 1 via Autounattend.
# WinFsp.Launcher maps the VirtIO-FS tag to a drive letter.
# Register the "desktest" tag to mount as Z:\ via WinFsp.Launcher service.

$winfspLauncherDir = "HKLM:\SOFTWARE\WOW6432Node\WinFsp\Services\desktest"
if (-not (Test-Path $winfspLauncherDir)) {
    New-Item -Path $winfspLauncherDir -Force | Out-Null
}
# WinFsp.Launcher service configuration for VirtIO-FS
Set-ItemProperty -Path $winfspLauncherDir -Name "Executable" -Value "C:\Program Files\VirtIO-FS\virtiofs.exe"
Set-ItemProperty -Path $winfspLauncherDir -Name "CommandLine" -Value "-t desktest -m Z:"
Set-ItemProperty -Path $winfspLauncherDir -Name "Security" -Value "D:P(A;;RPWPLC;;;WD)"
Set-ItemProperty -Path $winfspLauncherDir -Name "JobControl" -Value 1 -Type DWord

# Ensure WinFsp.Launcher service starts automatically
Set-Service -Name "WinFsp.Launcher" -StartupType Automatic -ErrorAction SilentlyContinue
Start-Service -Name "WinFsp.Launcher" -ErrorAction SilentlyContinue

Write-Host "VirtIO-FS configured to mount tag 'desktest' as Z:\"

# --------------------------------------------------------------------------
# 5. Deploy agent scripts to C:\desktest
# --------------------------------------------------------------------------
Write-Host "`n--- Deploying agent scripts ---"

# Scripts are SCP'd to C:\Temp\desktest-provision\ by the host before this
# script runs. Copy them to C:\desktest\.
$provisionSrc = "C:\Temp\desktest-provision"
$desktestDir = "C:\desktest"

if (-not (Test-Path $desktestDir)) {
    New-Item -ItemType Directory -Path $desktestDir -Force | Out-Null
}

$scripts = @("vm-agent.py", "execute-action.py", "get-a11y-tree.py", "win-screenshot.py")
foreach ($script in $scripts) {
    $src = Join-Path $provisionSrc $script
    if (Test-Path $src) {
        Copy-Item $src -Destination $desktestDir -Force
        Write-Host "  Deployed $script"
    } else {
        Write-Warning "  $script not found at $src"
    }
}

# --------------------------------------------------------------------------
# 6. Register vm-agent as a Scheduled Task (runs at logon)
# --------------------------------------------------------------------------
Write-Host "`n--- Registering vm-agent scheduled task ---"

$taskAction = New-ScheduledTaskAction `
    -Execute "python" `
    -Argument "C:\desktest\vm-agent.py Z:\"

$taskTrigger = New-ScheduledTaskTrigger -AtLogOn -User "tester"

$taskSettings = New-ScheduledTaskSettingsSet `
    -AllowStartIfOnBatteries `
    -DontStopIfGoingOnBatteries `
    -StartWhenAvailable `
    -ExecutionTimeLimit ([TimeSpan]::Zero)

Register-ScheduledTask `
    -TaskName "DesktestVMAgent" `
    -Action $taskAction `
    -Trigger $taskTrigger `
    -Settings $taskSettings `
    -RunLevel Highest `
    -User "tester" `
    -Password "desktest" `
    -Force

Write-Host "vm-agent registered as DesktestVMAgent (ONLOGON, HIGHEST privilege)"

# --------------------------------------------------------------------------
# 7. Disable UAC (belt and suspenders — also done in Autounattend)
# --------------------------------------------------------------------------
Write-Host "`n--- Disabling UAC ---"
Set-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System" `
    -Name "ConsentPromptBehaviorAdmin" -Value 0 -Type DWord
Set-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System" `
    -Name "EnableLUA" -Value 0 -Type DWord
Write-Host "UAC disabled."

# --------------------------------------------------------------------------
# 8. Disable Windows Defender real-time monitoring
# --------------------------------------------------------------------------
Write-Host "`n--- Disabling Windows Defender real-time monitoring ---"
try {
    Set-MpPreference -DisableRealtimeMonitoring $true
    Write-Host "Defender real-time monitoring disabled."
} catch {
    Write-Warning "Could not disable Defender: $_"
}

# --------------------------------------------------------------------------
# 9. Disable screensaver and screen lock
# --------------------------------------------------------------------------
Write-Host "`n--- Disabling screensaver and screen lock ---"
Set-ItemProperty -Path "HKCU:\Control Panel\Desktop" -Name "ScreenSaveActive" -Value "0"
Set-ItemProperty -Path "HKCU:\Control Panel\Desktop" -Name "ScreenSaveTimeOut" -Value "0"
# Disable lock screen
reg add "HKLM\SOFTWARE\Policies\Microsoft\Windows\Personalization" /v NoLockScreen /t REG_DWORD /d 1 /f | Out-Null
Write-Host "Screensaver and lock screen disabled."

# --------------------------------------------------------------------------
# 10. Configure auto-login (belt and suspenders — also in Autounattend)
# --------------------------------------------------------------------------
Write-Host "`n--- Configuring auto-login ---"
$winlogonPath = "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon"
Set-ItemProperty -Path $winlogonPath -Name "AutoAdminLogon" -Value "1"
Set-ItemProperty -Path $winlogonPath -Name "DefaultUserName" -Value "tester"
Set-ItemProperty -Path $winlogonPath -Name "DefaultPassword" -Value "desktest"
Write-Host "Auto-login configured for tester."

# --------------------------------------------------------------------------
# 11. Disable Windows Update
# --------------------------------------------------------------------------
Write-Host "`n--- Disabling Windows Update ---"
try {
    Stop-Service wuauserv -Force -ErrorAction SilentlyContinue
    Set-Service wuauserv -StartupType Disabled
    Write-Host "Windows Update service disabled."
} catch {
    Write-Warning "Could not disable Windows Update: $_"
}

# --------------------------------------------------------------------------
# 12. Set display resolution to 1920x1080
# --------------------------------------------------------------------------
Write-Host "`n--- Setting display resolution ---"
try {
    Set-DisplayResolution -Width 1920 -Height 1080 -Force -ErrorAction SilentlyContinue
    Write-Host "Resolution set to 1920x1080."
} catch {
    Write-Warning "Could not set resolution (will be configured at boot): $_"
}

# --------------------------------------------------------------------------
# 13. Power plan: high performance, no sleep
# --------------------------------------------------------------------------
Write-Host "`n--- Configuring power plan ---"
powercfg /change monitor-timeout-ac 0
powercfg /change monitor-timeout-dc 0
powercfg /change standby-timeout-ac 0
powercfg /change standby-timeout-dc 0
powercfg /change hibernate-timeout-ac 0
powercfg /setactive 8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c  # High Performance
Write-Host "Power plan set to High Performance, sleep disabled."

# --------------------------------------------------------------------------
# 14. Ensure C:\Temp exists
# --------------------------------------------------------------------------
if (-not (Test-Path "C:\Temp")) {
    New-Item -ItemType Directory -Path "C:\Temp" -Force | Out-Null
}

# --------------------------------------------------------------------------
# Done — shut down to finalize the golden image
# --------------------------------------------------------------------------
Write-Host "`n=== Provisioning complete ==="
Write-Host "Shutting down in 5 seconds..."
shutdown /s /t 5 /c "desktest: Stage 2 (provisioning) complete"
