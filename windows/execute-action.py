#!/usr/bin/env python3
"""Execute PyAutoGUI code received via stdin on a Windows guest.

Reads Python code from stdin, executes it in a prepared namespace with
pyautogui, time, pyperclip, and type_text pre-imported.

Returns JSON to stdout: {success: bool, error: string|null, duration_ms: number}
"""

import json
import sys
import time

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


def _sanitize_module(mod):
    """Remove dangerous attributes from a module before passing to exec namespace."""
    import types
    wrapper = types.ModuleType(mod.__name__)
    for attr in dir(mod):
        if attr in ("__loader__", "__spec__", "__builtins__"):
            continue
        try:
            setattr(wrapper, attr, getattr(mod, attr))
        except (AttributeError, TypeError):
            pass
    return wrapper


def _safe_builtins():
    """Return a restricted dict of Python builtins safe for LLM-generated code."""
    import builtins

    allowed = [
        "print",
        "int", "float", "str", "bool", "list", "dict", "tuple", "set",
        "frozenset", "bytes", "bytearray", "complex",
        "len", "range", "enumerate", "zip", "sorted", "reversed",
        "min", "max", "sum", "any", "all", "map", "filter",
        "iter", "next", "slice",
        "abs", "round", "pow", "divmod",
        "repr", "format", "chr", "ord", "hex", "oct", "bin",
        "isinstance", "issubclass", "callable", "hash", "id",
        "Exception", "ValueError", "TypeError", "AttributeError",
        "KeyError", "IndexError", "RuntimeError", "StopIteration",
        "NotImplementedError", "ArithmeticError", "ZeroDivisionError",
        "OverflowError", "NameError", "OSError", "IOError",
    ]
    safe = {name: getattr(builtins, name) for name in allowed if hasattr(builtins, name)}

    _allowed_modules = frozenset({
        "time", "pyautogui", "pyperclip", "math", "random", "string",
        "re", "json", "collections", "itertools", "functools", "datetime",
    })
    _real_import = builtins.__import__

    def _restricted_import(name, globals=None, locals=None, fromlist=(), level=0):
        top_level = name.split('.')[0]
        if top_level not in _allowed_modules:
            raise ImportError(f"Import of '{name}' is not allowed in this sandbox")
        mod = _real_import(name, globals, locals, fromlist, level)
        return _sanitize_module(mod)

    safe["__import__"] = _restricted_import
    return safe


def _make_type_text():
    """Factory for type_text() using Win32 SendInput for reliable Unicode input."""
    import ctypes
    from ctypes import wintypes

    user32 = ctypes.windll.user32

    KEYEVENTF_UNICODE = 0x0004
    KEYEVENTF_KEYUP = 0x0002
    INPUT_KEYBOARD = 1

    class KEYBDINPUT(ctypes.Structure):
        _fields_ = [
            ("wVk", wintypes.WORD),
            ("wScan", wintypes.WORD),
            ("dwFlags", wintypes.DWORD),
            ("time", wintypes.DWORD),
            ("dwExtraInfo", ctypes.POINTER(ctypes.c_ulong)),
        ]

    class INPUT(ctypes.Structure):
        class _INPUT(ctypes.Union):
            _fields_ = [("ki", KEYBDINPUT)]
        _fields_ = [
            ("type", wintypes.DWORD),
            ("_input", _INPUT),
        ]

    def type_text(text):
        """Type text using Win32 SendInput with Unicode events.

        Each character is sent as a KEYEVENTF_UNICODE key-down + key-up pair.
        This handles the full Unicode range including special characters.
        """
        for char in text:
            code = ord(char)
            # Key down
            ki_down = KEYBDINPUT(
                wVk=0,
                wScan=code,
                dwFlags=KEYEVENTF_UNICODE,
                time=0,
                dwExtraInfo=None,
            )
            inp_down = INPUT(type=INPUT_KEYBOARD)
            inp_down._input.ki = ki_down

            # Key up
            ki_up = KEYBDINPUT(
                wVk=0,
                wScan=code,
                dwFlags=KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                time=0,
                dwExtraInfo=None,
            )
            inp_up = INPUT(type=INPUT_KEYBOARD)
            inp_up._input.ki = ki_up

            user32.SendInput(2, (INPUT * 2)(inp_down, inp_up), ctypes.sizeof(INPUT))

    return type_text


# type_text is only available on Windows — gracefully degrade on other platforms
try:
    _type_text = _make_type_text()
except (AttributeError, OSError):
    def _type_text(text):
        raise RuntimeError("type_text() requires Windows (Win32 SendInput)")


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
        "pyautogui": _sanitize_module(pyautogui),
        "time": _sanitize_module(time),
        "type_text": _type_text,
        "__builtins__": _safe_builtins(),
    }
    if pyperclip is not None:
        namespace["pyperclip"] = _sanitize_module(pyperclip)

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
