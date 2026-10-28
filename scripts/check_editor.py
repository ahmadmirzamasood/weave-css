#!/usr/bin/env python3
"""Checks the editor support files in editors/.

- Every JSON file parses.
- The TextMate grammar's `include` references resolve, every scope name
  ends in `.weave`, and every regular expression compiles (translated from
  Oniguruma syntax for the few constructs Python spells differently).
- The VS Code manifest points at files that exist.
- Highlighting smoke test: a tiny matcher applies the grammar's top-level
  `match` and `begin` patterns to the example sources and fails if a
  keyword line is not recognised (catches broken patterns, not every
  highlighting detail).

Exit status 0 when everything passes, 1 otherwise.
"""

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EDITORS = ROOT / "editors"
GRAMMAR = EDITORS / "vscode" / "syntaxes" / "weave.tmLanguage.json"
MANIFEST = EDITORS / "vscode" / "package.json"

problems = []


def problem(message):
    problems.append(message)


def to_python(pattern):
    """Translates Oniguruma-only escapes used by the grammar."""

    def hex_escape(m):
        value = int(m.group(1), 16)
        return "\\U%08x" % value

    return re.sub(r"\\x\{([0-9A-Fa-f]+)\}", hex_escape, pattern)


def walk(node, path, repository):
    if isinstance(node, dict):
        for key in ("match", "begin", "end"):
            if key in node:
                try:
                    re.compile(to_python(node[key]))
                except re.error as err:
                    problem(f"{path}.{key}: {err}: {node[key]}")
        include = node.get("include")
        if include is not None:
            if include.startswith("#"):
                if include[1:] not in repository:
                    problem(f"{path}: unknown include {include}")
            elif include not in ("$self", "$base"):
                problem(f"{path}: external include {include}")
        for key in ("name", "contentName"):
            if key in node and not node[key].endswith(".weave"):
                problem(f"{path}.{key}: scope {node[key]!r} does not end in .weave")
        for key, value in node.items():
            walk(value, f"{path}.{key}", repository)
    elif isinstance(node, list):
        for i, value in enumerate(node):
            walk(value, f"{path}[{i}]", repository)


def main():
    for path in sorted(EDITORS.rglob("*.json")):
        try:
            json.loads(path.read_text(encoding="utf-8"))
        except ValueError as err:
            problem(f"{path.relative_to(ROOT)}: invalid JSON: {err}")
    if problems:
        return report()

    grammar = json.loads(GRAMMAR.read_text(encoding="utf-8"))
    repository = grammar.get("repository", {})
    if grammar.get("scopeName") != "source.weave":
        problem("grammar scopeName must be source.weave")
    walk(grammar.get("patterns", []), "patterns", repository)
    walk(repository, "repository", repository)

    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    if manifest.get("private") is not True:
        problem("package.json must stay private (the extension is not published)")
    contributes = manifest.get("contributes", {})
    for language in contributes.get("languages", []):
        if not (MANIFEST.parent / language["configuration"]).is_file():
            problem(f"missing {language['configuration']}")
    for entry in contributes.get("grammars", []):
        if not (MANIFEST.parent / entry["path"]).is_file():
            problem(f"missing {entry['path']}")
    if problems:
        return report()

    # Smoke test: statement keywords at the start of example lines must be
    # matched by the grammar's keyword-bearing rules.
    rules = {
        name: re.compile(to_python(repository[name].get("match") or repository[name]["begin"]))
        for name in ("rule", "callable", "load", "bind")
    }
    keyword = re.compile(to_python(repository["keyword"]["patterns"][0]["match"]))
    at_rule = re.compile(to_python(repository["at-rule"]["begin"]))
    checked = 0
    for source in sorted((ROOT / "examples").rglob("*.weave")):
        for number, line in enumerate(source.read_text(encoding="utf-8").splitlines(), 1):
            stripped = line.strip()
            first = stripped.split(" ", 1)[0]
            where = f"{source.relative_to(ROOT)}:{number}"
            expected = {
                "rule": "rule",
                "fn": "callable",
                "recipe": "callable",
                "load": "load",
                "bind": "bind",
            }.get(first)
            if first == "export":
                expected = "bind" if stripped.split()[1] == "bind" else "callable"
            if expected and not rules[expected].search(stripped):
                problem(f"{where}: `{first}` line not matched by #{expected}")
            elif first in ("when", "each", "apply", "yield", "slot", "fail", "warn") and not keyword.search(stripped):
                problem(f"{where}: keyword `{first}` not matched")
            elif first.startswith("@") and not at_rule.match(stripped):
                problem(f"{where}: at-rule not matched")
            checked += 1
    if checked == 0:
        problem("no example lines checked")
    return report()


def report():
    for message in problems:
        print(f"error: {message}")
    if problems:
        print(f"{len(problems)} problem(s)")
        return 1
    print("editor support files: ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
