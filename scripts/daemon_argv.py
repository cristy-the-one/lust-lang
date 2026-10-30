"""How to start the compiler process itself.

On Unix, `lust daemon` execs `lust-core`, so the process you spawned is the
daemon. On Windows there is no exec: `lust daemon` would spawn `lust-core`
and wait, and the spawned pid would be the client. Tests that name the
daemon by pid (its temp dir, its RSS) start `lust-core` directly.
"""
import os


def lust_bin(path):
    """`path` if it exists, else `path.exe` on Windows."""
    if os.path.isfile(path):
        return path
    exe = path + ".exe"
    if os.name == "nt" and os.path.isfile(exe):
        return exe
    return path


def daemon_argv(lust):
    """`[compiler, "daemon"]` for this host."""
    if os.name == "nt":
        base = os.path.dirname(os.path.abspath(lust))
        for name in ("lust-core.exe", "lust-core"):
            core = os.path.join(base, name)
            if os.path.isfile(core):
                return [core, "daemon"]
    return [lust, "daemon"]
