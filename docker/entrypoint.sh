#!/usr/bin/env bash
set -euo pipefail

DISPLAY_WIDTH="${DISPLAY_WIDTH:-1280}"
DISPLAY_HEIGHT="${DISPLAY_HEIGHT:-800}"
VNC_PORT="${VNC_PORT:-5900}"

export DISPLAY=:99

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

# Start VNC server
if [ -n "${VNC_PASSWORD:-}" ]; then
    x11vnc -storepasswd "$VNC_PASSWORD" /tmp/.vnc_passwd
    chmod 600 /tmp/.vnc_passwd
    x11vnc -display :99 -forever -shared -rfbauth /tmp/.vnc_passwd -rfbport "$VNC_PORT" &
else
    x11vnc -display :99 -forever -shared -nopw -rfbport "$VNC_PORT" &
fi

# Write a sentinel file when desktop is up
touch /tmp/.desktop-ready

# Keep container alive
exec sleep infinity
