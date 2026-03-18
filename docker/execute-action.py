#!/usr/bin/env python3
"""Execute PyAutoGUI code received via stdin and return structured JSON result.

Reads Python code from stdin, executes it in a prepared namespace with
pyautogui, time, and pyperclip pre-imported.

Returns JSON to stdout: {success: bool, error: string|null, duration_ms: number}
"""

import json
import os
import sys
import time

os.environ.setdefault("DISPLAY", ":99")

# Ensure ~/.Xauthority exists — python-xlib crashes without it.
_xauth = os.path.expanduser("~/.Xauthority")
if not os.path.exists(_xauth):
    open(_xauth, "a").close()

try:
    import pyautogui
    pyautogui.FAILSAFE = False
    pyautogui.PAUSE = 0.1
except ImportError:
    result = {
        "success": False,
        "error": "pyautogui not available",
        "duration_ms": 0,
    }
    print(json.dumps(result))
    sys.exit(0)

try:
    import pyperclip
except ImportError:
    pyperclip = None


def main():
    code = sys.stdin.read()
    if not code.strip():
        result = {
            "success": True,
            "error": None,
            "duration_ms": 0,
        }
        print(json.dumps(result))
        return

    namespace = {
        "pyautogui": pyautogui,
        "time": time,
        "__builtins__": __builtins__,
    }
    if pyperclip is not None:
        namespace["pyperclip"] = pyperclip

    start = time.monotonic()
    try:
        exec(code, namespace)
        duration_ms = int((time.monotonic() - start) * 1000)
        result = {
            "success": True,
            "error": None,
            "duration_ms": duration_ms,
        }
    except Exception as e:
        duration_ms = int((time.monotonic() - start) * 1000)
        result = {
            "success": False,
            "error": f"{type(e).__name__}: {e}",
            "duration_ms": duration_ms,
        }

    print(json.dumps(result))


if __name__ == "__main__":
    main()
