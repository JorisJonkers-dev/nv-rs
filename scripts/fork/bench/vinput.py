"""A virtual keyboard and pointer for the nested compositor, spoken over the Wayland wire protocol (standard library
only): zwp_virtual_keyboard_v1 with a real xkb keymap, so the game gets true evdev scancodes (wtype gives each key its own
private keycode, which the game reads as the wrong key), and zwlr_virtual_pointer_v1 for relative motion and buttons.

It is created before the game starts and lives for the whole run: a seat with no keyboard when the game binds it would
never be given one. Only ever connect it to the nested compositor's socket, never to the owner's session.
"""
import os
import socket
import struct
import subprocess
import tempfile
import time

EVDEV = {
    "esc": 1, "1": 2, "2": 3, "3": 4, "4": 5, "5": 6, "6": 7, "7": 8, "8": 9, "9": 10, "0": 11, "minus": 12,
    "backspace": 14, "tab": 15, "q": 16, "w": 17, "e": 18, "r": 19, "t": 20, "y": 21, "u": 22, "i": 23, "o": 24, "p": 25,
    "enter": 28, "ctrl": 29, "a": 30, "s": 31, "d": 32, "f": 33, "g": 34, "h": 35, "j": 36, "k": 37, "l": 38,
    "grave": 41, "shift": 42, "z": 44, "x": 45, "c": 46, "v": 47, "b": 48, "n": 49, "m": 50, "comma": 51, "period": 52,
    "slash": 53, "alt": 56, "space": 57, "f5": 63, "f9": 67, "up": 103, "left": 105, "right": 106, "down": 108,
}
BTN = {"left": 0x110, "right": 0x111, "middle": 0x112}


def pad(b):
    return b + b"\0" * (-len(b) % 4)


def string_arg(s):
    b = s.encode() + b"\0"
    return struct.pack("<I", len(b)) + pad(b)


def message(obj, opcode, body=b""):
    return struct.pack("<II", obj, ((8 + len(body)) << 16) | opcode) + body


def parse_events(buf):
    """Split a receive buffer into ((object, opcode, body) events, the rest)."""
    out = []
    while len(buf) >= 8:
        obj, w = struct.unpack_from("<II", buf)
        size = w >> 16
        if size < 8 or len(buf) < size:
            break
        out.append((obj, w & 0xFFFF, buf[8:size]))
        buf = buf[size:]
    return out, buf


def parse_global(body):
    name, n = struct.unpack_from("<II", body)
    iface = body[8:8 + n - 1].decode()
    version = struct.unpack_from("<I", body, 8 + len(pad(b"x" * n)))[0]
    return name, iface, version


def keymap_text():
    tool = None
    import glob
    for p in sorted(glob.glob("/nix/store/*-libxkbcommon-*/bin/xkbcli")):
        tool = p
    tool = os.environ.get("NV_BENCH_XKBCLI") or tool or "xkbcli"
    r = subprocess.run([tool, "compile-keymap", "--layout", "us"], capture_output=True, text=True, timeout=20)
    if r.returncode != 0 or "xkb_keymap" not in r.stdout:
        raise RuntimeError("no xkb keymap (xkbcli compile-keymap failed): nix shell nixpkgs#libxkbcommon")
    return r.stdout


class VirtualInput:
    def __init__(self, path):
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.connect(path)
        self.next_id = 2
        self.buf = b""
        self.t0 = time.monotonic()

    def new_id(self):
        self.next_id += 1
        return self.next_id - 1

    def send(self, data, fds=()):
        if fds:
            self.sock.sendmsg([data], [(socket.SOL_SOCKET, socket.SCM_RIGHTS, struct.pack("i" * len(fds), *fds))])
        else:
            self.sock.sendall(data)

    def read_events(self, timeout=5.0):
        self.sock.settimeout(timeout)
        data = self.sock.recv(65536)
        ev, self.buf = parse_events(self.buf + data)
        return ev

    def roundtrip(self):
        cb = self.new_id()
        self.send(message(1, 0, struct.pack("<I", cb)))
        while True:
            for obj, op, _ in self.read_events():
                if obj == cb:
                    return

    def start(self, keymap=None):
        reg = self.new_id()
        self.send(message(1, 1, struct.pack("<I", reg)))
        globs = {}
        cb = self.new_id()
        self.send(message(1, 0, struct.pack("<I", cb)))
        done = False
        while not done:
            for obj, op, body in self.read_events():
                if obj == reg and op == 0:
                    name, iface, ver = parse_global(body)
                    globs.setdefault(iface, (name, ver))
                elif obj == cb:
                    done = True
        for need in ("wl_seat", "zwp_virtual_keyboard_manager_v1", "zwlr_virtual_pointer_manager_v1"):
            if need not in globs:
                raise RuntimeError(f"the compositor has no {need}")

        def bind(iface, ver):
            oid = self.new_id()
            name, v = globs[iface]
            self.send(message(reg, 0, struct.pack("<I", name) + string_arg(iface) + struct.pack("<II", min(ver, v), oid)))
            return oid

        seat = bind("wl_seat", 1)
        kmgr, pmgr = bind("zwp_virtual_keyboard_manager_v1", 1), bind("zwlr_virtual_pointer_manager_v1", 1)
        self.kbd, self.ptr = self.new_id(), self.new_id()
        self.send(message(kmgr, 0, struct.pack("<II", seat, self.kbd)))
        self.send(message(pmgr, 0, struct.pack("<II", seat, self.ptr)))
        text = (keymap or keymap_text()).encode() + b"\0"
        with tempfile.TemporaryFile() as f:
            f.write(text)
            f.flush()
            self.send(message(self.kbd, 0, struct.pack("<II", 1, len(text))), fds=[f.fileno()])
        self.roundtrip()

    def ms(self):
        return int((time.monotonic() - self.t0) * 1000) & 0xFFFFFFFF

    def key(self, name, down):
        self.send(message(self.kbd, 1, struct.pack("<III", self.ms(), EVDEV[name], 1 if down else 0)))

    def hold(self, names, seconds):
        for n in names:
            self.key(n, True)
        time.sleep(seconds)
        for n in reversed(names):
            self.key(n, False)

    def tap(self, name, seconds=0.15):
        self.hold((name,), seconds)

    def move(self, dx, dy):
        self.send(message(self.ptr, 0, struct.pack("<Iii", self.ms(), int(dx * 256), int(dy * 256))) + message(self.ptr, 4))

    def click(self, button="left", seconds=0.1):
        self.send(message(self.ptr, 2, struct.pack("<III", self.ms(), BTN[button], 1)) + message(self.ptr, 4))
        time.sleep(seconds)
        self.send(message(self.ptr, 2, struct.pack("<III", self.ms(), BTN[button], 0)) + message(self.ptr, 4))

    def run(self, steps):
        """("hold", (keys), s), ("tap", key), ("move", dx, dy, count, gap_s), ("click", button), ("wait", s)."""
        for st in steps:
            k = st[0]
            if k == "hold":
                self.hold(st[1], st[2])
            elif k == "tap":
                self.tap(st[1])
            elif k == "move":
                for _ in range(st[3] if len(st) > 3 else 1):
                    self.move(st[1], st[2])
                    time.sleep(st[4] if len(st) > 4 else 0.01)
            elif k == "click":
                self.click(st[1])
            elif k == "wait":
                time.sleep(st[1])
            else:
                raise ValueError(k)

    def close(self):
        try:
            self.sock.close()
        except OSError:
            pass
