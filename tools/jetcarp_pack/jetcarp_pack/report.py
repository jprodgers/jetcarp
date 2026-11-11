"""What a check found: failures, warnings, skips and counts, each with where it was found."""

import json
from collections import Counter
from dataclasses import asdict, dataclass

FAIL, WARN, SKIP, INFO = "FAIL", "WARN", "SKIP", "INFO"
_ORDER = {FAIL: 0, WARN: 1, SKIP: 2, INFO: 3}


@dataclass
class Finding:
    level: int  # conformance level the check belongs to (0 to 3)
    severity: str  # FAIL, WARN, SKIP or INFO
    code: str  # a short, stable name for the check, such as "object-hash"
    where: str  # what was being checked: a path, a manifest, a table, a row
    message: str

    def line(self):
        return f"{self.severity} L{self.level} {self.code}: {self.where}: {self.message}"


class Report:
    def __init__(self, max_findings_per_code=200):
        self.findings = []
        self.passed = Counter()  # (level, what) -> how many passed
        self._per_code = Counter()
        self._suppressed = Counter()
        self._seen = set()
        self.max_per_code = max_findings_per_code

    def add(self, level, severity, code, where, message):
        finding = (level, severity, code, where, message)
        if finding in self._seen:  # the same object read again by another level
            return
        self._seen.add(finding)
        key = (severity, code)
        self._per_code[key] += 1
        if self._per_code[key] > self.max_per_code:
            self._suppressed[key] += 1
            return
        self.findings.append(Finding(level, severity, code, where, message))

    def fail(self, level, code, where, message):
        self.add(level, FAIL, code, where, message)

    def warn(self, level, code, where, message):
        self.add(level, WARN, code, where, message)

    def skip(self, level, code, where, message):
        self.add(level, SKIP, code, where, message)

    def info(self, level, code, where, message):
        self.add(level, INFO, code, where, message)

    def ok(self, level, what, n=1):
        self.passed[(level, what)] += n

    def count(self, severity):
        return sum(n for (sev, _), n in self._per_code.items() if sev == severity)

    def failed(self):
        return any(sev == FAIL for sev, _ in self._per_code)

    def codes(self, severity=FAIL):
        return {code for sev, code in self._per_code if sev == severity}

    def text(self, verbose=False):
        lines = []
        shown = [f for f in self.findings if verbose or f.severity in (FAIL, WARN, SKIP)]
        for f in sorted(shown, key=lambda f: (f.level, _ORDER[f.severity])):
            lines.append(f.line())
        for (sev, code), n in sorted(self._suppressed.items()):
            lines.append(f"{sev} {code}: {n} more not shown")
        if self.passed:
            lines.append("passed:")
            for (level, what), n in sorted(self.passed.items()):
                lines.append(f"  L{level} {what}: {n}")
        totals = {sev: sum(n for (s, _), n in self._per_code.items() if s == sev) for sev in _ORDER}
        lines.append("result: " + ("FAILED" if self.failed() else "ok") + " (" +
                     ", ".join(f"{n} {sev.lower()}" for sev, n in totals.items() if n) + ")")
        return "\n".join(lines)

    def as_json(self):
        return json.dumps({
            "ok": not self.failed(),
            "findings": [asdict(f) for f in self.findings],
            "suppressed": {f"{s} {c}": n for (s, c), n in self._suppressed.items()},
            "passed": {f"L{level} {what}": n for (level, what), n in sorted(self.passed.items())},
        }, indent=1, ensure_ascii=False)
