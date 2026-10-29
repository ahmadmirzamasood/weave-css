#!/usr/bin/env python3
"""Computed-style differential checks for nesting lowering.

For every case in tests/browser/cases, this script

1. compiles `main.weave` with the Weave CLI (the lowered, flat CSS);
2. derives the native CSS Nesting stylesheet from the same source by
   removing the `rule` keyword (the cases use only `rule`, declarations and
   CSS at-rules, so the result is exactly the native nested stylesheet);
3. loads both, with the case's `body.html`, in a headless Chromium-based
   browser and reads `getComputedStyle` for the listed properties of every
   element that has an `id`;
4. requires the two results to be identical, and requires every
   `naive.css` (a hand-written wrong lowering, such as Cartesian
   expansion) to differ from native on at least one value, which shows that
   the case can detect the error it targets.

The browser must support native CSS nesting (`CSS.supports("selector(&)")`).
It is found through $WEAVE_BROWSER or common install locations.

Usage: python3 scripts/browser_check.py [--weave PATH] [--browser PATH]
Exit status: 0 all cases agree, 1 a difference or failure, 2 setup error.
"""

from __future__ import annotations

import argparse
import html
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CASES = ROOT / "tests" / "browser" / "cases"

BROWSER_CANDIDATES = [
    "google-chrome",
    "google-chrome-stable",
    "chromium",
    "chromium-browser",
    "chrome",
    r"C:\Program Files\Google\Chrome\Application\chrome.exe",
    r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
]

PAGE = """<!doctype html>
<html><head><meta charset="utf-8"><style>
{css}
</style></head><body>
{body}
<script>
const props = {props};
const out = {{ nesting: CSS.supports("selector(&)"), agent: navigator.userAgent, probes: {{}} }};
for (const el of document.querySelectorAll("[id]")) {{
  const cs = getComputedStyle(el);
  out.probes[el.id] = Object.fromEntries(props.map(p => [p, cs.getPropertyValue(p)]));
}}
const pre = document.createElement("pre");
pre.textContent = "@@RES" + "ULT@@" + JSON.stringify(out) + "@@E" + "ND@@";
document.body.appendChild(pre);
</script></body></html>
"""

# Cases never use the word `rule` other than as the keyword.
RULE_KEYWORD = re.compile(r"\brule\s+")


def find_browser(explicit: str | None) -> str | None:
    for candidate in [explicit, os.environ.get("WEAVE_BROWSER"), *BROWSER_CANDIDATES]:
        if not candidate:
            continue
        found = shutil.which(candidate) or (candidate if Path(candidate).is_file() else None)
        if found:
            return found
    return None


def native_css(source: str) -> str:
    return RULE_KEYWORD.sub("", source)


def computed(browser: str, css: str, body: str, props: list[str], work: Path, name: str) -> dict:
    page = work / f"{name}.html"
    page.write_text(PAGE.format(css=css, body=body, props=json.dumps(props)), encoding="utf-8")
    profile = work / f"profile-{name}"
    command = [
        browser,
        "--headless=new",
        "--disable-gpu",
        "--no-sandbox",
        "--no-first-run",
        "--disable-extensions",
        f"--user-data-dir={profile}",
        "--window-size=1024,768",
        "--dump-dom",
        page.as_uri(),
    ]
    result = subprocess.run(command, capture_output=True, text=True, encoding="utf-8", timeout=120)
    match = re.search(r"@@RESULT@@(.*?)@@END@@", result.stdout, re.S)
    if not match:
        raise RuntimeError(f"no result from the browser for {name}: {result.stderr[-2000:]}")
    return json.loads(html.unescape(match.group(1)))


def differences(a: dict, b: dict) -> list[str]:
    found = []
    for probe in sorted(set(a) | set(b)):
        for prop in sorted(set(a.get(probe, {})) | set(b.get(probe, {}))):
            left = a.get(probe, {}).get(prop)
            right = b.get(probe, {}).get(prop)
            if left != right:
                found.append(f"#{probe} {prop}: native {left!r}, other {right!r}")
    return found


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--weave", help="path to the weave CLI (default: build it with cargo)")
    parser.add_argument("--browser", help="Chromium-based browser executable")
    args = parser.parse_args()

    browser = find_browser(args.browser)
    if browser is None:
        print("error: no Chromium-based browser found; set WEAVE_BROWSER", file=sys.stderr)
        return 2
    weave = args.weave
    if weave is None:
        subprocess.run(["cargo", "build", "--quiet", "--locked", "-p", "weave-css"], cwd=ROOT, check=True)
        exe = "weave.exe" if os.name == "nt" else "weave"
        weave = str(ROOT / "target" / "debug" / exe)

    cases = sorted(p for p in CASES.iterdir() if (p / "case.toml").is_file())
    if not cases:
        print("error: no cases found", file=sys.stderr)
        return 2
    failures = 0
    agent = None
    with tempfile.TemporaryDirectory(prefix="weave-browser-") as tmp:
        work = Path(tmp)
        for case in cases:
            meta = tomllib.loads((case / "case.toml").read_text(encoding="utf-8"))
            props = meta["properties"]
            source = (case / "main.weave").read_text(encoding="utf-8")
            body = (case / "body.html").read_text(encoding="utf-8")
            build = subprocess.run([weave, "build", str(case / "main.weave")], capture_output=True, text=True, encoding="utf-8")
            if build.returncode != 0:
                print(f"FAIL {case.name}: weave build failed\n{build.stderr}")
                failures += 1
                continue
            native = computed(browser, native_css(source), body, props, work, f"{case.name}-native")
            if not native["nesting"]:
                print("error: the browser does not support native CSS nesting", file=sys.stderr)
                return 2
            agent = native["agent"]
            lowered = computed(browser, build.stdout, body, props, work, f"{case.name}-lowered")
            diff = differences(native["probes"], lowered["probes"])
            probes = sum(len(v) for v in native["probes"].values())
            if diff:
                failures += 1
                print(f"FAIL {case.name}: lowered CSS differs from native nesting")
                for line in diff:
                    print(f"  {line}")
                continue
            sensitive = ""
            for naive_path in sorted(case.glob("naive*.css")):
                naive = computed(browser, naive_path.read_text(encoding="utf-8"), body, props, work, f"{case.name}-{naive_path.stem}")
                naive_diff = differences(native["probes"], naive["probes"])
                if not naive_diff:
                    failures += 1
                    print(f"FAIL {case.name}: {naive_path.name} is not distinguished from native nesting")
                    break
                sensitive += f"; {naive_path.name} differs on {len(naive_diff)}"
            else:
                print(f"ok   {case.name}: {probes} values equal{sensitive}")
    print(f"browser: {agent}")
    print(f"{len(cases)} cases, {failures} failed")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
