#!/usr/bin/env python3
"""Builds deterministic release archives and checksum files.

    package_release.py archive --version V --target T --binary PATH --out DIR
    package_release.py source  --version V --out DIR
    package_release.py sums    --out FILE ARCHIVE...

Archives contain the `weavec` binary, LICENSE, THIRD-PARTY-NOTICES.md and
README.md under a directory named after the archive, with fixed timestamps,
owners and permissions, so the same inputs always give the same bytes.
Windows targets get a .zip, others a .tar.gz. Standard library only.
"""
import argparse
import gzip
import hashlib
import io
import os
import subprocess
import sys
import tarfile
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FIXED_TIME = 1_704_067_200  # 2024-01-01T00:00:00Z; archive metadata only
EXTRAS = ["LICENSE", "THIRD-PARTY-NOTICES.md", "README.md"]


def epoch() -> int:
    """SOURCE_DATE_EPOCH when set, so archives can be tied to a commit time."""
    value = os.environ.get("SOURCE_DATE_EPOCH")
    return int(value) if value and value.isdigit() else FIXED_TIME


def members(binary: Path, directory: str, windows: bool):
    name = "weavec.exe" if windows else "weavec"
    yield f"{directory}/{name}", binary.read_bytes(), 0o755
    for extra in EXTRAS:
        yield f"{directory}/{extra}", (ROOT / extra).read_bytes(), 0o644


def write_tar_gz(path: Path, files, mtime: int) -> None:
    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w", format=tarfile.PAX_FORMAT) as tar:
        for name, data, mode in sorted(files):
            info = tarfile.TarInfo(name)
            info.size, info.mode, info.mtime = len(data), mode, mtime
            info.uid = info.gid = 0
            info.uname = info.gname = ""
            tar.addfile(info, io.BytesIO(data))
    with open(path, "wb") as out:
        # mtime=0 and no file name in the gzip header: nothing varies.
        with gzip.GzipFile(filename="", mode="wb", fileobj=out, mtime=0, compresslevel=9) as gz:
            gz.write(raw.getvalue())


def write_zip(path: Path, files, mtime: int) -> None:
    stamp = max(mtime, 315_532_800)  # zip cannot store dates before 1980
    date_time = tuple(__import__("time").gmtime(stamp))[:6]
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for name, data, mode in sorted(files):
            info = zipfile.ZipInfo(name, date_time=date_time)
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = (0o100000 | mode) << 16
            info.create_system = 3
            archive.writestr(info, data)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def cmd_archive(args) -> Path:
    windows = "windows" in args.target
    directory = f"weavec-{args.version}-{args.target}"
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    files = list(members(Path(args.binary), directory, windows))
    path = out / (directory + (".zip" if windows else ".tar.gz"))
    (write_zip if windows else write_tar_gz)(path, files, epoch())
    print(f"{sha256(path)}  {path.name}")
    return path


def cmd_source(args) -> Path:
    name = f"weave-css-{args.version}-source"
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    path = out / f"{name}.tar.gz"
    tar_bytes = subprocess.run(
        ["git", "archive", "--format=tar", f"--prefix={name}/", "HEAD"],
        cwd=ROOT, check=True, stdout=subprocess.PIPE,
    ).stdout
    with open(path, "wb") as handle:
        with gzip.GzipFile(filename="", mode="wb", fileobj=handle, mtime=0, compresslevel=9) as gz:
            gz.write(tar_bytes)
    print(f"{sha256(path)}  {path.name}")
    return path


def cmd_sums(args) -> None:
    lines = sorted(f"{sha256(Path(a))}  {Path(a).name}" for a in args.archives)
    Path(args.out).write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    print("\n".join(lines))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="command", required=True)
    a = sub.add_parser("archive")
    a.add_argument("--version", required=True)
    a.add_argument("--target", required=True)
    a.add_argument("--binary", required=True)
    a.add_argument("--out", required=True)
    s = sub.add_parser("source")
    s.add_argument("--version", required=True)
    s.add_argument("--out", required=True)
    c = sub.add_parser("sums")
    c.add_argument("--out", required=True)
    c.add_argument("archives", nargs="+")
    args = parser.parse_args()
    {"archive": cmd_archive, "source": cmd_source, "sums": cmd_sums}[args.command](args)
    return 0


if __name__ == "__main__":
    sys.exit(main())
