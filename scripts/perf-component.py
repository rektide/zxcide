#!/usr/bin/env python3
"""Record one component Criterion case with Linux perf and Wasmtime jitdump."""
import argparse
import json
import os
from pathlib import Path
from subprocess import check_output, run
from tempfile import mkdtemp


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("case", nargs="?", default="component/upstream-l7/decode")
    parser.add_argument("--seconds", type=int, default=5)
    args = parser.parse_args()
    if args.seconds < 1:
        parser.error("--seconds must be positive")

    root = Path(__file__).resolve().parents[1]
    target = (root / os.environ.get("CARGO_TARGET_DIR", "target")).resolve()
    env = os.environ.copy()
    if "ZXC_BENCH_INPUT" in env:
        env["ZXC_BENCH_INPUT"] = str(Path(env["ZXC_BENCH_INPUT"]).resolve())
    run([root / "scripts/build-component.sh"], check=True)
    build = check_output([
        "cargo", "bench", "--locked", "--no-run", "--message-format=json-render-diagnostics",
        "-p", "zxc-component", "--bench", "component", "--features", "host-perf",
    ], cwd=root, text=True)
    bench, = (msg["executable"] for msg in map(json.loads, build.splitlines())
              if msg.get("target", {}).get("kind") == ["bench"] and msg.get("executable"))

    env["ZXC_COMPONENT"] = str(target / "component/zxc-codec.wasm")
    cmd = [bench, "--bench", "--exact", args.case]
    cases = check_output([*cmd, "--list"], cwd=root, env=env, text=True)
    if f"{args.case}: benchmark" not in cases.splitlines():
        parser.error("no such case with the current ZXC_BENCH_MODE and ZXC_BENCH_BLOCK_LOG")

    out = Path(mkdtemp(prefix="component-perf-", dir=target))
    print(f"Capture: {out}", flush=True)
    env["ZXC_JITDUMP"] = "1"
    run(["perf", "record", "-k", "mono", "-F", "997", "-e", "cycles:u", "--",
         *cmd, "--profile-time", str(args.seconds)], cwd=out, env=env, check=True)
    run(["perf", "inject", "--jit", "-i", "perf.data", "-o", "perf.jit.data"], cwd=out, check=True)
    print(f"Report: perf report -i {out / 'perf.jit.data'}")


if __name__ == "__main__":
    main()
