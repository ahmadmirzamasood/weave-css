"""Builds the seed corpora for the fuzz targets from the repository's
fixtures and examples.

Usage: python3 fuzz/seed.py OUT_DIR

Writes OUT_DIR/{lex,parse,compile}/. Every `.weave` and `.css` file under
tests/fixtures and examples becomes a `lex` and `parse` seed. Each fixture
or example directory becomes `compile` seeds: its `main.weave` (or
`input.weave`) plus up to three other modules, renamed to the target's
fixed module names (`a.weave`, `lib/b.weave`, `lib/c.weave`) with their
`load` paths rewritten, joined by 0xFF and prefixed with option bytes.
Seeds are named by content hash so reruns are stable.
"""

import hashlib
import posixpath
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SLOTS = ["main.weave", "a.weave", "lib/b.weave", "lib/c.weave"]
# Option bytes for the compile target: none, source map, raised depths,
# tiny diagnostic limit.
FLAGS = [0, 1, 4 | 8, 16]


def write(directory: Path, data: bytes) -> None:
    directory.mkdir(parents=True, exist_ok=True)
    (directory / hashlib.sha1(data).hexdigest()).write_bytes(data)


def relative(from_slot: str, to_slot: str) -> str:
    path = posixpath.relpath(to_slot, posixpath.dirname(from_slot) or ".")
    return path if path.startswith("../") else "./" + path


def compile_seed(directory: Path) -> bytes | None:
    files = sorted(p for p in directory.rglob("*.weave") if p.is_file())
    entry = next((p for p in files if p.name in ("main.weave", "input.weave")), None)
    if entry is None:
        return None
    others = [p for p in files if p != entry][: len(SLOTS) - 1]
    slot_of = {entry: SLOTS[0], **{p: SLOTS[i + 1] for i, p in enumerate(others)}}
    parts = []
    for path, slot in slot_of.items():
        data = path.read_bytes()
        for target, target_slot in slot_of.items():
            if target == path:
                continue
            original = posixpath.relpath(
                target.relative_to(directory).as_posix(),
                posixpath.dirname(path.relative_to(directory).as_posix()) or ".",
            )
            for spelling in (original, "./" + original):
                data = data.replace(
                    f'"{spelling}"'.encode(), f'"{relative(slot, target_slot)}"'.encode()
                )
        parts.append(data)
    return b"\xff".join(parts)


def main() -> int:
    out = Path(sys.argv[1])
    sources = [ROOT / "tests" / "fixtures", ROOT / "examples"]
    count = {"lex": 0, "parse": 0, "compile": 0}
    for base in sources:
        for path in sorted(base.rglob("*")):
            if path.suffix in (".weave", ".css") and path.is_file():
                data = path.read_bytes()
                write(out / "lex", data)
                write(out / "parse", data)
                count["lex"] += 1
                count["parse"] += 1
            if path.is_dir():
                seed = compile_seed(path) if any(path.glob("*.weave")) else None
                if seed is not None:
                    for flag in FLAGS:
                        write(out / "compile", bytes([flag]) + seed)
                        count["compile"] += 1
    print(count)
    return 0


if __name__ == "__main__":
    sys.exit(main())
