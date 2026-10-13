#!/usr/bin/env python3
"""Isolated UI fixture: 80x40, photo-like 80x42, and narrow 30x28."""
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
    fcntl.ioctl(1, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 80, 0, 0))
    os.execv(sys.argv[1], [sys.argv[1], "panel"])


def resize(columns, rows):
    fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", rows, columns, 0, 0))
    os.kill(pid, signal.SIGWINCH)


signal.signal(signal.SIGUSR1, lambda *_: resize(30, 28))
signal.signal(signal.SIGUSR2, lambda *_: resize(80, 40))
signal.signal(signal.SIGALRM, lambda *_: resize(80, 42))
readers, reaped = [master, 0], False
try:
    while master in readers:
        for descriptor in select.select(readers, [], [], 2)[0]:
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
