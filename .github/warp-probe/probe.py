#!/usr/bin/env python3
"""TEMPORARY (#321): JETCARP's GPU code on Windows' WARP, several ways, and how each ended.

Run by .github/workflows/warp-probe.yml on the investigation branch only. Never exits non-zero:
the summary at the end is the result.
"""

import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import time

TOOLS = pathlib.Path(os.environ["WARP_TOOLS"])
CDB = pathlib.Path(r"C:\Program Files (x86)\Windows Kits\10\Debuggers\x64\cdb.exe")
SUMMARY = []


def code_text(code):
    if code is None:
        return "timed out"
    return f"{code} (0x{code & 0xFFFFFFFF:08X})"


def run(label, cmd, env, timeout=1500, cwd=None):
    print(f"::group::{label}", flush=True)
    t0 = time.time()
    try:
        p = subprocess.run(cmd, env=env, timeout=timeout, stdout=subprocess.PIPE,
                           stderr=subprocess.STDOUT, text=True, errors="replace", cwd=cwd)
        code, out = p.returncode, p.stdout
    except subprocess.TimeoutExpired as e:
        code = None
        out = e.stdout.decode(errors="replace") if isinstance(e.stdout, bytes) else (e.stdout or "")
    print(out, flush=True)
    print("::endgroup::", flush=True)
    line = f"{label}: exit {code_text(code)} after {time.time() - t0:.0f} s"
    print("RESULT " + line, flush=True)
    SUMMARY.append(line)
    return code, out


def test_exes(pkgs):
    cmd = ["cargo", "test", "--release", "--no-run", "--message-format=json"]
    for p in pkgs:
        cmd += ["-p", p]
    out = subprocess.run(cmd, check=True, stdout=subprocess.PIPE, text=True).stdout
    exes = []
    for line in out.splitlines():
        try:
            m = json.loads(line)
        except ValueError:
            continue
        if m.get("reason") == "compiler-artifact" and m.get("executable") and m["profile"]["test"]:
            exes.append((str(pathlib.Path(m["manifest_path"]).parent), m["target"]["name"],
                         m["executable"]))
    return exes


def sweep(variant, env, exe, rounds):
    """The probe, again and again, each time leaving out the steps that crashed it before."""
    skip, crashed = [], []
    for r in range(rounds):
        e = dict(env, JETCARP_WARP_PROBE_SKIP="\n".join(skip))
        code, out = run(f"probe [{variant}] round {r + 1}",
                        [exe, "--ignored", "--nocapture", "--test-threads=1", "--exact",
                         "every_kernel_on_warp"], e)
        if "PROBE DONE" in out:
            done = re.findall(r"^PROBE DONE.*$", out, re.M)
            SUMMARY.append(f"probe [{variant}]: {done[-1] if done else ''}")
            break
        begun = re.findall(r"^BEGIN (.*)$", out, re.M)
        open_ = [b for b in begun if f"END {b}: " not in out]
        if not open_:
            SUMMARY.append(f"probe [{variant}]: ended outside any step")
            break
        last = open_[-1]
        crashed.append((last, code_text(code)))
        if last.startswith("making"):
            SUMMARY.append(f"probe [{variant}]: crashed while {last}")
            break
        skip.append(last)
    for label, code in crashed:
        SUMMARY.append(f"CRASH [{variant}] {code}: {label}")
    return [c[0] for c in crashed]


def under_cdb(variant, exe, env, label):
    if not CDB.exists():
        SUMMARY.append("cdb: not installed")
        return
    script = TOOLS / f"cdb-{variant}.txt"
    syms = TOOLS / "symbols"
    script.write_text(
        f".sympath srv*{syms}*https://msdl.microsoft.com/download/symbols\n"
        f".sympath+ {pathlib.Path(exe).parent}\n"
        'sxe -c ".echo FIRST-CHANCE AV; .lastevent; r; kn 40; gn" '
        '-c2 ".echo SECOND-CHANCE AV; .lastevent; r; kn 80; .echo THREADS; ~*kn 16; '
        'lmvm d3d10warp; q" av\n'
        "g\n"
    )
    e = dict(env, JETCARP_WARP_PROBE="=" + label)
    run(f"cdb [{variant}] {label}",
        [str(CDB), "-G", "-lines", "-cf", str(script), exe, "--ignored", "--nocapture",
         "--test-threads=1", "--exact", "every_kernel_on_warp"], e, timeout=2400)


def with_new_warp(exe):
    """A copy of a test program beside NuGet's WARP, which its own folder takes over the system's."""
    d = pathlib.Path(exe).parent / "newwarp"
    d.mkdir(exist_ok=True)
    warp_bin = TOOLS / "warp" / "build" / "native" / "bin" / "x64"
    for f in ("d3d10warp.dll", "d3d10warp.pdb"):
        shutil.copy2(warp_bin / f, d / f)
    copy = d / pathlib.Path(exe).name
    shutil.copy2(exe, copy)
    return str(copy)


def fxc_exe():
    kits = pathlib.Path(r"C:\Program Files (x86)\Windows Kits\10\bin")
    found = sorted(kits.glob("10.*/x64/fxc.exe"))
    return found[-1] if found else None


def main():
    base = dict(os.environ, JETCARP_GPU="software", JETCARP_ALLOW_WARP="1",
                JETCARP_REQUIRE_GPU="1", WGPU_DX12_COMPILER="fxc", RUST_BACKTRACE="1")
    dxc_bin = TOOLS / "dxc" / "build" / "native" / "bin" / "x64"
    dxc = dict(base, WGPU_DX12_COMPILER="dxc", PATH=f"{dxc_bin}{os.pathsep}{base['PATH']}")
    sdk = TOOLS / "d3d12" / "build" / "native" / "bin" / "x64"
    checks = dict(WGPU_DX12_AGILITY_SDK_PATH=f"{sdk}\\", WGPU_DX12_AGILITY_SDK_VERSION="620",
                  WGPU_DX12_AGILITY_SDK_REQUIRE="1", WGPU_VALIDATION="1", WGPU_DEBUG="1",
                  WGPU_GPU_BASED_VALIDATION="1")

    # The soup hunt's generated kernel, opened on WARP directly (no self-test).
    hunt = next(e for c, n, e in test_exes(["jetcarp-search"]) if n == "warp_hunt_probe")
    new_hunt = with_new_warp(hunt)
    args = ["--ignored", "--nocapture", "--exact", "the_hunt_kernel_on_warp"]
    code, _ = run("hunt [system FXC]", [hunt] + args, base, 900)
    run("hunt [NuGet FXC]", [new_hunt] + args, base, 900)
    run("hunt [system DXC]", [hunt] + args, dxc, 900)
    if code != 0 and CDB.exists():
        script = TOOLS / "cdb-hunt.txt"
        script.write_text(
            f".sympath srv*{TOOLS / 'symbols'}*https://msdl.microsoft.com/download/symbols\n"
            'sxe -c ".echo FIRST-CHANCE AV; .lastevent; r; kn 12; gn" '
            '-c2 ".echo SECOND-CHANCE AV; kn 12; q" av\n'
            "g\n"
        )
        run("cdb hunt [system FXC]", [str(CDB), "-G", "-cf", str(script), hunt] + args, base, 1800)
    # wgpu's HLSL of the hunt kernel and FXC's DXBC, for the record.
    hl = TOOLS / "hlsl-hunt"
    run("hunt [system FXC, HLSL kept]", [hunt] + args, dict(base, JETCARP_WARP_HLSL_DIR=str(hl)), 900)
    fxc = fxc_exe()
    asm = TOOLS / "asm"
    asm.mkdir(exist_ok=True)
    for f in sorted(hl.glob("*.hlsl")) if fxc else []:
        subprocess.run([str(fxc), "/nologo", "/T", "cs_5_1", "/E", f.stem, "/Ges", "/Fc",
                        str(asm / f"hunt--{f.stem}.asm"), str(f)])
        shutil.copy2(f, asm / f"hunt--{f.stem}.hlsl")

    print("\n==== SUMMARY ====")
    for line in SUMMARY:
        print(line)
    sys.stdout.flush()


def under_cdb_jit(case, exe, env):
    if not CDB.exists():
        SUMMARY.append("cdb: not installed")
        return
    script = TOOLS / "cdb-jit.txt"
    syms = TOOLS / "symbols"
    script.write_text(
        f".sympath srv*{syms}*https://msdl.microsoft.com/download/symbols\n"
        'sxe -c ".echo FIRST-CHANCE AV; .lastevent; r; kn 20; ub @rip L8; u @rip L4; gn" '
        '-c2 ".echo SECOND-CHANCE AV; kn 20; q" av\n'
        "g\n"
    )
    run(f"cdb jit [system FXC] {case}",
        [str(CDB), "-G", "-lines", "-cf", str(script), exe, "--ignored", "--nocapture",
         "--exact", "jit_child"], dict(env, JETCARP_WARP_JIT=case), timeout=1800)


if __name__ == "__main__":
    main()
