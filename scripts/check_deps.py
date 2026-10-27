"""Dependency policy check for the Weave Cargo workspace.

Fails when the resolved dependency graph (all targets, all features) contains
a package that is not reviewed in REVIEWED below, when a package's license is
not in ALLOWED_LICENSES, or when any package declares a native `links` key.
Every package's build script is listed for review.

Usage: python scripts/check_deps.py
"""

import json
import subprocess
import sys

# Third-party packages reviewed for license and native build requirements:
# name -> reviewed version. Empty: the workspace has no third-party
# dependencies. Add an entry only after reviewing the crate and recording it in
# CHANGELOG.md.
REVIEWED: dict[str, str] = {}

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
    errors = []
    for package in sorted(metadata["packages"], key=lambda p: p["name"]):
        name, version = package["name"], package["version"]
        third_party = package["id"] not in members
        license_ = package.get("license")
        build_script = any("custom-build" in t["kind"] for t in package["targets"])
        print(
            f"{name} {version}: {'third-party' if third_party else 'workspace'}, "
            f"license {license_}, build script {'yes' if build_script else 'no'}"
        )
        if third_party and REVIEWED.get(name) != version:
            errors.append(f"{name} {version} is not a reviewed dependency")
        if license_ not in ALLOWED_LICENSES:
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
