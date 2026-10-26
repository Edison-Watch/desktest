#!/usr/bin/env python3
"""Fail CI if a Dockerfile or GitHub Actions file uses a mutable reference.

Tags (``node:24-slim``, ``:latest``, ``actions/checkout@v6``) can be moved to
point at different content after we review them, which is how tag-rewrite
supply-chain attacks land. Every external reference must therefore be pinned
to content it cannot change:

- Dockerfile ``FROM`` / ``COPY --from=`` / ``RUN --mount=...,from=`` images:
  ``image:tag@sha256:<digest>``
  (a ``$VAR`` is resolved from its ``ARG`` default, which must itself be pinned)
- Workflow / ``action.yml`` ``uses:``: ``owner/repo@<40-hex commit sha>``
- Workflow ``image:`` / scalar ``container:``: ``image:tag@sha256:<digest>``
  (a ``${{ }}`` expression cannot be verified, so it fails closed)
- Action inputs such as ``bun-version: latest``: name an exact version

Keep the human-readable tag next to the pin (``node:24.11.1-slim@sha256:...``,
``actions/checkout@<sha> # v6.0.3``) so updates stay reviewable.

A reference that genuinely cannot be pinned (an image built locally by the
same repo) is allowed when the line before it is ``# pin-exempt: <reason>``.
The exemption covers images only: an action ``uses:`` always needs a sha.
"""

from __future__ import annotations

import os
import re
import subprocess
import sys
from pathlib import PurePosixPath

DIGEST = re.compile(r"@sha256:[0-9a-f]{64}$")
COMMIT_SHA = re.compile(r"^[^@\s/]+/[^@\s]+@[0-9a-f]{40}$")  # owner/repo[/path]@<sha>
INSTRUCTION = re.compile(r"^\s*(?P<cmd>[A-Za-z]+)\s+(?P<args>.*)$")
ARG_DEFAULT = re.compile(r"^(?P<name>[A-Za-z_][A-Za-z0-9_]*)=(?P<value>\S*)$")
VARIABLE = re.compile(
    r"\$\{(?P<braced>[A-Za-z_][A-Za-z0-9_]*)\}|\$(?P<bare>[A-Za-z_][A-Za-z0-9_]*)"
)
COPY_FROM = re.compile(r"--from=(?P<ref>\S+)", re.IGNORECASE)
MOUNT_FROM = re.compile(r"--mount=\S*?\bfrom=(?P<ref>[^\s,]+)", re.IGNORECASE)
# A YAML key, optionally quoted, with optional space before the colon. Keys after `{`
# or `,` count only on a flow-collection line (`- { uses: ... }`, `steps: [{ ... }]`),
# so a brace or comma inside a `run:` string is not mistaken for a key.
BLOCK_KEY = r"""^\s*(?:-\s*)?["']?"""
FLOW_KEY = r"""(?:^\s*(?:-\s*)?|[{,]\s*)["']?"""
FLOW_LINE = re.compile(r"""^\s*(?:-\s*)?(?:["']?[\w.-]+["']?\s*:\s*)?[\[{]""")
YAML_VALUE = r"""["']?\s*:\s*(?P<ref>[^\s#,{}]+)"""


def key_patterns(key: str) -> tuple[re.Pattern[str], re.Pattern[str], re.Pattern[str]]:
    return (
        re.compile(key + "uses" + YAML_VALUE),
        re.compile(key + "(?P<key>image|container)" + YAML_VALUE),
        re.compile(
            key + r"""(?P<key>[\w-]*version)["']?\s*:\s*["']?latest["']?\s*(?:[,}#].*)?$""",
            re.IGNORECASE,
        ),
    )


BLOCK_PATTERNS = key_patterns(BLOCK_KEY)
FLOW_PATTERNS = key_patterns(FLOW_KEY)
QUOTED_OR_COMMENT = re.compile(r""""(?:[^"\\]|\\.)*"|'(?:[^']|'')*'|\s#.*$""")


def flow_depth(line: str, depth: int) -> int:
    """Unclosed `{` / `[` after this line, ignoring quoted text and comments."""
    bare = QUOTED_OR_COMMENT.sub("", line)
    return max(0, depth + bare.count("{") + bare.count("[") - bare.count("}") - bare.count("]"))


# A `*version` key whose value starts on the next line: plain multiline or `|` / `>` block.
VERSION_OPEN_TAIL = r"""(?P<key>[\w-]*version)["']?\s*:\s*(?:[|>][-+0-9]*)?\s*(?:#.*)?$"""
VERSION_OPEN = re.compile(BLOCK_KEY + VERSION_OPEN_TAIL, re.IGNORECASE)
VERSION_OPEN_FLOW = re.compile(FLOW_KEY + VERSION_OPEN_TAIL, re.IGNORECASE)
EXEMPT = re.compile(r"^\s*#\s*pin-exempt:\s*\S")
PARSER_DIRECTIVE = re.compile(r"^#\s*(?P<name>[A-Za-z]+)\s*=\s*(?P<value>\S*)\s*$")


def list_files() -> list[str]:
    out = subprocess.run(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
        capture_output=True,
        check=True,
    ).stdout.decode()
    return [name for name in out.split("\x00") if name]


def is_dockerfile(path: str) -> bool:
    name = PurePosixPath(path).name.lower()
    if name.endswith(".dockerignore"):
        return False
    return name.startswith("dockerfile") or name.endswith(".dockerfile")


def is_actions_file(path: str) -> bool:
    p = PurePosixPath(path)
    if p.suffix not in {".yml", ".yaml"}:
        return False
    if p.parts[:2] == (".github", "workflows"):
        return True
    return p.stem == "action"  # composite / docker action metadata, wherever it lives


def mutable_image(ref: str) -> str | None:
    """Return why an image reference is mutable, or None if it is pinned."""
    ref = ref.strip("\"'")
    if DIGEST.search(ref):
        return None
    tag = ref.rsplit("/", 1)[-1].partition(":")[2]
    if not tag:
        return "untagged (implicitly :latest) and not pinned by digest"
    if tag == "latest":
        return ":latest is mutable"
    return "tag is mutable; append @sha256:<digest>"


def escape_char(lines: list[str]) -> str:
    """The line-continuation character set by a leading `# escape=` parser directive."""
    for raw in lines:
        m = PARSER_DIRECTIVE.match(raw.strip())
        if not m:
            break  # directives are only honoured before the first non-directive line
        if m["name"].lower() == "escape" and m["value"] in {"\\", "`"}:
            return m["value"]
    return "\\"


def instructions(lines: list[str]) -> list[tuple[int, bool, str]]:
    """Join continuation lines into (first lineno, pin-exempt, instruction text)."""
    escape = escape_char(lines)
    out: list[tuple[int, bool, str]] = []
    exempt = False
    buf: list[str] = []
    start = 0
    for lineno, raw in enumerate(lines, 1):
        stripped = raw.strip()
        if not buf:
            if not stripped:
                exempt = False  # an exemption covers only the instruction right after it
                continue
            if stripped.startswith("#"):
                exempt = bool(EXEMPT.match(stripped))
                continue
            start = lineno
        elif not stripped or stripped.startswith("#"):
            continue  # blank and comment lines inside a continuation are dropped by Docker
        # Docker drops only the escape and the newline, keeping the next line's indent
        part = stripped if not buf else raw.rstrip()
        if part.endswith(escape):
            buf.append(part[:-1])
            continue
        buf.append(part)
        out.append((start, exempt, "".join(buf)))
        buf, exempt = [], False
    if buf:
        out.append((start, exempt, "".join(buf)))
    return out


def resolve(ref: str, args: dict[str, str]) -> str | None:
    """Substitute ``$VAR`` / ``${VAR}`` from ARG defaults; None if any is unset."""
    unresolved = False

    def sub(m: re.Match[str]) -> str:
        nonlocal unresolved
        name = m["braced"] or m["bare"]
        if name not in args:
            unresolved = True
            return ""
        return args[name]

    out = VARIABLE.sub(sub, ref)
    return None if unresolved else out


def image_refs(cmd: str, rest: str, stages: set[str]) -> list[tuple[str, str]]:
    """External image refs a FROM/COPY instruction pulls, as (label, ref); records stages."""
    if cmd == "FROM":
        tokens = [t for t in rest.split() if not t.startswith("--")]
        if not tokens:
            return []
        image = tokens[0]
        refs = [] if image.lower() in {"scratch", *stages} else [("FROM ", image)]
        if len(tokens) >= 3 and tokens[1].lower() == "as":
            stages.add(tokens[2].lower())  # after the check: only earlier stages are aliases
        return refs
    if cmd in {"COPY", "RUN"}:
        pattern, label = (COPY_FROM, "--from=") if cmd == "COPY" else (MOUNT_FROM, "from=")
        refs = [m["ref"] for m in pattern.finditer(rest)]
        return [(label, r) for r in refs if not r.isdigit() and r.lower() not in stages]
    return []


def image_problem(ref: str, args: dict[str, str]) -> str | None:
    resolved = resolve(ref, args)
    if resolved is None:
        return (
            "build arg has no pinned default; "
            "give the ARG a digest-pinned default or add # pin-exempt"
        )
    return mutable_image(resolved)


def declare_args(rest: str, scope: dict[str, str], inherit: dict[str, str]) -> None:
    """Record an ARG instruction; a bare `ARG NAME` in a stage takes the global default."""
    for token in rest.split():
        if arg := ARG_DEFAULT.match(token):
            scope[arg["name"]] = arg["value"].strip("\"'")
        elif token in inherit:
            scope[token] = inherit[token]
        else:
            scope.pop(token, None)  # declared without a default: unresolved from here on


def check_dockerfile(path: str, lines: list[str]) -> list[str]:
    errors: list[str] = []
    stages: set[str] = set()
    # Docker scoping: FROM sees only ARGs declared before the first FROM; each stage
    # starts empty and sees a global value only by redeclaring it without a default.
    global_args: dict[str, str] = {}
    stage_args: dict[str, str] | None = None
    for lineno, exempt, text in instructions(lines):
        m = INSTRUCTION.match(text)
        if not m:
            continue
        cmd, rest = m["cmd"].upper(), m["args"]
        if cmd == "ARG":
            if stage_args is None:
                declare_args(rest, global_args, {})
            else:
                declare_args(rest, stage_args, global_args)
        scope = global_args if cmd == "FROM" else (stage_args or {})
        for label, ref in image_refs(cmd, rest, stages):
            if not exempt and (why := image_problem(ref, scope)):
                errors.append(f"{path}:{lineno}: {label}{ref}: {why}")
        if cmd == "FROM":
            stage_args = {}
    return errors


def uses_problem(ref: str) -> str | None:
    if ref.startswith("./"):
        return None
    if ref.startswith("docker://"):
        return mutable_image(ref.removeprefix("docker://"))
    return None if COMMIT_SHA.search(ref) else "pin to a 40-character commit sha"


def container_problem(key: str, ref: str, is_action_metadata: bool) -> str | None:
    if is_action_metadata and key == "image":
        # runs.image in action metadata: a local Dockerfile path, or an image
        # (docker://<image> or a bare name), which must be pinned like any other
        if ref.startswith("docker://"):
            ref = ref.removeprefix("docker://")
        elif ref.endswith(("Dockerfile", "dockerfile")) and not {":", "@"} & set(ref):
            # a path, by the runner's own rule; `:` or `@` makes it an image reference
            return None
    if "${{" in ref:
        return (
            "expression cannot be verified as pinned; "
            "use a digest-pinned literal or add # pin-exempt"
        )
    return mutable_image(ref)


def next_scalar_line(lines: list[str], lineno: int, key_line: str, in_flow: bool) -> str | None:
    """First line of a value that starts below its key, or None if no line continues it.

    Comments are dropped; inside a block scalar that can only over-match (fail closed).
    In a flow collection indentation does not matter and a closing `}` / `]` / `,` ends the value.
    """
    indent = len(key_line) - len(key_line.lstrip())
    for line in lines[lineno:]:
        value = line.strip()
        if not value or value.startswith("#"):
            continue
        if not in_flow and len(line) - len(line.lstrip()) <= indent:
            return None
        value = re.sub(r"\s+#.*$", "", value)
        if in_flow:
            value = re.split(r"\s*[,}\]]", value, maxsplit=1)[0]
        return value.strip().strip("\"'").lower()
    return None


def check_actions_file(path: str, lines: list[str]) -> list[str]:
    errors: list[str] = []
    is_action_metadata = PurePosixPath(path).stem == "action"
    depth = 0  # open flow collections, so continuation lines of a `{ ... }` keep flow parsing
    for lineno, line in enumerate(lines, 1):
        in_flow = depth > 0 or bool(FLOW_LINE.match(line))
        uses_re, image_re, latest_re = FLOW_PATTERNS if in_flow else BLOCK_PATTERNS
        depth = flow_depth(line, depth) if in_flow else 0
        # Every match counts: a flow-style line can hold several steps or keys.
        for m in uses_re.finditer(line):
            ref = m["ref"].strip("\"'")
            if why := uses_problem(ref):
                errors.append(f"{path}:{lineno}: uses {ref}: {why}")
        if (m := latest_re.search(line)) or (
            (m := (VERSION_OPEN_FLOW.search(line) if in_flow else VERSION_OPEN.match(line)))
            and next_scalar_line(lines, lineno, line, in_flow) == "latest"
        ):
            errors.append(f"{path}:{lineno}: {m['key']}: latest is mutable; name an exact version")
        if lineno > 1 and EXEMPT.match(lines[lineno - 2]):
            continue  # pin-exempt covers images only, so it is checked after uses
        for m in image_re.finditer(line):
            key, ref = m["key"], m["ref"].strip("\"'")
            if why := container_problem(key, ref, is_action_metadata):
                errors.append(f"{path}:{lineno}: {key} {ref}: {why}")
    return errors


def main() -> int:
    errors: list[str] = []
    for path in list_files():
        if is_dockerfile(path):
            checker = check_dockerfile
        elif is_actions_file(path):
            checker = check_actions_file
        else:
            continue
        if not os.path.exists(path):
            continue  # tracked but deleted in the working tree
        try:
            with open(path, encoding="utf-8") as fh:
                lines = fh.read().splitlines()
        except (OSError, UnicodeDecodeError) as exc:
            errors.append(f"{path}: could not be read ({exc}); cannot verify its references")
            continue
        errors.extend(checker(path, lines))

    if errors:
        print(
            "Mutable references found (pin images by digest, actions by commit sha):",
            file=sys.stderr,
        )
        for err in errors:
            print(f"  {err}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
