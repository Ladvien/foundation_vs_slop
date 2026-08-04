#!/usr/bin/env python3
"""Drive the editor on macOS the way scripts/vinput.py drives it on the Wayland box.

vinput.py needs /dev/uinput and does not work here. This is the macOS half.
See docs/2026-08-04-emerge-mapper-handoff.md §4 for the two traps it encodes: a chord needs a
real modifier key-down (flags alone arrive as the bare key), and CG coordinates differ from window
coordinates by a constant Y offset on this display.

Run the editor against a COPY of assets/ - a stray Enter lands on "add to library".

Quartz CGEvent, so events travel the real input path (WindowServer -> winit -> Bevy ButtonInput)
rather than being poked into the ECS. Coordinates are POINTS in global display space; the editor is
run borderless-fullscreen for these runs so screen points == window points.
"""
import sys
import time

import Quartz

# macOS virtual keycodes
VK = {
    "f": 0x03, "z": 0x06, "x": 0x07, "c": 0x08, "s": 0x01, "n": 0x2D,
    "g": 0x05, "r": 0x0F, "t": 0x11, "o": 0x1F, "w": 0x0D, "a": 0x00,
    "d": 0x02, "q": 0x0C, "e": 0x0E, "1": 0x12, "2": 0x13,
    "lbracket": 0x21, "rbracket": 0x1E, "delete": 0x33, "tab": 0x30,
    "b": 0x0B, "h": 0x04, "i": 0x22, "j": 0x26, "k": 0x28, "l": 0x25,
    "m": 0x2E, "p": 0x23, "u": 0x20, "v": 0x09, "y": 0x10,
    "3": 0x14, "4": 0x15, "5": 0x17, "6": 0x16, "7": 0x1A, "8": 0x1C,
    "9": 0x19, "0": 0x1D, "enter": 0x24, "down": 0x7D, "up": 0x7E, "escape": 0x35,
    # The Tiles tab binds left/right to "which list the arrows walk".
    "left": 0x7B, "right": 0x7C,
    "lbracket2": 0x21, "rbracket2": 0x1E, "space": 0x31,
}
CTRL = Quartz.kCGEventFlagMaskControl
CMD = Quartz.kCGEventFlagMaskCommand


# **Measured, not guessed.** CG global-display coordinates and the app's window coordinates differ by
# a constant band at the top of this display (notch/menu-bar area): sending CG y=97 arrived in Bevy as
# y=71. Callers pass the coordinate they read off a frame (window/UI logical px) and this puts it back
# into CG space. Big targets absorbed the error for a long time; a 23pt-tall filter box did not.
Y_OFFSET = 26


def move(x, y):
    y = y + Y_OFFSET
    Quartz.CGWarpMouseCursorPosition(Quartz.CGPointMake(x, y))
    # A warp does not generate a move event; post one so the app's cursor_position updates.
    ev = Quartz.CGEventCreateMouseEvent(
        None, Quartz.kCGEventMouseMoved, Quartz.CGPointMake(x, y), 0
    )
    Quartz.CGEventPost(Quartz.kCGHIDEventTap, ev)
    time.sleep(0.15)


def click(x, y):
    move(x, y)
    for kind in (Quartz.kCGEventLeftMouseDown, Quartz.kCGEventLeftMouseUp):
        ev = Quartz.CGEventCreateMouseEvent(None, kind, Quartz.CGPointMake(x, y + Y_OFFSET), 0)
        Quartz.CGEventPost(Quartz.kCGHIDEventTap, ev)
        time.sleep(0.06)


# Real modifier KEY codes. Setting only CGEventSetFlags marks the event's modifier state but never
# produces a ControlLeft key-down, and Bevy's ButtonInput<KeyCode> is built from actual key events —
# so a flags-only chord arrives in the app as the bare key. Press the physical modifier too.
MODS = {"ctrl": (0x3B, Quartz.kCGEventFlagMaskControl),
        "cmd": (0x37, Quartz.kCGEventFlagMaskCommand)}


def _post(code, down, flags):
    ev = Quartz.CGEventCreateKeyboardEvent(None, code, down)
    Quartz.CGEventSetFlags(ev, flags)
    Quartz.CGEventPost(Quartz.kCGHIDEventTap, ev)
    time.sleep(0.06)


def drag(x0, y0, x1, y1, steps=12):
    """Press at A, glide to B, release — real intermediate moves, so the app sees a drag."""
    move(x0, y0)
    ev = Quartz.CGEventCreateMouseEvent(
        None, Quartz.kCGEventLeftMouseDown, Quartz.CGPointMake(x0, y0 + Y_OFFSET), 0)
    Quartz.CGEventPost(Quartz.kCGHIDEventTap, ev)
    time.sleep(0.15)
    for i in range(1, steps + 1):
        x = x0 + (x1 - x0) * i / steps
        y = y0 + (y1 - y0) * i / steps
        ev = Quartz.CGEventCreateMouseEvent(
            None, Quartz.kCGEventLeftMouseDragged, Quartz.CGPointMake(x, y + Y_OFFSET), 0)
        Quartz.CGEventPost(Quartz.kCGHIDEventTap, ev)
        time.sleep(0.04)
    ev = Quartz.CGEventCreateMouseEvent(
        None, Quartz.kCGEventLeftMouseUp, Quartz.CGPointMake(x1, y1 + Y_OFFSET), 0)
    Quartz.CGEventPost(Quartz.kCGHIDEventTap, ev)
    time.sleep(0.3)


def press(x, y):
    move(x, y)
    ev = Quartz.CGEventCreateMouseEvent(
        None, Quartz.kCGEventLeftMouseDown, Quartz.CGPointMake(x, y + Y_OFFSET), 0)
    Quartz.CGEventPost(Quartz.kCGHIDEventTap, ev)
    time.sleep(0.2)


def dragto(x, y):
    ev = Quartz.CGEventCreateMouseEvent(
        None, Quartz.kCGEventLeftMouseDragged, Quartz.CGPointMake(x, y + Y_OFFSET), 0)
    Quartz.CGEventPost(Quartz.kCGHIDEventTap, ev)
    time.sleep(0.2)


def release(x, y):
    ev = Quartz.CGEventCreateMouseEvent(
        None, Quartz.kCGEventLeftMouseUp, Quartz.CGPointMake(x, y + Y_OFFSET), 0)
    Quartz.CGEventPost(Quartz.kCGHIDEventTap, ev)
    time.sleep(0.3)


def key(name, mod=None):
    code = VK[name]
    modcode, flags = MODS[mod] if mod else (None, 0)
    if modcode is not None:
        _post(modcode, True, flags)
    _post(code, True, flags)
    _post(code, False, flags)
    if modcode is not None:
        _post(modcode, False, 0)
    time.sleep(0.2)


def scroll(clicks):
    ev = Quartz.CGEventCreateScrollWheelEvent(None, Quartz.kCGScrollEventUnitLine, 1, int(clicks))
    Quartz.CGEventPost(Quartz.kCGHIDEventTap, ev)
    time.sleep(0.25)


if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "move":
        move(float(sys.argv[2]), float(sys.argv[3]))
    elif cmd == "click":
        click(float(sys.argv[2]), float(sys.argv[3]))
    elif cmd in ("press", "dragto", "release"):
        {"press": press, "dragto": dragto, "release": release}[cmd](
            float(sys.argv[2]), float(sys.argv[3]))
    elif cmd == "drag":
        drag(*[float(v) for v in sys.argv[2:6]])
    elif cmd == "scroll":
        scroll(float(sys.argv[2]))
    elif cmd == "key":
        key(sys.argv[2], sys.argv[3] if len(sys.argv) > 3 else None)
    else:
        raise SystemExit(f"unknown command {cmd}")
