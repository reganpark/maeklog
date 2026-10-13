#!/usr/bin/env python3
"""Capture the shipped TUI at a requested fixture-only terminal size."""
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
    fcntl.ioctl(1, termios.TIOCSWINSZ, struct.pack("HHHH", int(sys.argv[3]), int(sys.argv[2]), 0, 0))
    os.execv(sys.argv[1], [sys.argv[1], "panel"])

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
