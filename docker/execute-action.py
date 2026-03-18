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
import pathlib
pathlib.Path(os.path.expanduser("~/.Xauthority")).touch(exist_ok=True)

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
    """Remove dangerous attributes from a module before passing to exec namespace.

    Strips __loader__ (which exposes load_module() for arbitrary imports) and
    __spec__.loader. This is hardening — not a complete sandbox. Attribute
    traversal via __class__.__mro__ can still reach dangerous classes; full
    prevention requires RestrictedPython or process-level isolation.
    """
    import types
    # Create a shallow wrapper module to avoid mutating the real module
    wrapper = types.ModuleType(mod.__name__)
    for attr in dir(mod):
        if attr in ("__loader__", "__spec__"):
            continue
        try:
            setattr(wrapper, attr, getattr(mod, attr))
        except (AttributeError, TypeError):
            pass
    return wrapper


def _safe_builtins():
    """Return a restricted dict of Python builtins safe for LLM-generated code.

    This is defense-in-depth hardening, NOT a full sandbox. CPython's exec()
    cannot be fully sandboxed — attribute traversal (e.g. "".__class__.__mro__)
    can reach arbitrary classes regardless of __builtins__ restrictions. The
    Docker container (non-root "tester" user) remains the primary security
    boundary. This allowlist raises the bar by removing the most obvious
    escape vectors (direct __import__, open, eval, etc.).
    """
    import builtins

    allowed = [
        # Output
        "print",
        # Type constructors & conversions
        "int", "float", "str", "bool", "list", "dict", "tuple", "set",
        "frozenset", "bytes", "bytearray", "complex",
        # Iteration & sequences
        "len", "range", "enumerate", "zip", "sorted", "reversed",
        "min", "max", "sum", "any", "all", "map", "filter",
        "iter", "next", "slice",
        # Math
        "abs", "round", "pow", "divmod",
        # String/char utilities
        "repr", "format", "chr", "ord", "hex", "oct", "bin",
        # Introspection (safe subset)
        "isinstance", "issubclass", "callable", "hash", "id",
        # OOP primitives (object excluded — __subclasses__() enables sandbox escape)
        "super", "property", "staticmethod", "classmethod",
        # Exception types
        "Exception", "ValueError", "TypeError", "AttributeError",
        "KeyError", "IndexError", "RuntimeError", "StopIteration",
        "NotImplementedError", "ArithmeticError", "ZeroDivisionError",
        "OverflowError", "NameError", "OSError", "IOError",
    ]
    return {name: getattr(builtins, name) for name in allowed if hasattr(builtins, name)}


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

    # Sanitize modules to remove __loader__ (prevents load_module() import bypass).
    # This is hardening — see _safe_builtins() docstring for full threat model.
    namespace = {
        "pyautogui": _sanitize_module(pyautogui),
        "time": _sanitize_module(time),
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
