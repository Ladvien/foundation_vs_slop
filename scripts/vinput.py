#!/usr/bin/env python3
"""**A virtual keyboard and pointer**, for driving the game the way a person would.

Three of the Site editor's bugs were invisible to a green test suite and were found only by rendering
a frame and looking at it (see `docs/2026-08-03-emerge-mapper-implementation.md` §4). But *reaching*
the frame worth looking at meant pressing keys and clicking — and this host has no `xdotool`, no
`wtype`, no `ydotool`, and runs Wayland, where the X11 tools would not have worked anyway.

It does have `/dev/uinput`, granted to this user by ACL (`user:ladvien:rw-`). So this creates a real
kernel input device. The compositor cannot tell it from a physical keyboard and mouse, which is the
point: events travel the whole path — libinput, the compositor, winit, Bevy's `ButtonInput` — rather
than being poked into the ECS behind the input system's back. A test that bypasses the input path
cannot find a bug in the input path, and the editor has already shipped two of those (a `TabGroup`
that ate WASD, and a second camera that silently killed nine systems).

**No dependencies.** `ctypes` + `struct` against the uinput ABI, so it runs anywhere the repo does.

Usage:

    ./scripts/vinput.py key f7                    # tap
    ./scripts/vinput.py hold w 1.5                # press, wait, release
    ./scripts/vinput.py move 0.5 0.5              # pointer to screen centre (fractions)
    ./scripts/vinput.py click 0.5 0.5             # move then left-click
    ./scripts/vinput.py script run.txt            # a timeline, one command per line
    ./scripts/vinput.py script -                  # ... from stdin

Script lines are the same verbs plus `sleep <seconds>` and `shot <name>` (which touches
`screenshot.request` and waits for `screenshot.png`, then renames it). `#` starts a comment.

Absolute pointer coordinates are **fractions of the screen**, 0.0–1.0. The kernel device advertises a
0–65535 absolute range and libinput maps that onto the output, so fractions are the only honest unit
here — pixels would silently mean something different on another display.
"""

import ctypes
import fcntl
import os
import shutil
import struct
import sys
import time
from pathlib import Path

# ---------------------------------------------------------------------------------------------
# uinput ABI. Values from linux/input-event-codes.h and linux/uinput.h.
# ---------------------------------------------------------------------------------------------

EV_SYN, EV_KEY, EV_REL, EV_ABS = 0x00, 0x01, 0x02, 0x03
SYN_REPORT = 0
ABS_X, ABS_Y = 0x00, 0x01
REL_WHEEL = 0x08
BTN_LEFT, BTN_RIGHT, BTN_MIDDLE = 0x110, 0x111, 0x112

ABS_CNT = 64
UINPUT_MAX_NAME_SIZE = 80

# _IOW('U', nr, int) == 0x40045500 | nr ; _IO('U', nr) == 0x5500 | nr
UI_DEV_CREATE = 0x5501
UI_DEV_DESTROY = 0x5502
UI_SET_EVBIT = 0x40045564
UI_SET_KEYBIT = 0x40045565
UI_SET_RELBIT = 0x40045566
UI_SET_ABSBIT = 0x40045567

# The absolute range the device advertises. libinput maps it onto the output's extents, so the number
# itself is arbitrary — only the resolution matters, and 16 bits is finer than any display.
ABS_MAX = 65535

#: Names this tool accepts → Linux keycodes. Deliberately only the keys the game and editor bind, so a
#: typo is an error naming the key rather than a silently wrong keypress.
KEYS = {
    # letters
    "a": 30, "b": 48, "c": 46, "d": 32, "e": 18, "f": 33, "g": 34, "h": 35, "i": 23,
    "j": 36, "k": 37, "l": 38, "m": 50, "n": 49, "o": 24, "p": 25, "q": 16, "r": 19,
    "s": 31, "t": 20, "u": 22, "v": 47, "w": 17, "x": 45, "y": 21, "z": 44,
    # digits
    "0": 11, "1": 2, "2": 3, "3": 4, "4": 5, "5": 6, "6": 7, "7": 8, "8": 9, "9": 10,
    # function row
    "f1": 59, "f2": 60, "f3": 61, "f4": 62, "f5": 63, "f6": 64,
    "f7": 65, "f8": 66, "f9": 67, "f10": 68, "f11": 87, "f12": 88,
    # modifiers and friends
    "esc": 1, "escape": 1, "enter": 28, "return": 28, "space": 57, "tab": 15,
    "backspace": 14, "delete": 111,
    "ctrl": 29, "lctrl": 29, "rctrl": 97,
    "shift": 42, "lshift": 42, "rshift": 54,
    "alt": 56, "lalt": 56, "ralt": 100,
    "super": 125, "cmd": 125,
    # punctuation the editor uses for rotate/scale nudges
    "lbracket": 26, "[": 26, "rbracket": 27, "]": 27,
    "minus": 12, "-": 12, "equal": 13, "=": 13,
    "comma": 51, ",": 51, "period": 52, ".": 52, "slash": 53, "/": 53,
    # arrows
    "up": 103, "down": 108, "left": 105, "right": 106,
}

BUTTONS = {"left": BTN_LEFT, "right": BTN_RIGHT, "middle": BTN_MIDDLE}


class uinput_user_dev(ctypes.Structure):
    """The legacy device-setup struct written to the fd before `UI_DEV_CREATE`.

    Preferred over `UI_DEV_SETUP` because it is one write with no per-axis ioctls and has been stable
    since forever; there is nothing here that needs the newer interface.
    """

    _fields_ = [
        ("name", ctypes.c_char * UINPUT_MAX_NAME_SIZE),
        ("bustype", ctypes.c_uint16),
        ("vendor", ctypes.c_uint16),
        ("product", ctypes.c_uint16),
        ("version", ctypes.c_uint16),
        ("ff_effects_max", ctypes.c_uint32),
        ("absmax", ctypes.c_int32 * ABS_CNT),
        ("absmin", ctypes.c_int32 * ABS_CNT),
        ("absfuzz", ctypes.c_int32 * ABS_CNT),
        ("absflat", ctypes.c_int32 * ABS_CNT),
    ]


class VirtualInput:
    """A kernel input device that types and points.

    Keyboard and pointer are **one device** rather than two. A pointer that is a separate device from
    the keyboard is normal hardware, but one device is fewer things to create, settle, and destroy, and
    libinput is happy to treat a combined device as both.
    """

    def __init__(self, name: str = "fvs-virtual-input", settle: float = 0.6):
        self.fd = os.open("/dev/uinput", os.O_WRONLY | os.O_NONBLOCK)

        for ev in (EV_KEY, EV_ABS, EV_REL, EV_SYN):
            fcntl.ioctl(self.fd, UI_SET_EVBIT, ev)
        for code in sorted(set(KEYS.values())):
            fcntl.ioctl(self.fd, UI_SET_KEYBIT, code)
        for code in BUTTONS.values():
            fcntl.ioctl(self.fd, UI_SET_KEYBIT, code)
        for code in (ABS_X, ABS_Y):
            fcntl.ioctl(self.fd, UI_SET_ABSBIT, code)
        fcntl.ioctl(self.fd, UI_SET_RELBIT, REL_WHEEL)

        dev = uinput_user_dev()
        dev.name = name.encode()
        dev.bustype = 0x03  # BUS_USB — a device claiming to be virtual gets treated as one
        dev.vendor, dev.product, dev.version = 0x1234, 0x5678, 1
        dev.absmax[ABS_X] = ABS_MAX
        dev.absmax[ABS_Y] = ABS_MAX
        os.write(self.fd, bytes(memoryview(dev)))
        fcntl.ioctl(self.fd, UI_DEV_CREATE)

        # The compositor needs a moment to notice a new device; events written before it does are
        # delivered nowhere and look exactly like a bug in the thing under test.
        time.sleep(settle)
        self._pos = (0.5, 0.5)

    # -- raw ------------------------------------------------------------------------------------

    def _emit(self, etype: int, code: int, value: int) -> None:
        # struct input_event on 64-bit: struct timeval (2 × long) + u16 + u16 + s32.
        os.write(self.fd, struct.pack("llHHi", 0, 0, etype, code, value))

    def _sync(self) -> None:
        self._emit(EV_SYN, SYN_REPORT, 0)

    # -- keyboard -------------------------------------------------------------------------------

    def key_down(self, name: str) -> None:
        self._emit(EV_KEY, self._code(name), 1)
        self._sync()

    def key_up(self, name: str) -> None:
        self._emit(EV_KEY, self._code(name), 0)
        self._sync()

    def tap(self, name: str, hold: float = 0.06) -> None:
        """Press and release. `hold` is long enough that a 60 Hz frame loop cannot miss it — a tap
        shorter than one frame is a real risk with `just_pressed`."""
        self.key_down(name)
        time.sleep(hold)
        self.key_up(name)
        time.sleep(0.04)

    def hold(self, name: str, seconds: float) -> None:
        self.key_down(name)
        time.sleep(seconds)
        self.key_up(name)
        time.sleep(0.04)

    def chord(self, *names: str, hold: float = 0.06) -> None:
        """Press several keys together (`ctrl z`), release in reverse."""
        for n in names:
            self._emit(EV_KEY, self._code(n), 1)
        self._sync()
        time.sleep(hold)
        for n in reversed(names):
            self._emit(EV_KEY, self._code(n), 0)
        self._sync()
        time.sleep(0.04)

    @staticmethod
    def _code(name: str) -> int:
        key = name.lower()
        if key not in KEYS:
            raise SystemExit(
                f"vinput: unknown key {name!r}. Known: {', '.join(sorted(KEYS))}"
            )
        return KEYS[key]

    # -- pointer --------------------------------------------------------------------------------

    def move(self, fx: float, fy: float) -> None:
        """Absolute pointer position, as a fraction of the screen in each axis."""
        fx, fy = min(max(fx, 0.0), 1.0), min(max(fy, 0.0), 1.0)
        self._emit(EV_ABS, ABS_X, int(fx * ABS_MAX))
        self._emit(EV_ABS, ABS_Y, int(fy * ABS_MAX))
        self._sync()
        self._pos = (fx, fy)
        time.sleep(0.05)

    def button(self, which: str = "left", hold: float = 0.08) -> None:
        code = BUTTONS.get(which)
        if code is None:
            raise SystemExit(f"vinput: unknown button {which!r}")
        self._emit(EV_KEY, code, 1)
        self._sync()
        time.sleep(hold)
        self._emit(EV_KEY, code, 0)
        self._sync()
        time.sleep(0.05)

    def click(self, fx: float, fy: float, which: str = "left") -> None:
        # Move, settle, then click. Clicking in the same event batch as the move can beat the
        # application's own cursor tracking, so the click lands where the pointer *was*.
        self.move(fx, fy)
        time.sleep(0.12)
        self.button(which)

    def drag(self, fx0: float, fy0: float, fx1: float, fy1: float, steps: int = 12) -> None:
        """Press at one point, move in steps, release at another.

        Stepped rather than teleported because a drag handler that samples the cursor per frame needs
        intermediate positions — one jump looks like a click somewhere else.
        """
        self.move(fx0, fy0)
        time.sleep(0.1)
        self._emit(EV_KEY, BTN_LEFT, 1)
        self._sync()
        for i in range(1, steps + 1):
            t = i / steps
            self.move(fx0 + (fx1 - fx0) * t, fy0 + (fy1 - fy0) * t)
        time.sleep(0.1)
        self._emit(EV_KEY, BTN_LEFT, 0)
        self._sync()
        time.sleep(0.05)

    def scroll(self, clicks: int) -> None:
        for _ in range(abs(clicks)):
            self._emit(EV_REL, REL_WHEEL, 1 if clicks > 0 else -1)
            self._sync()
            time.sleep(0.03)

    def close(self) -> None:
        fcntl.ioctl(self.fd, UI_DEV_DESTROY)
        os.close(self.fd)

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()


# ---------------------------------------------------------------------------------------------
# Screenshots
# ---------------------------------------------------------------------------------------------


def shot(name: str, cwd: Path, timeout: float = 20.0) -> Path:
    """Ask the running game for a frame and wait for it.

    `src/devshot.rs` watches for a `screenshot.request` sentinel and writes `screenshot.png` on the
    next frame. Waiting for the file to *appear* rather than sleeping a fixed time is the difference
    between a capture rig and a race — the game may be loading, and a screenshot of a half-loaded
    frame is how `tests/visual_golden.rs` came to be pinned to one (see the memory note).
    """
    out = cwd / "screenshot.png"
    if out.exists():
        out.unlink()
    (cwd / "screenshot.request").touch()
    deadline = time.time() + timeout
    while time.time() < deadline:
        if out.exists() and out.stat().st_size > 0:
            # The game writes then closes; give the write a beat to finish before renaming.
            time.sleep(0.4)
            dest = cwd / name if name.endswith(".png") else cwd / f"{name}.png"
            dest.parent.mkdir(parents=True, exist_ok=True)
            # `shutil.move`, not `Path.replace`: shots usually land in a scratch directory on a
            # different filesystem, and `os.replace` across devices is `EXDEV`, not a copy.
            shutil.move(str(out), str(dest))
            print(f"vinput: captured {dest}")
            return dest
        time.sleep(0.15)
    raise SystemExit(
        f"vinput: no screenshot after {timeout}s — is the game running in {cwd}, "
        "and was it built with debug assertions (devshot is dev-only)?"
    )


# ---------------------------------------------------------------------------------------------
# Script runner
# ---------------------------------------------------------------------------------------------


def run_script(vi: VirtualInput, lines, cwd: Path) -> None:
    for lineno, raw in enumerate(lines, 1):
        line = raw.split("#", 1)[0].strip()
        if not line:
            continue
        parts = line.split()
        verb, args = parts[0], parts[1:]
        try:
            if verb == "key":
                vi.tap(args[0])
            elif verb == "chord":
                vi.chord(*args)
            elif verb == "hold":
                vi.hold(args[0], float(args[1]))
            elif verb == "move":
                vi.move(float(args[0]), float(args[1]))
            elif verb == "click":
                vi.click(float(args[0]), float(args[1]), args[2] if len(args) > 2 else "left")
            elif verb == "drag":
                vi.drag(float(args[0]), float(args[1]), float(args[2]), float(args[3]))
            elif verb == "scroll":
                vi.scroll(int(args[0]))
            elif verb == "sleep":
                time.sleep(float(args[0]))
            elif verb == "shot":
                shot(args[0], cwd)
            elif verb == "echo":
                print("vinput:", " ".join(args))
            else:
                raise SystemExit(f"vinput: line {lineno}: unknown verb {verb!r}")
        except IndexError:
            raise SystemExit(f"vinput: line {lineno}: not enough arguments for {verb!r}")
        print(f"  [{lineno}] {line}", flush=True)


def main() -> None:
    argv = sys.argv[1:]
    if not argv:
        raise SystemExit(__doc__)
    cwd = Path.cwd()
    verb, args = argv[0], argv[1:]

    with VirtualInput() as vi:
        if verb == "script":
            src = sys.stdin if args and args[0] == "-" else open(args[0])
            with src as fh:
                run_script(vi, fh.readlines(), cwd)
        else:
            run_script(vi, [" ".join(argv)], cwd)


if __name__ == "__main__":
    main()
