#!/usr/bin/env python3
"""Execute PyAutoGUI code received via stdin and return structured JSON result.

Reads Python code from stdin, executes it in a prepared namespace with
pyautogui, time, and pyperclip pre-imported.  Code is compiled through
RestrictedPython which rewrites attribute access, item access, and other
operations to pass through guard functions — blocking __globals__,
__class__.__mro__, __subclasses__(), and similar escape vectors at the
AST level.

Returns JSON to stdout: {success: bool, error: string|null, duration_ms: number}
"""

import json
import os
import sys
import time

try:
    from RestrictedPython import compile_restricted
    from RestrictedPython.Eval import default_guarded_getiter
    from RestrictedPython.Guards import (
        guarded_unpack_sequence,
        safer_getattr,
    )
    HAS_RESTRICTED_PYTHON = True
except ImportError:
    HAS_RESTRICTED_PYTHON = False

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

    Strips __loader__ (which exposes load_module() for arbitrary imports),
    __spec__.loader, and __builtins__. This is hardening — not a complete sandbox.

    When RestrictedPython is available, attribute access is guarded via
    safer_getattr which blocks dunder attributes (__globals__, __class__,
    __mro__, __subclasses__, etc.) at the AST level.  Module sanitization
    remains as a secondary defense layer.
    """
    import types
    # Create a shallow wrapper module to avoid mutating the real module
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
    """Return a restricted dict of Python builtins safe for LLM-generated code.

    When RestrictedPython is available, this works alongside compile_restricted()
    and safer_getattr to provide AST-level blocking of dunder attribute traversal.
    Without RestrictedPython, this is defense-in-depth hardening only — CPython's
    exec() cannot be fully sandboxed via builtins restrictions alone.
    The Docker container (non-root "tester" user, cap_drop ALL,
    no-new-privileges) remains an additional security boundary.
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
        # OOP: class definitions are intentionally blocked (__build_class__ excluded)
        # to reduce attack surface. super/property/staticmethod/classmethod removed
        # since they are unusable without class definitions.
        # Exception types
        "Exception", "ValueError", "TypeError", "AttributeError",
        "KeyError", "IndexError", "RuntimeError", "StopIteration",
        "NotImplementedError", "ArithmeticError", "ZeroDivisionError",
        "OverflowError", "NameError", "OSError", "IOError",
    ]
    safe = {name: getattr(builtins, name) for name in allowed if hasattr(builtins, name)}

    # Provide a restricted __import__ so LLM-generated `import time` etc. still
    # work, while blocking `import os`, `import subprocess`, etc.
    _allowed_modules = frozenset({
        "time", "pyautogui", "pyperclip", "math", "random", "string",
        "re", "json", "collections", "itertools", "functools", "datetime",
    })
    _real_import = builtins.__import__

    def _restricted_import(name, globals=None, locals=None, fromlist=(), level=0):
        # Check top-level module so dotted imports like `import collections.abc`
        # work when `collections` is in the allowlist.
        top_level = name.split('.')[0]
        if top_level not in _allowed_modules:
            raise ImportError(f"Import of '{name}' is not allowed in this sandbox")
        mod = _real_import(name, globals, locals, fromlist, level)
        # Sanitize returned modules to prevent __builtins__ leakage via re-import.
        # `import X` (fromlist=()) returns the module directly; `from X import Y`
        # (non-empty fromlist) also returns the module, so sanitize in both cases.
        return _sanitize_module(mod)

    safe["__import__"] = _restricted_import
    return safe


def _make_type_text():
    """Factory that returns a type_text closure whose __globals__ doesn't
    include the script's os/sys/pathlib imports — defense-in-depth against
    sandbox escape via type_text.__globals__['os']."""
    import subprocess as _sp

    def type_text(text, delay_ms=12):
        """Type text reliably using xdotool, handling special characters and Unicode.

        Exposed as type_text() in the sandbox namespace. Bypasses PyAutoGUI's
        typewrite() limitations (which can't handle @, (, ), \\, #, !, etc.)
        by calling xdotool type --clearmodifiers directly.

        Args:
            text: The string to type. Supports full UTF-8.
            delay_ms: Delay between keystrokes in milliseconds (default 12).
        """
        delay_ms = max(0, int(delay_ms))
        estimated_s = len(text) * delay_ms / 1000
        effective_timeout = max(30, estimated_s + 5)
        _sp.run(
            ["xdotool", "type", "--clearmodifiers", "--delay", str(delay_ms), "--", text],
            check=True,
            timeout=effective_timeout,
            stdout=_sp.DEVNULL,
            stderr=_sp.PIPE,
        )

    return type_text


_type_text = _make_type_text()


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
    namespace = {
        "pyautogui": _sanitize_module(pyautogui),
        "time": _sanitize_module(time),
        "type_text": _type_text,
        "__builtins__": _safe_builtins(),
    }
    if pyperclip is not None:
        namespace["pyperclip"] = _sanitize_module(pyperclip)

    # RestrictedPython guard functions — these are injected into the namespace
    # so that the rewritten AST can call them at runtime.
    if HAS_RESTRICTED_PYTHON:
        namespace["_getattr_"] = safer_getattr
        namespace["_getiter_"] = default_guarded_getiter
        namespace["_getitem_"] = lambda obj, key: obj[key]
        namespace["_unpack_sequence_"] = guarded_unpack_sequence
        namespace["_iter_unpack_sequence_"] = guarded_unpack_sequence
        # _write_ guard: RestrictedPython calls _write_(obj) before .append(),
        # .extend(), etc. We allow all writes since the container is the
        # isolation boundary — we only need to block attribute traversal escapes.
        namespace["_inplacevar_"] = lambda op, x, y: op(x, y)

        def _default_write_(obj):
            return obj

        namespace["_write_"] = _default_write_

    start = time.monotonic()
    try:
        if HAS_RESTRICTED_PYTHON:
            compiled = compile_restricted(code, "<action>", "exec")
            exec(compiled, namespace)
        else:
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
