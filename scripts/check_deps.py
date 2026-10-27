"""Dependency policy check for the Weave Cargo workspace.

Fails when the resolved dependency graph (all targets, all features) contains
a package that is not reviewed in REVIEWED below, when a package's license is
not allowed, or when any package declares a native `links` key. Every
package's build script is listed for review.

Packages reviewed as dev-only (DEV_ONLY) may use a license from
DEV_ONLY_LICENSES; the check fails if such a package becomes reachable from a
workspace crate through normal or build dependencies, so it can never end up
in a shipped binary or library.

Usage: python scripts/check_deps.py
"""

import json
import subprocess
import sys

# Third-party packages reviewed for license and native build requirements:
# name -> reviewed version. Add an entry only after reviewing the crate and
# recording it in CHANGELOG.md.
REVIEWED: dict[str, str] = {
    # Test-only: independent CSS Syntax 3 parser validating emitted CSS
    # (default features disabled: no proc macros, no phf).
    "cssparser": "0.38.0",
    "dtoa-short": "0.3.5",
    "dtoa": "1.0.11",
    "itoa": "1.0.18",
    "smallvec": "1.16.4",
}

# Reviewed packages that must only ever be dev-dependencies.
DEV_ONLY = {"cssparser", "dtoa-short", "dtoa", "itoa", "smallvec"}

ALLOWED_LICENSES = {
    "MIT",
    "Apache-2.0",
    "MIT OR Apache-2.0",
    "Apache-2.0 OR MIT",
    "Unicode-3.0",
    "Zlib",
    "BSD-2-Clause",
    "BSD-3-Clause",
}

# File-level copyleft is acceptable for unmodified test tooling that is
# never distributed with Weave.
DEV_ONLY_LICENSES = {"MPL-2.0"}


def shipped_packages(metadata: dict) -> set[str]:
    """Package ids reachable from workspace crates via normal/build edges."""
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    seen: set[str] = set()
    todo = list(metadata["workspace_members"])
    while todo:
        package = todo.pop()
        if package in seen:
            continue
        seen.add(package)
        for dep in nodes.get(package, {}).get("deps", []):
            if any(kind["kind"] in (None, "build") for kind in dep["dep_kinds"]):
                todo.append(dep["pkg"])
    return seen


def main() -> int:
    metadata = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--locked", "--all-features"],
            check=True,
            capture_output=True,
            text=True,
        ).stdout
    )
    members = set(metadata["workspace_members"])
    shipped = shipped_packages(metadata)
    errors = []
    for package in sorted(metadata["packages"], key=lambda p: p["name"]):
        name, version = package["name"], package["version"]
        third_party = package["id"] not in members
        dev_only = package["id"] not in shipped
        license_ = package.get("license")
        build_script = any("custom-build" in t["kind"] for t in package["targets"])
        print(
            f"{name} {version}: {'third-party' if third_party else 'workspace'}"
            f"{', dev-only' if dev_only else ''}, "
            f"license {license_}, build script {'yes' if build_script else 'no'}"
        )
        if third_party and REVIEWED.get(name) != version:
            errors.append(f"{name} {version} is not a reviewed dependency")
        if name in DEV_ONLY and not dev_only:
            errors.append(f"{name} {version} is reviewed as dev-only but is shipped")
        license_ok = license_ in ALLOWED_LICENSES or (
            name in DEV_ONLY and dev_only and license_ in DEV_ONLY_LICENSES
        )
        if not license_ok:
            errors.append(f"{name} {version} has license {license_!r}")
        if package.get("links"):
            errors.append(f"{name} {version} links native library {package['links']!r}")
        if build_script and not third_party:
            errors.append(f"workspace crate {name} must not have a build script")
    for error in errors:
        print(f"error: {error}", file=sys.stderr)
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
