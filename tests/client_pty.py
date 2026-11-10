#!/usr/bin/env python3
"""Exercise the real client through a controlling PTY, using only the stdlib."""
import errno
import fcntl
import json
import os
import select
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time
import unittest
import faulthandler
from pathlib import Path

BINARY = str(Path(sys.argv.pop(1) if len(sys.argv) > 1 else "target/debug/saumux").resolve())
faulthandler.dump_traceback_later(15)

class ClientTerminalTest(unittest.TestCase):
    def test_detach_reattach_and_signal_restore_terminal(self):
        with tempfile.TemporaryDirectory(prefix="sx-pty-") as directory:
            env = dict(os.environ, XDG_RUNTIME_DIR=directory, XDG_CONFIG_HOME=directory,
                       SHELL="/bin/sh", TERM="xterm-256color", LANG="en_US.UTF-8")
            env.pop("SAUMUX", None)
            def control(*args):
                return subprocess.run([BINARY, "ctl", "--session", "ptytest", *args], env=env,
                                      capture_output=True, text=True, check=True).stdout
            def launch(*args):
                master, slave = os.openpty()
                fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 100, 0, 0))
                original = termios.tcgetattr(slave)
                def child_setup():
                    os.setsid()
                    fcntl.ioctl(0, termios.TIOCSCTTY, 0)
                process = subprocess.Popen([BINARY, *args], stdin=slave, stdout=slave, stderr=slave,
                                           env=env, preexec_fn=child_setup, close_fds=True)
                return process, master, slave, original
            def read_until(master, needle):
                data = bytearray()
                deadline = time.monotonic() + 5
                while needle not in data:
                    self.assertLess(time.monotonic(), deadline, repr(data[-1500:]))
                    if select.select([master], [], [], .1)[0]:
                        try:
                            chunk = os.read(master, 65536)
                        except OSError as exc:
                            if exc.errno == errno.EIO:
                                self.fail(f"client exited: {data!r}")
                            raise
                        self.assertTrue(chunk, repr(data))
                        data.extend(chunk)
                return data
            def wait_client(process, master):
                deadline = time.monotonic() + 5
                while process.poll() is None:
                    self.assertLess(time.monotonic(), deadline, "client did not exit")
                    if select.select([master], [], [], .05)[0]:
                        try:
                            os.read(master, 65536)
                        except OSError:
                            break
                process.wait(timeout=2)
            process = None
            try:
                process, master, slave, original = launch("new", "ptytest", "--", "/bin/sh")
                read_until(master, b"System")
                self.assertFalse(termios.tcgetattr(slave)[3] & termios.ICANON)
                os.write(master, b"printf 'CLIENT_PTY_OK\\n'\r")
                read_until(master, b"CLIENT_PTY_OK")
                os.write(master, b"\x1bd")
                wait_client(process, master)
                self.assertEqual(process.returncode, 0)
                self.assertEqual(termios.tcgetattr(master), original)
                self.assertEqual(len(json.loads(control("list"))), 1)
                os.close(master); os.close(slave)
                process, master, slave, original = launch("attach", "ptytest")
                read_until(master, b"CLIENT_PTY_OK")
                process.send_signal(signal.SIGTERM)
                wait_client(process, master)
                self.assertEqual(process.returncode, 0)
                self.assertEqual(termios.tcgetattr(master), original)
                self.assertEqual(len(json.loads(control("list"))), 1)
                os.close(master); os.close(slave)
            finally:
                if process and process.poll() is None:
                    process.kill()
                    os.close(master); os.close(slave)
                    process.wait(timeout=2)
                subprocess.run([BINARY, "ctl", "--session", "ptytest", "session.end", "--confirm"],
                               env=env, capture_output=True)

if __name__ == "__main__":
    unittest.main()
