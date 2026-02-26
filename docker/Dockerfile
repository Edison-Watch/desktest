FROM debian:bookworm-slim

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
    xvfb \
    xfce4 \
    xfce4-terminal \
    x11vnc \
    xdotool \
    scrot \
    dbus-x11 \
    libfuse2 \
    fuse3 \
    file \
    procps \
    net-tools \
    curl \
    ca-certificates \
    libgtk-3-0 \
    libnotify4 \
    libnss3 \
    libxss1 \
    libasound2 \
    fonts-liberation \
    xdg-utils \
    # Python and PyAutoGUI dependencies
    python3 \
    python3-pyautogui \
    python3-xlib \
    # Accessibility tree support
    python3-pyatspi \
    at-spi2-core \
    libatspi2.0-0 \
    # Clipboard support
    python3-pyperclip \
    xclip \
    # Video recording
    ffmpeg \
    && apt-get clean && rm -rf /var/lib/apt/lists/*

# Create a non-root user for the desktop session
RUN useradd -m -s /bin/bash tester

# Install helper scripts
COPY get-a11y-tree.py /usr/local/bin/get-a11y-tree
COPY execute-action.py /usr/local/bin/execute-action
RUN chmod +x /usr/local/bin/get-a11y-tree /usr/local/bin/execute-action

# Startup script
COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

USER tester
WORKDIR /home/tester

ENTRYPOINT ["/entrypoint.sh"]
