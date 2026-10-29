#!/usr/bin/env python3
"""Claude Code Stop hook: every AI run must end with the closing recap (IDEE.md §6).

Blocks the stop (decision "block") when the last assistant message has no
"Récapitulatif de clôture" heading, and feeds back the computed Spec Kit state.
Never blocks twice in a row (stop_hook_active), never fails the session.
"""
from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

MARKER = "Récapitulatif de clôture"


def main() -> int:
    try:
        payload = json.loads(sys.stdin.buffer.read().decode("utf-8", "replace"))
    except Exception:
        return 0
    if payload.get("stop_hook_active"):
        return 0
    message = payload.get("last_assistant_message") or ""
    if MARKER.lower() in message.lower():
        return 0
    status_script = Path(__file__).with_name("speckit_status.py")
    try:
        state = subprocess.run(
            [sys.executable, str(status_script)],
            capture_output=True, text=True, encoding="utf-8", timeout=20, check=False,
        ).stdout
    except Exception:
        state = "(état non calculable)"
    reason = (
        f"Termine par le « {MARKER} » (IDEE.md §6, constitution) : une checklist en français, "
        "lisible en diagonale, avec ce qui est fait [x], ce qui reste à faire [ ], "
        "l'état Spec Kit courant et les bugs ouverts. État calculé :\n\n" + state
    )
    sys.stdout.buffer.write(json.dumps({"decision": "block", "reason": reason}, ensure_ascii=False).encode("utf-8"))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
