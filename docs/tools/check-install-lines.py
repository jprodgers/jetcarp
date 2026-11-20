#!/usr/bin/env python3
"""Check that every copy of the Windows install and uninstall lines saves the installer to the temp folder.

The PowerShell installer is saved to a file and then run, and the lines that do it are written in
the README, the book, the release notes (packaging/package.py), the installer's own usage text and
`jetcarp uninstall`. A window that opens in C:\\Windows\\system32 can't save a file into its current
folder, so every copy saves to and runs from "$env:TEMP\\jetcarp-installer.ps1" (#143). This finds
each line in the repository that saves or runs jetcarp-installer.ps1 and fails on any that doesn't.
CI's own test lines, which name the runner's folders, are left out.

Standard library only. Usage: python3 docs/tools/check-install-lines.py
"""

import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent
# A line that saves the installer (-OutFile) or runs it (-File), in any of the files' quoting.
SAVES_OR_RUNS = re.compile(r"-(?:Out)?File\s+\S*jetcarp-installer\.ps1")
# The temp folder, as PowerShell reads it, with the backslash written once or escaped once.
IN_TEMP = re.compile(r"\$env:TEMP\\{1,2}jetcarp-installer\.ps1")
CI_ONLY = ("RUNNER_TEMP", "-Installer ")


def main():
    files = subprocess.run(["git", "ls-files"], cwd=REPO, capture_output=True, text=True, check=True).stdout
    bad, n = [], 0
    for name in files.splitlines():
        path = REPO / name
        if path.suffix not in (".md", ".ps1", ".py", ".rs", ".sh") or not path.is_file():
            continue
        try:
            lines = path.read_text(encoding="utf-8").splitlines()
        except UnicodeDecodeError:
            continue
        for i, line in enumerate(lines, 1):
            if not SAVES_OR_RUNS.search(line) or any(c in line for c in CI_ONLY):
                continue
            n += 1
            if not IN_TEMP.search(line):
                bad.append(f"{name}:{i}: {line.strip()}")
    if bad:
        print('these lines save or run the installer outside "$env:TEMP" (#143):', file=sys.stderr)
        for b in bad:
            print("  " + b, file=sys.stderr)
        sys.exit(1)
    print(f"ok: all {n} lines that save or run jetcarp-installer.ps1 use the temp folder")


if __name__ == "__main__":
    main()
