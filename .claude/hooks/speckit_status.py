#!/usr/bin/env python3
"""Compute the current Spec Kit / project state for the closing recap (IDEE.md §6).

Pure stdlib. Prints a French Markdown checklist. Used by recap_guard.py and usable by hand:
    python .claude/hooks/speckit_status.py
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TASK_RE = re.compile(r"^\s*- \[( |x|X)\] (T\d{3})")
PHASE_RE = re.compile(r"^##\s+(Phase\s+\d+[^\n]*)")


def constitution_state() -> str:
    path = ROOT / ".specify" / "memory" / "constitution.md"
    if not path.exists():
        return "absente"
    text = path.read_text(encoding="utf-8", errors="replace")
    if re.search(r"\[[A-Z_]{3,}\]", text):
        return "modèle non rempli"
    version = re.search(r"\*\*Version\*\*:\s*([0-9.]+)", text)
    return f"ratifiée (v{version.group(1)})" if version else "présente"


def feature_state(feature: Path) -> tuple[str, list[str]]:
    lines: list[str] = []
    spec = feature / "spec.md"
    plan = feature / "plan.md"
    tasks = feature / "tasks.md"
    stage = "créée"
    if spec.exists():
        stage = "spécifiée"
        if "## Clarifications" in spec.read_text(encoding="utf-8", errors="replace"):
            stage = "clarifiée"
    if plan.exists():
        stage = "planifiée"
    if tasks.exists():
        stage = "tâches"
        done = total = 0
        phases: list[tuple[str, int, int]] = []
        current = None
        for raw in tasks.read_text(encoding="utf-8", errors="replace").splitlines():
            m = PHASE_RE.match(raw)
            if m:
                current = [m.group(1).strip(), 0, 0]
                phases.append(current)  # type: ignore[arg-type]
                continue
            t = TASK_RE.match(raw)
            if t:
                total += 1
                checked = t.group(1).lower() == "x"
                done += checked
                if current is not None:
                    current[2] += 1
                    current[1] += checked
        if total and done == total:
            stage = "implémentée"
            if "Convergence" in tasks.read_text(encoding="utf-8", errors="replace"):
                stage = "convergée"
        lines.append(f"  - tâches : {done}/{total}")
        for name, d, t in phases:
            mark = "x" if t and d == t else " "
            lines.append(f"  - [{mark}] {name} — {d}/{t}")
    return stage, lines


def open_bugs() -> list[str]:
    bug_dir = ROOT / "bug"
    out: list[str] = []
    if not bug_dir.is_dir():
        return out
    for fiche in sorted(bug_dir.glob("*.md")):
        if fiche.name in {"README.md", "_modele.md"}:
            continue
        text = fiche.read_text(encoding="utf-8", errors="replace")
        m = re.search(r"^etat:\s*(\S+)", text, re.MULTILINE)
        state = m.group(1) if m else "inconnu"
        if state not in {"corrige", "ferme"}:
            out.append(f"{fiche.name} ({state})")
    return out


def main() -> int:
    out = ["**État Spec Kit (calculé)**", f"- Constitution : {constitution_state()}"]
    specs = ROOT / "specs"
    features = sorted(p for p in specs.iterdir() if p.is_dir()) if specs.is_dir() else []
    if not features:
        out.append("- Aucune spec")
    for feature in features:
        stage, lines = feature_state(feature)
        out.append(f"- `{feature.name}` : {stage}")
        out.extend(lines)
    bugs = open_bugs()
    out.append(f"- Bugs ouverts : {', '.join(bugs) if bugs else 'aucun'}")
    sys.stdout.reconfigure(encoding="utf-8")  # type: ignore[attr-defined]
    sys.stdout.write("\n".join(out) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
