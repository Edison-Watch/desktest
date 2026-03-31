#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
AGENT_SRC="${SCRIPT_DIR}/vm-agent.py"
INSTALL_PATH="/usr/local/bin/desktest-vm-agent.py"
LAUNCH_AGENT="${HOME}/Library/LaunchAgents/dev.desktest.vm-agent.plist"
SHARED_DIR="${1:-/Volumes/My Shared Files/desktest}"
PYTHON_BIN="${PYTHON_BIN:-$(command -v python3)}"

mkdir -p "$(dirname "${INSTALL_PATH}")"
cp "${AGENT_SRC}" "${INSTALL_PATH}"
chmod 755 "${INSTALL_PATH}"

mkdir -p "$(dirname "${LAUNCH_AGENT}")"
cat > "${LAUNCH_AGENT}" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>dev.desktest.vm-agent</string>
  <key>ProgramArguments</key>
  <array>
    <string>${PYTHON_BIN}</string>
    <string>${INSTALL_PATH}</string>
    <string>${SHARED_DIR}</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>EnvironmentVariables</key>
  <dict>
    <key>PATH</key>
    <string>/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
  </dict>
  <key>StandardOutPath</key>
  <string>/tmp/desktest-vm-agent.log</string>
  <key>StandardErrorPath</key>
  <string>/tmp/desktest-vm-agent.log</string>
</dict>
</plist>
PLIST

launchctl unload "${LAUNCH_AGENT}" >/dev/null 2>&1 || true
launchctl load "${LAUNCH_AGENT}"
echo "Installed desktest VM agent at ${INSTALL_PATH}"
