"""Smoke test of the real cmd_ai TUI in a pseudo-terminal (Linux and macOS).

    python3 tests/tui_drive.py <path/to/cmd_ai>

Runs two sessions in a pty with a throw-away cmd_ai folder (no network needed):
1. a terminal that answers cursor position queries (like a real emulator): the inline TUI
   must open, show the status bar, run /aide, /bypass, /quitter and exit with code 0;
2. a terminal that never answers: cmd_ai must explain it and fall back to the text-mode
   loop, which handles the same commands.
Exits 1 on the first failed expectation.
"""
import fcntl
import os
import pty
import re
import select
import shutil
import struct
import sys
import tempfile
import termios
import time

CONFIG = '{"version":1,"provider":{"base_url":"http://127.0.0.1:9/v1","model":"modele-test"}}\n'


def drive(binary, home, answer_dsr, steps, limit=25.0):
    pid, fd = pty.fork()
    if pid == 0:
        fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 100, 0, 0))
        os.environ["TERM"] = "xterm-256color"
        os.environ.pop("OLLAMA_API_KEY", None)
        os.execv(binary, [binary, "--home", home])
    pending = list(steps)
    start = time.time()
    out = b""
    while True:
        r, _, _ = select.select([fd], [], [], 0.05)
        if fd in r:
            try:
                data = os.read(fd, 65536)
            except OSError:
                break
            if not data:
                break
            out += data
            if answer_dsr:
                for _ in range(data.count(b"\x1b[6n")):
                    os.write(fd, b"\x1b[10;1R")
        now = time.time() - start
        while pending and now >= pending[0][0]:
            os.write(fd, pending.pop(0)[1])
        if now > limit:
            os.kill(pid, 9)
            break
    _, status = os.waitpid(pid, 0)
    code = os.waitstatus_to_exitcode(status) if hasattr(os, "waitstatus_to_exitcode") else status >> 8
    text = re.sub(r"\x1b\[[0-9;?]*[a-zA-Z]", "", out.decode("utf-8", "replace"))
    return code, text


def check(label, code, text, expected):
    ok = code == 0 and all(e in text for e in expected) and "panicked" not in text
    missing = [e for e in expected if e not in text]
    print(f"{label}: exit={code} {'OK' if ok else 'FAILED'} {'' if ok else missing}")
    if not ok:
        print(text[-2000:])
    return ok


def main():
    binary = os.path.abspath(sys.argv[1])
    home = tempfile.mkdtemp(prefix="cmd_ai_tui_")
    try:
        with open(os.path.join(home, "config.json"), "w", encoding="utf-8") as f:
            f.write(CONFIG)
        keys = [b"/aide\r", b"/bypass\r", b"/quitter\r"]
        # Real TUI: inputs from 1.5 s.
        code, text = drive(binary, home, True, [(1.5 + i, k) for i, k in enumerate(keys)])
        ok = check("inline TUI", code, text, ["PLAN", "BYPASS", "Commandes :", "bientôt", "Votre demande"])
        # Silent terminal: the probe waits ~2 s for the cursor answer, so type later.
        code, text = drive(binary, home, False, [(4.0 + i, k) for i, k in enumerate(keys)])
        ok = check("text-mode fallback", code, text,
                   ["passage en mode texte", "PLAN", "Commandes :", "Mode BYPASS", "bientôt"]) and ok
        sys.exit(0 if ok else 1)
    finally:
        shutil.rmtree(home, ignore_errors=True)


if __name__ == "__main__":
    main()
