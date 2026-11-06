#!/usr/bin/env python3
"""Owns org.freedesktop.secrets on the bus at argv[1], and answers nothing: a keyring waiting on a prompt nobody is at.
For keyring-sandbox.sh, on its private bus only."""

import os
import socket
import struct
import sys


def pad(data, to):
    return data + b"\0" * (-len(data) % to)


def string(text):
    raw = text.encode()
    return struct.pack("<I", len(raw)) + raw + b"\0"


def field(code, signature, value):
    return bytes([code]) + bytes([len(signature)]) + signature.encode() + b"\0" + value


def call(serial, member, body=b"", signature=""):
    fields = [
        (1, "o", string("/org/freedesktop/DBus")),
        (2, "s", string("org.freedesktop.DBus")),
        (3, "s", string(member)),
        (6, "s", string("org.freedesktop.DBus")),
    ]
    if signature:
        fields.append((8, "g", bytes([len(signature)]) + signature.encode() + b"\0"))
    array = b""
    for code, kind, value in fields:
        array = pad(array, 8) + field(code, kind, value)
    head = b"l\x01\0\x01" + struct.pack("<II", len(body), serial) + struct.pack("<I", len(array)) + array
    return pad(head, 8) + body


address = sys.argv[1]
if address != "/run/k/bus":
    sys.exit("stub: that isn't the sandbox's bus")
bus = socket.socket(socket.AF_UNIX)
bus.connect(address)
bus.sendall(b"\0AUTH EXTERNAL " + str(os.getuid()).encode().hex().encode() + b"\r\n")
assert bus.recv(4096).startswith(b"OK")
bus.sendall(b"BEGIN\r\n")
bus.sendall(call(1, "Hello"))
name = pad(string("org.freedesktop.secrets"), 4) + struct.pack("<I", 4)
bus.sendall(call(2, "RequestName", name, "su"))
while bus.recv(65536):
    pass
