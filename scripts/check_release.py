#!/usr/bin/env python3
"""Consistency checks for a release candidate (run in CI and by the release
workflow). Standard library only; exits non-zero and lists every problem.

Checks: one workspace version everywhere (manifests, lockfiles, docs), a
pre-release or release version without `-dev`, publishable crate metadata,
LICENSE copies, a changelog section and release notes for the version,
required public documents, and that every workflow action is pinned to a
full commit SHA.
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
problems = []


def read(path):
    return (ROOT / path).read_text(encoding="utf-8").replace("\r\n", "\n")


def problem(message):
    problems.append(message)


def toml_value(text, key, section=None):
    """First `key = "value"` (optionally inside [section])."""
    scope = text
    if section:
        match = re.search(rf"^\[{re.escape(section)}\]\n(.*?)(?=^\[|\Z)", text, re.S | re.M)
        scope = match.group(1) if match else ""
    match = re.search(rf'^{re.escape(key)}\s*=\s*"([^"]*)"', scope, re.M)
    return match.group(1) if match else None


def main():
    cargo = read("Cargo.toml")
    version = toml_value(cargo, "version", "workspace.package")
    if not version:
        problem("Cargo.toml: workspace.package.version missing")
        version = "?"
    if not re.fullmatch(r"\d+\.\d+\.\d+(-(alpha|beta|rc)\.\d+)?", version):
        problem(f"version {version!r} is not X.Y.Z or X.Y.Z-(alpha|beta|rc).N (no -dev for a release candidate)")

    # Workspace dependency pin must equal the version.
    pin = re.search(r'weave-compiler\s*=\s*\{[^}]*version\s*=\s*"=([^"]+)"', cargo)
    if not pin or pin.group(1) != version:
        problem(f"Cargo.toml: weave-compiler dependency must be pinned with version = \"={version}\"")

    # Lockfiles.
    for lock in ("Cargo.lock", "fuzz/Cargo.lock"):
        text = read(lock)
        for name in ("weave-compiler", "weave-css", "weave-bench", "weave-docs"):
            found = re.search(rf'name = "{name}"\nversion = "([^"]+)"', text)
            if lock == "Cargo.lock" and not found:
                problem(f"{lock}: package {name} missing")
            elif found and found.group(1) != version:
                problem(f"{lock}: {name} is {found.group(1)}, expected {version}")

    # Crate manifests.
    compiler = read("crates/weave-compiler/Cargo.toml")
    cli = read("crates/weave-cli/Cargo.toml")
    bench = read("crates/weave-bench/Cargo.toml")
    if toml_value(compiler, "name", "package") != "weave-compiler":
        problem("crates/weave-compiler: package name must be weave-compiler")
    if toml_value(cli, "name", "package") != "weave-css":
        problem("crates/weave-cli: package name must be weave-css")
    for label, manifest in (("weave-compiler", compiler), ("weave-css", cli)):
        for key in ("description", "readme", "keywords", "categories", "include"):
            if not re.search(rf"^{key}\s*=", manifest, re.M):
                problem(f"{label}: manifest lacks `{key}`")
        for key in ("license", "repository", "homepage", "version", "edition", "rust-version"):
            if not re.search(rf"^{re.escape(key)}(\.workspace\s*=\s*true|\s*=)", manifest, re.M):
                problem(f"{label}: manifest lacks `{key}`")
        if re.search(r"^publish\s*=\s*false", manifest, re.M):
            problem(f"{label}: must be publishable")
    for label, text in (("weave-bench", bench), ("weave-docs", read("crates/weave-docs/Cargo.toml"))):
        if not re.search(r"^publish\s*=\s*false", text, re.M):
            problem(f"{label} must stay publish = false")

    # LICENSE copies and package READMEs.
    root_license = read("LICENSE")
    for crate in ("weave-compiler", "weave-cli"):
        if read(f"crates/{crate}/LICENSE") != root_license:
            problem(f"crates/{crate}/LICENSE differs from LICENSE")
        if not (ROOT / "crates" / crate / "README.md").is_file():
            problem(f"crates/{crate}/README.md missing")
    if "MIT License" not in root_license:
        problem("LICENSE is not the MIT license")

    # Documents.
    for doc in (
        "README.md", "CHANGELOG.md", "CONTRIBUTING.md", "SECURITY.md", "THIRD-PARTY-NOTICES.md",
        "docs/INSTALL.md", "docs/TUTORIAL.md", "docs/LIBRARY.md", "docs/CLI.md",
        "docs/COMPATIBILITY-POLICY.md", "docs/BENCHMARKS.md", "docs/RELEASING.md",
        f"docs/RELEASE-NOTES-{version}.md",
    ):
        if not (ROOT / doc).is_file():
            problem(f"{doc} missing")
    changelog = read("CHANGELOG.md")
    if not re.search(rf"^## {re.escape(version)}( |$)", changelog, re.M):
        problem(f"CHANGELOG.md has no `## {version}` section")
    if re.search(r"^## Unreleased\n\s*\n- ", changelog, re.M):
        problem("CHANGELOG.md still has entries under `## Unreleased`; move them into the release section")
    notes = ROOT / f"docs/RELEASE-NOTES-{version}.md"
    if notes.is_file():
        text = notes.read_text(encoding="utf-8")
        if version not in text.splitlines()[0]:
            problem(f"{notes.name}: the title must name {version}")
        for needle in ("Known limitations", "Security reporting", "Benchmark", "Migration", "Candidate commit"):
            if needle not in text:
                problem(f"{notes.name}: missing the {needle!r} section")
    for doc, needle in (("docs/INSTALL.md", version), ("README.md", version), ("crates/weave-cli/README.md", version)):
        if (ROOT / doc).is_file() and needle not in read(doc):
            problem(f"{doc} does not mention {needle}")

    # Workflows: every `uses:` pinned to a 40-hex commit SHA (local actions excepted).
    for workflow in sorted((ROOT / ".github" / "workflows").glob("*.yml")):
        for number, line in enumerate(workflow.read_text(encoding="utf-8").splitlines(), 1):
            match = re.match(r"\s*-?\s*uses:\s*(\S+)", line)
            if match and not match.group(1).startswith("./"):
                if not re.fullmatch(r"[\w.-]+/[\w./-]+@[0-9a-f]{40}", match.group(1)):
                    problem(f"{workflow.name}:{number}: action {match.group(1)} is not pinned to a full commit SHA")

    if problems:
        print("release check FAILED:")
        for item in problems:
            print(f"  - {item}")
        return 1
    print(f"release check ok: version {version}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
