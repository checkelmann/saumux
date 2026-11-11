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
                output = bytearray()
                while process.poll() is None or select.select([master], [], [], 0)[0]:
                    self.assertLess(time.monotonic(), deadline, f"client did not exit: {process.poll()}, {output[-600:]!r}")
                    if select.select([master], [], [], .05)[0]:
                        try:
                            chunk = os.read(master, 65536)
                            if not chunk:
                                break
                            output.extend(chunk)
                        except OSError:
                            break
                process.wait(timeout=2)
                return bytes(output)
            process = None
            try:
                process, master, slave, original = launch("new", "ptytest", "--", "/bin/sh")
                startup = read_until(master, b"System")
                self.assertIn(b"\x1b[>1u", startup)
                self.assertFalse(termios.tcgetattr(slave)[3] & termios.ICANON)
                os.write(master, b"printf 'CLIENT_PTY_OK\\n'\r")
                read_until(master, b"CLIENT_PTY_OK")
                record = Path(directory) / "recorded-key"
                recorder = "import os,sys,tty,time; from pathlib import Path; tty.setraw(0); p=Path(sys.argv[1]); p.with_suffix('.ready').write_text('ready'); data=b''\nwhile len(data)<7: data+=os.read(0,7-len(data))\np.with_suffix('.hex').write_text(data.hex()); time.sleep(60)"
                control("window.new", "--", "python3", "-u", "-c", recorder, str(record))
                def await_file(path):
                    deadline = time.monotonic() + 5
                    while not path.exists() or path.stat().st_size == 0:
                        self.assertLess(time.monotonic(), deadline, f"missing {path}")
                        if select.select([master], [], [], .05)[0]:
                            os.read(master, 65536)
                    return path.read_text()
                await_file(record.with_suffix('.ready'))
                os.write(master, b"\x1b[13;2u")
                self.assertEqual(await_file(record.with_suffix('.hex')), "1b5b31333b3275")
                control("window.close")
                os.write(master, b"\x1bd")
                restored = wait_client(process, master)
                self.assertEqual(restored.count(b"\x1b[<1u"), 1)
                self.assertEqual(process.returncode, 0)
                self.assertEqual(termios.tcgetattr(master), original)
                self.assertEqual(len(json.loads(control("list"))), 1)
                os.close(master); os.close(slave)
                process, master, slave, original = launch("attach", "ptytest")
                read_until(master, b"CLIENT_PTY_OK")
                process.send_signal(signal.SIGTERM)
                restored = wait_client(process, master)
                self.assertEqual(restored.count(b"\x1b[<1u"), 1)
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
