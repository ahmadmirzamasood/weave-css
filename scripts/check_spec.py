"""Consistency checks for the Weave fixtures and documentation.

Checks fixture structure and metadata, diagnostic expectation syntax and
positions, line endings, the fixture indexes, and relative Markdown links in
git-tracked files, for the specification fixtures (tests/fixtures/spec) and the
CSS-subset fixtures (tests/fixtures/css). It does not compile anything: the
fixture runner in crates/weave-compiler/tests/fixtures.rs executes them.

Usage: python scripts/check_spec.py [repo-root]
"""

import re
import subprocess
import sys
import tomllib
from pathlib import Path

FIXTURE_SETS = {
    # directory: required case.toml keys
    Path("tests/fixtures/spec"): {"description", "spec", "status", "entry", "expect"},
    Path("tests/fixtures/css"): {"description", "spec", "expect", "validation"},
}
CRLF_FIXTURES = {"tests/fixtures/spec/035-unicode-and-crlf/main.weave"}
VALIDATION = {"independent", "preserve-only"}
DIAG_LINE = re.compile(
    r'^(error|warning) ([EW]\d{4}) ([A-Za-z0-9_./-]+\.weave):(\d+):(\d+) "(.+)"$'
)
MD_LINK = re.compile(r"\[[^\]]*\]\(([^)\s]+)\)")


class Checker:
    def __init__(self, root: Path):
        self.root = root
        self.errors: list[str] = []

    def fail(self, where: Path | str, message: str) -> None:
        if isinstance(where, Path) and where.is_relative_to(self.root):
            where = where.relative_to(self.root).as_posix()
        self.errors.append(f"{where}: {message}")

    def read_text(self, path: Path) -> str | None:
        try:
            return path.read_bytes().decode("utf-8")
        except UnicodeDecodeError:
            self.fail(path, "not valid UTF-8")
            return None

    def check_fixture(self, case: Path, keys: set[str]) -> None:
        rel = case.relative_to(self.root)
        if not re.fullmatch(r"\d{3}-[a-z0-9]+(-[a-z0-9]+)*", case.name):
            self.fail(case, "directory name must be NNN-kebab-case")

        meta_path = case / "case.toml"
        if not meta_path.is_file():
            self.fail(case, "missing case.toml")
            return
        try:
            meta = tomllib.loads(meta_path.read_text(encoding="utf-8"))
        except (tomllib.TOMLDecodeError, UnicodeDecodeError) as err:
            self.fail(meta_path, f"invalid TOML: {err}")
            return
        if meta.get("expect") == "error":
            keys = keys - {"validation"}  # a validation mode applies to CSS output only
        if set(meta) != keys:
            self.fail(meta_path, f"keys must be exactly {sorted(keys)}")
            return
        if "status" in meta and meta["status"] != "specification":
            self.fail(meta_path, "status must be 'specification'")
        if "validation" in meta and meta["validation"] not in VALIDATION:
            self.fail(meta_path, f"validation must be one of {sorted(VALIDATION)}")
        if not isinstance(meta["spec"], list) or not meta["spec"]:
            self.fail(meta_path, "spec must be a non-empty list")
        if not (case / meta.get("entry", "main.weave")).is_file():
            self.fail(meta_path, f"entry {meta.get('entry', 'main.weave')} does not exist")

        sources: dict[str, list[str]] = {}
        for weave in sorted(case.glob("*.weave")):
            text = self.read_text(weave)
            if text is None:
                continue
            key = f"{rel.as_posix()}/{weave.name}"
            has_crlf = "\r\n" in text
            if key in CRLF_FIXTURES and not has_crlf:
                self.fail(weave, "must keep CRLF line endings")
            if key not in CRLF_FIXTURES and "\r" in text:
                self.fail(weave, "must use LF line endings")
            sources[weave.name] = text.replace("\r\n", "\n").split("\n")

        css, diag = case / "expected.css", case / "expected.diag"
        if meta["expect"] == "css":
            if diag.exists() or not css.is_file():
                self.fail(case, "expect = css needs expected.css and no expected.diag")
                return
            text = self.read_text(css)
            if text is not None:
                if "\r" in text:
                    self.fail(css, "expected output must use LF line endings")
                if text and not text.endswith("\n"):
                    self.fail(css, "non-empty expected output must end with a newline")
        elif meta["expect"] == "error":
            if css.exists() or not diag.is_file():
                self.fail(case, "expect = error needs expected.diag and no expected.css")
                return
            self.check_diag(diag, sources)
        else:
            self.fail(meta_path, "expect must be 'css' or 'error'")

    def check_diag(self, diag: Path, sources: dict[str, list[str]]) -> None:
        text = self.read_text(diag)
        if text is None:
            return
        entries = 0
        for number, line in enumerate(text.splitlines(), start=1):
            if not line.strip() or line.startswith("#"):
                continue
            match = DIAG_LINE.match(line)
            if not match:
                self.fail(f"{diag.relative_to(self.root).as_posix()}:{number}", "expected 'severity CODE file:line:col \"substring\"'")
                continue
            entries += 1
            severity, code, file, row, col = match.group(1, 2, 3, 4, 5)
            if severity[0].upper() != code[0]:
                self.fail(f"{diag.relative_to(self.root).as_posix()}:{number}", "error codes start with E, warning codes with W")
            lines = sources.get(file)
            if lines is None:
                self.fail(f"{diag.relative_to(self.root).as_posix()}:{number}", f"{file} is not a file of this fixture")
                continue
            row, col = int(row), int(col)
            if not 1 <= row <= len(lines) or not 1 <= col <= len(lines[row - 1]):
                self.fail(f"{diag.relative_to(self.root).as_posix()}:{number}", f"position {row}:{col} is outside {file}")
        if entries == 0:
            self.fail(diag, "no diagnostics listed")

    def check_index(self, directory: Path, cases: list[Path]) -> None:
        readme = self.root / directory / "README.md"
        text = self.read_text(readme) if readme.is_file() else None
        if text is None:
            self.fail(readme, "missing fixture index")
            return
        for case in cases:
            if f"[{case.name}]({case.name}/)" not in text:
                self.fail(readme, f"index does not list {case.name}")

    def check_links(self, md: Path) -> None:
        text = self.read_text(md)
        if text is None:
            return
        for target in MD_LINK.findall(text):
            if re.match(r"^[a-z][a-z0-9+.-]*:", target) or target.startswith("#"):
                continue
            path = target.split("#", 1)[0]
            if path and not (md.parent / path).exists():
                self.fail(md, f"broken link: {target}")

    def run(self) -> int:
        total = 0
        for directory, keys in FIXTURE_SETS.items():
            cases = sorted(p for p in (self.root / directory).iterdir() if p.is_dir())
            numbers = [c.name[:3] for c in cases]
            if len(numbers) != len(set(numbers)):
                self.fail(directory.as_posix(), "fixture numbers must be unique")
            for case in cases:
                self.check_fixture(case, keys)
            self.check_index(directory, cases)
            total += len(cases)
        tracked = subprocess.run(
            ["git", "ls-files", "-z", "--", "*.md"],
            cwd=self.root, capture_output=True, check=True,
        ).stdout.decode("utf-8").split("\0")
        for name in sorted(filter(None, tracked)):
            self.check_links(self.root / name)

        for error in self.errors:
            print(f"error: {error}")
        print(f"checked {total} fixtures: {len(self.errors)} problem(s)")
        return 1 if self.errors else 0


if __name__ == "__main__":
    sys.exit(Checker(Path(sys.argv[1] if len(sys.argv) > 1 else ".").resolve()).run())
