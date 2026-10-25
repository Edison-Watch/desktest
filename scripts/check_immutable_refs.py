#!/usr/bin/env python3
"""Fail CI if a Dockerfile or GitHub Actions file uses a mutable reference.

Tags (``node:24-slim``, ``:latest``, ``actions/checkout@v6``) can be moved to
point at different content after we review them, which is how tag-rewrite
supply-chain attacks land. Every external reference must therefore be pinned
to content it cannot change:

- Dockerfile ``FROM`` / ``COPY --from=`` images: ``image:tag@sha256:<digest>``
- Workflow / composite-action ``uses:``: ``owner/repo@<40-hex commit sha>``
- Workflow ``image:`` (container / services): ``image:tag@sha256:<digest>``

Keep the human-readable tag next to the pin (``node:24.11.1-slim@sha256:...``,
``actions/checkout@<sha> # v6.0.3``) so updates stay reviewable.

A reference that genuinely cannot be pinned (an image built locally by the
same repo) is allowed when the line before it is ``# pin-exempt: <reason>``.
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import PurePosixPath

DIGEST = re.compile(r"@sha256:[0-9a-f]{64}$")
COMMIT_SHA = re.compile(r"@[0-9a-f]{40}$")
FROM_LINE = re.compile(r"^\s*FROM\s+(?P<args>.+)$", re.IGNORECASE)
COPY_FROM = re.compile(r"--from=(?P<ref>\S+)", re.IGNORECASE)
USES = re.compile(r"^\s*(?:-\s*)?uses:\s*(?P<ref>[^\s#]+)")
IMAGE = re.compile(r"^\s*(?:-\s*)?image:\s*(?P<ref>[^\s#]+)")
EXEMPT = re.compile(r"^\s*#\s*pin-exempt:\s*\S")


def list_files() -> list[str]:
    out = subprocess.run(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
        capture_output=True,
        check=True,
    ).stdout.decode()
    return [name for name in out.split("\x00") if name]


def is_dockerfile(path: str) -> bool:
    name = PurePosixPath(path).name
    if name.endswith(".dockerignore"):
        return False
    return name.startswith("Dockerfile") or name.endswith(".Dockerfile")


def is_actions_file(path: str) -> bool:
    p = PurePosixPath(path)
    if p.suffix not in {".yml", ".yaml"}:
        return False
    if p.parts[:2] == (".github", "workflows"):
        return True
    return p.parts[:2] == (".github", "actions") and p.stem == "action"


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


def check_dockerfile(path: str, lines: list[str]) -> list[str]:
    errors: list[str] = []
    stages: set[str] = set()
    for lineno, line in enumerate(lines, 1):
        exempt = lineno > 1 and bool(EXEMPT.match(lines[lineno - 2]))
        if m := FROM_LINE.match(line):
            tokens = [t for t in m["args"].split() if not t.startswith("--")]
            image = tokens[0]
            if len(tokens) >= 3 and tokens[1].lower() == "as":
                stages.add(tokens[2].lower())
            if exempt or image.lower() in {"scratch", *stages} or "$" in image:
                continue
            if why := mutable_image(image):
                errors.append(f"{path}:{lineno}: FROM {image}: {why}")
        for m in COPY_FROM.finditer(line):
            ref = m["ref"]
            if exempt or ref.isdigit() or ref.lower() in stages or "$" in ref:
                continue
            if why := mutable_image(ref):
                errors.append(f"{path}:{lineno}: --from={ref}: {why}")
    return errors


def check_actions_file(path: str, lines: list[str]) -> list[str]:
    errors: list[str] = []
    for lineno, line in enumerate(lines, 1):
        if lineno > 1 and EXEMPT.match(lines[lineno - 2]):
            continue
        if m := USES.match(line):
            ref = m["ref"].strip("\"'")
            if ref.startswith("./"):
                continue
            if ref.startswith("docker://"):
                if why := mutable_image(ref.removeprefix("docker://")):
                    errors.append(f"{path}:{lineno}: uses {ref}: {why}")
            elif not COMMIT_SHA.search(ref):
                errors.append(f"{path}:{lineno}: uses {ref}: pin to a 40-character commit sha")
        elif (m := IMAGE.match(line)) and "${{" not in line:
            if why := mutable_image(m["ref"]):
                errors.append(f"{path}:{lineno}: image {m['ref']}: {why}")
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
        try:
            with open(path, encoding="utf-8") as fh:
                lines = fh.read().splitlines()
        except (OSError, UnicodeDecodeError):
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
