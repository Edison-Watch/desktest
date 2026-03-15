#!/usr/bin/env bash
set -euo pipefail

DISPLAY_WIDTH="${DISPLAY_WIDTH:-1280}"
DISPLAY_HEIGHT="${DISPLAY_HEIGHT:-800}"
VNC_PORT="${VNC_PORT:-5900}"

export DISPLAY=:99

# Create .Xauthority so python3-xlib (pip) does not crash
touch "$HOME/.Xauthority"

# Start Xvfb
Xvfb :99 -screen 0 "${DISPLAY_WIDTH}x${DISPLAY_HEIGHT}x24" -ac &
sleep 1

# Start dbus session
eval "$(dbus-launch --sh-syntax)"
export DBUS_SESSION_BUS_ADDRESS

# Start AT-SPI2 accessibility registry daemon
/usr/libexec/at-spi2-registryd &

# Start XFCE session
xfce4-session &
sleep 2

# Start VNC server (no password, shared mode)
x11vnc -display :99 -forever -shared -nopw -rfbport "$VNC_PORT" &

# Write a sentinel file when desktop is up
touch /tmp/.desktop-ready

# Keep container alive
exec sleep infinity
