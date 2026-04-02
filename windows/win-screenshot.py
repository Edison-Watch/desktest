#!/usr/bin/env python3
"""Capture a screenshot on Windows using Pillow's ImageGrab.

Usage: python win-screenshot.py [output_path]

Default output path: C:\\Temp\\screenshot.png
"""

import sys

try:
    from PIL import ImageGrab
except ImportError:
    print("ERROR: Pillow not available (pip install Pillow)", file=sys.stderr)
    sys.exit(1)


def main():
    output_path = sys.argv[1] if len(sys.argv) > 1 else r"C:\Temp\screenshot.png"
    screenshot = ImageGrab.grab()
    screenshot.save(output_path, "PNG")


if __name__ == "__main__":
    main()
