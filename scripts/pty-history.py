#!/usr/bin/env python3
"""Isolated real PTY; SIGUSR1 resizes to 30 columns, SIGUSR2 restores 80."""
import errno
import fcntl
import os
import pty
import select
import signal
import struct
import sys
import termios
pid, master = pty.fork()
if pid == 0:
    fcntl.ioctl(1, termios.TIOCSWINSZ, struct.pack("HHHH", 110, 80, 0, 0))
    os.execv(sys.argv[1], [sys.argv[1], "panel"])
def resize(columns, rows=110):
    fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", rows, columns, 0, 0))
    os.kill(pid, signal.SIGWINCH)
signal.signal(signal.SIGUSR1, lambda *_: resize(30))
signal.signal(signal.SIGUSR2, lambda *_: resize(80))
signal.signal(signal.SIGALRM, lambda *_: resize(30, 42))
signal.signal(signal.SIGWINCH, lambda *_: resize(80, 42))
readers = [master, 0]
reaped = False
try:
    while master in readers:
        ready, _, _ = select.select(readers, [], [], 2)
        for descriptor in ready:
            try:
                data = os.read(descriptor, 65536)
            except OSError as error:
                if descriptor == master and error.errno == errno.EIO:
                    data = b""
                else:
                    raise
            if not data:
                readers.remove(descriptor)
                continue
            target = 1 if descriptor == master else master
            while data:
                data = data[os.write(target, data):]
    _, status = os.waitpid(pid, 0)
    reaped = True
    sys.exit(os.waitstatus_to_exitcode(status))
finally:
    os.close(master)
    if not reaped:
        try:
            os.kill(pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
