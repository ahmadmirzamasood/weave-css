"""Weave benchmark driver.

Builds the release binaries, generates the benchmark corpus with
`weave-bench generate`, measures cold `weavec build` processes (wall time and
peak memory), runs the in-process scenarios of `weave-bench run`, and writes
one JSON file with the environment, the corpus hashes and every raw sample.

    python scripts/bench.py run --out-dir DIR [--baseline CHECKOUT] [--rounds 4]
                                [--samples 40] [--cold-samples 20]
    python scripts/bench.py summary report.json
    python scripts/bench.py compare before.json after.json

Cold means a new `weave` process per build. The operating system's file
cache is warm: every workload is built once, untimed, before its samples, so
cold numbers measure process start, file reading and compilation, not disk
latency. Spawning from Python adds overhead to every cold sample; the
`process-baseline` scenario (`weavec --version`) measures it.

With --baseline, both builds are measured in alternating rounds (the order
flips every round) so that slow drift in machine speed affects both alike;
before.json and after.json then hold the merged samples of each build.

Peak memory is the peak working set (Windows) or maximum resident set size
(Linux, macOS) of each `weavec build` process.
"""

import argparse
import ctypes
import hashlib
import json
import os
import platform
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EXE = ".exe" if os.name == "nt" else ""


def cli_package(tree: Path) -> str:
    """The CLI package name of a checkout (`weave-cli` before 0.1.0-rc.2)."""
    manifest = (tree / "crates" / "weave-cli" / "Cargo.toml").read_text(encoding="utf-8")
    return "weave-css" if 'name = "weave-css"' in manifest else "weave-cli"


def sh(*cmd: str) -> str:
    return subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True, check=False).stdout.strip()


def windows_info() -> dict:
    query = (
        "$c=Get-CimInstance Win32_Processor; $s=Get-CimInstance Win32_ComputerSystem;"
        "$o=Get-CimInstance Win32_OperatingSystem; $b=Get-CimInstance Win32_Battery;"
        "[pscustomobject]@{cpu=$c.Name; cores=$c.NumberOfCores; threads=$c.NumberOfLogicalProcessors;"
        "max_mhz=$c.MaxClockSpeed; ram_bytes=$s.TotalPhysicalMemory; os=$o.Caption + ' ' + $o.Version;"
        "battery_status=$b.BatteryStatus; power_plan=(powercfg /getactivescheme) -join ' '} | ConvertTo-Json"
    )
    out = subprocess.run(
        ["powershell", "-NoProfile", "-Command", query], capture_output=True, text=True, check=False
    ).stdout
    try:
        info = json.loads(out)
    except json.JSONDecodeError:
        return {}
    # Win32_Battery.BatteryStatus: 1 = discharging, 2 = on AC power.
    status = info.pop("battery_status", None)
    info["power_source"] = {1: "battery", 2: "ac"}.get(status, "unknown" if status else "no battery")
    return info


def environment() -> dict:
    env = {
        "platform": platform.platform(),
        "machine": platform.machine(),
        "python": platform.python_version(),
        "rustc": sh("rustc", "-vV"),
        "cargo": sh("cargo", "-V"),
        "revision": sh("git", "rev-parse", "HEAD"),
        "branch": sh("git", "rev-parse", "--abbrev-ref", "HEAD"),
        "dirty_files": sh("git", "status", "--porcelain", "--untracked-files=no").splitlines(),
        "release_profile": "codegen-units = 1, lto = thin (Cargo.toml [profile.release]), no RUSTFLAGS"
        if not os.environ.get("RUSTFLAGS")
        else "RUSTFLAGS=" + os.environ["RUSTFLAGS"],
        "timestamp_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
    }
    if os.name == "nt":
        env.update(windows_info())
    else:
        env["cpu"] = platform.processor() or sh("uname", "-p")
        env["threads"] = os.cpu_count()
    return env


def corpus_hashes(corpus: Path) -> dict:
    hashes = {}
    for workload in sorted(p for p in corpus.iterdir() if p.is_dir()):
        digest = hashlib.sha256()
        files = sorted(p for p in workload.rglob("*") if p.is_file())
        for path in files:
            digest.update(path.relative_to(workload).as_posix().encode() + b"\0")
            digest.update(path.read_bytes() + b"\0")
        hashes[workload.name] = {"sha256": digest.hexdigest(), "files": len(files)}
    return hashes


def fnv1a64(data: bytes) -> str:
    h = 0xCBF29CE484222325
    for b in data:
        h = ((h ^ b) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return f"{h:016x}"


class _Pmc(ctypes.Structure):
    _fields_ = [
        ("cb", ctypes.c_ulong),
        ("PageFaultCount", ctypes.c_ulong),
        ("PeakWorkingSetSize", ctypes.c_size_t),
        ("WorkingSetSize", ctypes.c_size_t),
        ("QuotaPeakPagedPoolUsage", ctypes.c_size_t),
        ("QuotaPagedPoolUsage", ctypes.c_size_t),
        ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t),
        ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
        ("PagefileUsage", ctypes.c_size_t),
        ("PeakPagefileUsage", ctypes.c_size_t),
    ]


def timed_process(cmd: list[str]) -> tuple[int, int | None, int]:
    """Runs `cmd`; returns (wall ns, peak memory bytes or None, exit code)."""
    start = time.perf_counter_ns()
    proc = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    if os.name == "nt":
        _, err = proc.communicate()
        wall = time.perf_counter_ns() - start
        peak = None
        counters = _Pmc()
        counters.cb = ctypes.sizeof(_Pmc)
        handle = getattr(proc, "_handle", None)
        if handle is not None and ctypes.windll.kernel32.K32GetProcessMemoryInfo(
            ctypes.c_void_p(int(handle)), ctypes.byref(counters), counters.cb
        ):
            peak = counters.PeakWorkingSetSize
        code = proc.returncode
    else:
        _, status, usage = os.wait4(proc.pid, 0)
        wall = time.perf_counter_ns() - start
        proc.returncode = code = os.waitstatus_to_exitcode(status)
        err = proc.stderr.read() if proc.stderr else b""
        # ru_maxrss is KiB on Linux and bytes on macOS.
        peak = usage.ru_maxrss * (1 if sys.platform == "darwin" else 1024)
    if code != 0:
        raise SystemExit(f"{' '.join(cmd)} failed ({code}): {err.decode(errors='replace')}")
    return wall, peak, code


def cold(weave: Path, corpus: Path, entries: dict, bench: dict, samples: int) -> list:
    results = []
    baseline = {"name": "process-baseline", "command": "weavec --version", "wall": [], "peak_memory": []}
    timed_process([str(weave), "--version"])
    for _ in range(samples):
        wall, peak, _ = timed_process([str(weave), "--version"])
        baseline["wall"].append(wall)
        baseline["peak_memory"].append(peak)
    results.append(baseline)
    with tempfile.TemporaryDirectory() as tmp:
        for workload in bench["workloads"]:
            name = workload["name"]
            entry = corpus / name / entries[name]
            for scenario, extra in (("cold", []), ("cold-map", ["--source-map"])):
                out = Path(tmp) / f"{name}.css"
                cmd = [str(weave), "build", str(entry), "-o", str(out), *extra]
                timed_process(cmd)  # untimed: warms the OS file cache
                css = out.read_bytes()
                checked = css
                if extra:
                    # The CLI appends a `sourceMappingURL` comment line.
                    checked = css[: css.rstrip(b"\n").rfind(b"\n") + 1]
                if fnv1a64(checked) != workload["output_fnv1a64"]:
                    raise SystemExit(f"{name}/{scenario}: CLI output differs from the in-process build")
                row = {"name": f"{name}/{scenario}", "command": "weavec build " + " ".join(
                    [f"{name}/{entry.name}", "-o", "out.css", *extra]), "wall": [], "peak_memory": [],
                    "output_bytes": len(css)}
                if extra:
                    row["map_bytes"] = Path(str(out) + ".map").stat().st_size
                for _ in range(samples):
                    wall, peak, _ = timed_process(cmd)
                    row["wall"].append(wall)
                    row["peak_memory"].append(peak)
                results.append(row)
    return results


def measure(weave: Path, bench_exe: Path, args, tmp: Path, samples: int, cold_samples: int) -> dict:
    """One measurement of one build: corpus, in-process and cold samples."""
    corpus = tmp / "corpus"
    if corpus.exists():
        shutil.rmtree(corpus)
    listing = subprocess.run(
        [str(bench_exe), "generate", str(corpus)], check=True, capture_output=True, text=True
    ).stdout
    entries = dict(line.split("\t") for line in listing.splitlines() if line)
    raw = tmp / "inprocess.json"
    cmd = [str(bench_exe), "run", "--samples", str(samples), "--warmup", str(args.warmup), "--out", str(raw)]
    for name in args.workload or []:
        cmd += ["--workload", name]
    subprocess.run(cmd, check=True, capture_output=True)
    in_process = json.loads(raw.read_text())
    return {
        "corpus": corpus_hashes(corpus),
        "in_process": in_process,
        "cold": cold(weave, corpus, entries, in_process, cold_samples),
    }


def merge(parts: list) -> dict:
    """Concatenates the samples of several measurements of one build."""
    first = parts[0]
    for part in parts[1:]:
        if part["corpus"] != first["corpus"]:
            raise SystemExit("the corpus changed between rounds")
        for wa, wb in zip(first["in_process"]["workloads"], part["in_process"]["workloads"]):
            if wa["output_fnv1a64"] != wb["output_fnv1a64"]:
                raise SystemExit(f"{wa['name']}: output changed between rounds")
            for sa, sb in zip(wa["scenarios"], wb["scenarios"]):
                for key in ("total", "load", "eval", "emit"):
                    sa[key] += sb[key]
        for ca, cb in zip(first["cold"], part["cold"]):
            ca["wall"] += cb["wall"]
            ca["peak_memory"] += cb["peak_memory"]
    first["in_process"]["samples"] = sum(p["in_process"]["samples"] for p in parts)
    return first


def binaries(tree: Path) -> tuple[Path, Path]:
    target = tree / "target" / "release"
    # The binary was called `weave` before 0.1.0-rc.2.
    cli = target / f"weavec{EXE}"
    return (cli if cli.exists() else target / f"weave{EXE}"), target / f"weave-bench{EXE}"


def run(args) -> None:
    trees = [("after", ROOT)]
    if args.baseline:
        trees.insert(0, ("before", Path(args.baseline).resolve()))
    if not args.no_build:
        for _, tree in trees:
            subprocess.run(
                ["cargo", "build", "--release", "--locked", "-p", cli_package(tree), "-p", "weave-bench"],
                cwd=tree,
                check=True,
            )
    rounds = max(1, args.rounds)
    samples = max(1, args.samples // rounds)
    cold_samples = max(1, args.cold_samples // rounds)
    parts: dict = {label: [] for label, _ in trees}
    with tempfile.TemporaryDirectory() as tmp:
        for r in range(rounds):
            # Alternate the order so slow drift affects both builds alike.
            order = trees if r % 2 == 0 else list(reversed(trees))
            for label, tree in order:
                print(f"round {r + 1}/{rounds}: {label}", file=sys.stderr)
                weave, bench_exe = binaries(tree)
                parts[label].append(measure(weave, bench_exe, args, Path(tmp), samples, cold_samples))
    env = environment()
    for label, tree in trees:
        data = merge(parts[label])
        revision = subprocess.run(
            ["git", "rev-parse", "HEAD"], cwd=tree, capture_output=True, text=True, check=False
        ).stdout.strip()
        report = {
            "label": label,
            "environment": dict(env, revision=revision),
            "method": {
                "rounds": rounds,
                "interleaved_with": [other for other, _ in trees if other != label],
                "in_process_samples": samples * rounds,
                "cold_samples": cold_samples * rounds,
                "warmup_per_round": args.warmup,
            },
            "corpus": data["corpus"],
            "in_process": data["in_process"],
            "cold_process": {
                "samples": cold_samples * rounds,
                "cache_state": "process cold, OS file cache warm (one untimed build first in each round)",
                "scenarios": data["cold"],
            },
        }
        out = Path(args.out_dir) / f"{label}.json"
        out.write_text(json.dumps(report, indent=1) + "\n", encoding="utf-8", newline="\n")
        print(f"wrote {out}")


def stats(values: list) -> dict:
    values = sorted(v for v in values if v is not None)
    if not values:
        return {}
    n = len(values)
    q = statistics.quantiles(values, n=4, method="inclusive") if n > 1 else [values[0]] * 3
    mean = statistics.fmean(values)
    sd = statistics.stdev(values) if n > 1 else 0.0
    return {
        "n": n,
        "min": values[0],
        "median": statistics.median(values),
        "p95": values[min(n - 1, max(0, -(-95 * n // 100) - 1))],
        "max": values[-1],
        "q1": q[0],
        "q3": q[2],
        "mean": mean,
        "cv_pct": 100 * sd / mean if mean else 0.0,
    }


def ms(ns: float) -> str:
    return f"{ns / 1e6:.3f}"


def rows(report: dict) -> dict:
    """(key) -> dict of stats for every measured series in a report."""
    out = {}
    for scenario in report["cold_process"]["scenarios"]:
        out[("cold", scenario["name"], "wall")] = stats(scenario["wall"])
        out[("cold", scenario["name"], "peak")] = stats(scenario["peak_memory"])
    for workload in report["in_process"]["workloads"]:
        for scenario in workload["scenarios"]:
            for part in ("total", "load", "eval", "emit"):
                out[("in", f"{workload['name']}/{scenario['name']}", part)] = stats(scenario[part])
    return out


def summary(args) -> None:
    report = json.loads(Path(args.report).read_text())
    env = report["environment"]
    print(f"# {report['label']} — {env.get('revision', '?')[:12]}\n")
    print("| series | n | median ms | p95 ms | IQR ms | CV % |")
    print("|---|---:|---:|---:|---:|---:|")
    for (kind, name, part), s in rows(report).items():
        if not s or part == "peak":
            continue
        if kind == "in" and part != "total":
            continue
        print(f"| {kind}: {name} | {s['n']} | {ms(s['median'])} | {ms(s['p95'])} | "
              f"{ms(s['q1'])}–{ms(s['q3'])} | {s['cv_pct']:.1f} |")


def compare(args) -> None:
    before = json.loads(Path(args.before).read_text())
    after = json.loads(Path(args.after).read_text())
    a, b = rows(before), rows(after)
    print(f"Before: {before['label']} ({before['environment']['revision'][:12]}); "
          f"after: {after['label']} ({after['environment']['revision'][:12]})\n")
    outputs = {w["name"]: w["output_fnv1a64"] for w in before["in_process"]["workloads"]}
    for w in after["in_process"]["workloads"]:
        same = outputs.get(w["name"]) == w["output_fnv1a64"]
        print(f"- {w['name']}: output {'byte-identical' if same else 'DIFFERS'} "
              f"({w['output_bytes']} bytes, FNV-1a 64 {w['output_fnv1a64']})")
    if before["corpus"] != after["corpus"]:
        print("- WARNING: the corpus differs between the two reports")
    print()
    print("| series | before median | after median | change | before IQR | after IQR | verdict |")
    print("|---|---:|---:|---:|---:|---:|---|")
    for key in a:
        if key not in b or not a[key] or not b[key]:
            continue
        kind, name, part = key
        if kind == "in" and part != "total":
            continue
        x, y = a[key], b[key]
        fmt = (lambda v: f"{v / 2**20:.1f} MiB") if part == "peak" else (lambda v: f"{ms(v)} ms")
        change = (y["median"] - x["median"]) / x["median"] * 100 if x["median"] else 0.0
        overlap = not (y["q3"] < x["q1"] or y["q1"] > x["q3"])
        verdict = "within noise (IQRs overlap)" if overlap else ("faster" if change < 0 else "slower")
        if part == "peak":
            if abs(change) < 1.0:
                verdict = "unchanged (<1%)"
            else:
                verdict = "within noise (IQRs overlap)" if overlap else ("less" if change < 0 else "more")
        label = f"{kind}: {name}" + (" peak memory" if part == "peak" else "")
        print(f"| {label} | {fmt(x['median'])} | {fmt(y['median'])} | {change:+.1f}% | "
              f"{fmt(x['q1'])}–{fmt(x['q3'])} | {fmt(y['q1'])}–{fmt(y['q3'])} | {verdict} |")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    p = sub.add_parser("run")
    p.add_argument("--out-dir", required=True, help="writes after.json (and before.json)")
    p.add_argument("--baseline", help="a checkout of the baseline revision, measured interleaved")
    p.add_argument("--rounds", type=int, default=4)
    p.add_argument("--samples", type=int, default=30)
    p.add_argument("--warmup", type=int, default=3)
    p.add_argument("--cold-samples", type=int, default=20)
    p.add_argument("--workload", action="append")
    p.add_argument("--no-build", action="store_true")
    p.set_defaults(func=run)
    p = sub.add_parser("summary")
    p.add_argument("report")
    p.set_defaults(func=summary)
    p = sub.add_parser("compare")
    p.add_argument("before")
    p.add_argument("after")
    p.set_defaults(func=compare)
    args = parser.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
