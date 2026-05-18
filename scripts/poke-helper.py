#!/usr/bin/env python3
"""Smoke test: spawn the helper and round-trip the Chrome Native Messaging
frame protocol. Lives alongside the .bat/.ps1 versions; used when we want
to verify the protocol independent of PowerShell's pipe handling.
"""
from __future__ import annotations

import json
import os
import struct
import subprocess
import sys
import time
import uuid
from pathlib import Path


def helper_path() -> Path:
    if len(sys.argv) > 1:
        return Path(sys.argv[1])
    local = os.environ.get("LOCALAPPDATA")
    if local:
        p = Path(local) / "w3stream" / "helper.exe"
        if p.exists():
            return p
    # Fall back to the cargo build output for local development.
    repo = Path(__file__).resolve().parent.parent
    return repo / "target" / "x86_64-pc-windows-msvc" / "release" / "w3stream-helper.exe"


def send(stdin, obj: dict) -> None:
    body = json.dumps(obj, separators=(",", ":")).encode("utf-8")
    stdin.write(struct.pack("<I", len(body)))
    stdin.write(body)
    stdin.flush()


def recv(stdout) -> dict:
    lb = stdout.read(4)
    if len(lb) != 4:
        raise RuntimeError(f"short read on length prefix: {lb!r}")
    (n,) = struct.unpack("<I", lb)
    body = stdout.read(n)
    if len(body) != n:
        raise RuntimeError(f"short body read: got {len(body)} expected {n}")
    return json.loads(body.decode("utf-8"))


def main() -> int:
    h = helper_path()
    if not h.exists():
        print(f"[!] helper not found at {h}", file=sys.stderr)
        return 2
    print(f"[+] Spawning {h}")
    proc = subprocess.Popen(
        [str(h)],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        bufsize=0,
        creationflags=0x08000000 if os.name == "nt" else 0,  # CREATE_NO_WINDOW
    )
    try:
        hello = recv(proc.stdout)
        print(f"[+] hello: version={hello['version']} gamepad.available={hello['gamepad']['available']}")
        print(f"    vigem_status={hello['gamepad']['vigem_status']}")
        print(f"    hidhide_status={hello['gamepad']['hidhide_status']}")
        print(f"    actions={[a['action_id'] for a in hello['actions']]}")

        send(proc.stdin, {"requestId": "h1", "command": "health"})
        h = recv(proc.stdout)
        print(f"[+] health: {h['result']}")

        send(proc.stdin, {"requestId": "list-1", "command": "actions.list"})
        h = recv(proc.stdout)
        print(f"[+] actions.list ok ({len(h['result']['actions'])} actions)")

        send(proc.stdin, {"requestId": "en-1", "command": "enabled", "params": {"enabled": True}})
        h = recv(proc.stdout)
        print(f"[+] enabled: {h['result']}")

        # Fire test_type_hi only if the user explicitly asked (no focused window in CI/headless).
        if "--type" in sys.argv:
            print("[+] firing test_type_hi in 3s; focus a text window NOW")
            time.sleep(3)
            send(proc.stdin, {
                "requestId": "trig-1",
                "command": "trigger",
                "params": {"action_id": "test_type_hi", "request_id": str(uuid.uuid4())},
            })
            h = recv(proc.stdout)
            print(f"[+] trigger: {h.get('result') or h.get('error')}")

        send(proc.stdin, {"requestId": "panic-1", "command": "panic"})
        h = recv(proc.stdout)
        print(f"[+] panic: {h['result']}")

        proc.stdin.close()
        proc.wait(timeout=3)
        print(f"[+] Done. exit={proc.returncode}")
        return 0
    except Exception as e:
        print(f"[!] {type(e).__name__}: {e}", file=sys.stderr)
        proc.kill()
        return 1


if __name__ == "__main__":
    sys.exit(main())
