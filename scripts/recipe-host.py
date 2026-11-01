#!/usr/bin/env python3
"""A scripted recipe host for scripts/test-recipes.sh (tests/recipes/README.md).

Runs one built recipe: sends the start line, answers each call, and writes
the transcript (every line in both directions) to stdout. The answers are a
deterministic function of the calls, so a recipe built by lust and by Cargo
must produce the same transcript.

Usage: recipe-host.py <recipe binary> '<params json>'
"""
import json
import subprocess
import sys


def main():
    exe, params = sys.argv[1], json.loads(sys.argv[2])
    p = subprocess.Popen([exe], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env={})
    start = json.dumps({"v": 1, "params": params}, separators=(",", ":"))
    p.stdin.write(start + "\n")
    p.stdin.flush()
    log = ["> " + start]
    volume = 0.0
    handles = 0
    while True:
        line = p.stdout.readline()
        if not line:
            break
        line = line.rstrip("\n")
        log.append("< " + line)
        msg = json.loads(line)
        if "done" in msg:
            break
        call, args = msg["call"], msg["args"]
        if call == "solid_new":
            handles += 1
            sx, sy, sz = args["size"]
            volume = sx * sy * sz
            reply = {"id": msg["id"], "ok": True, "result": f"h{handles}"}
        elif call == "apply":
            op = args["op"]
            if op["kind"] == "hole":
                volume -= 3.14159 * op["radius"] ** 2 * 4.0
            elif op["kind"] == "pocket":
                volume -= op["across"] ** 2 * 0.65 * op["depth"]
            # Every seventh call is refused, as a kernel refuses a bad edit.
            ok = msg["id"] % 7 != 0
            reply = {"id": msg["id"], "ok": ok, "result": None} if ok else {"id": msg["id"], "ok": False, "error": "the edit does not fit"}
        elif call == "measure":
            reply = {"id": msg["id"], "ok": True, "result": round(volume, 3)}
        else:
            reply = {"id": msg["id"], "ok": False, "error": f"{call} is not allowed"}
        text = json.dumps(reply, separators=(",", ":"))
        log.append("> " + text)
        p.stdin.write(text + "\n")
        p.stdin.flush()
    p.stdin.close()
    err = p.stderr.read()
    code = p.wait()
    log.append(f"stderr: {err.strip()}")
    log.append(f"exit: {code}")
    print("\n".join(log))


if __name__ == "__main__":
    main()
