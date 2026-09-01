#!/usr/bin/env python3
"""Renders assets/macos/monk.icns from the geometry of assets/logo.svg.

The logo is a handful of primitives, so it is cheaper to rasterize them
directly than to carry an SVG renderer: signed distances give clean
antialiasing at every icon size, and the result is byte-identical on any
machine. Run via `just icns` after changing the logo.
"""
import math
import struct
import subprocess
import sys
import tempfile
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "assets/macos/monk.icns"

PLATE = (0x0B, 0x0D, 0x12)
STROKE = (0xE8, 0xEC, 0xF1)
CURSOR_FROM = (0x7A, 0xA2, 0xF7)
CURSOR_TO = (0xBB, 0x9A, 0xF7)
HALF_STROKE = 1.25
CORNER = 7.0
# macOS leaves a margin around the plate; the logo's own 32-unit square is
# drawn inside that.
MARGIN = 0.09


def cubic(out, c1, c2, to, steps=24):
    p0 = out[-1]
    for i in range(1, steps + 1):
        t = i / steps
        n = 1 - t
        a, b, c, d = n**3, 3 * n * n * t, 3 * n * t * t, t**3
        out.append(
            (
                a * p0[0] + b * c1[0] + c * c2[0] + d * to[0],
                a * p0[1] + b * c1[1] + c * c2[1] + d * to[1],
            )
        )


def letter_m():
    left = [(7.9, 20.6), (7.9, 14.9)]
    cubic(left, (7.9, 12.7), (9.1, 11.7), (10.75, 11.7))
    cubic(left, (12.4, 11.7), (13.6, 12.7), (13.6, 14.9))
    left.append((13.6, 20.6))
    right = [(13.6, 14.9)]
    cubic(right, (13.6, 12.7), (14.8, 11.7), (16.45, 11.7))
    cubic(right, (18.1, 11.7), (19.3, 12.7), (19.3, 14.9))
    right.append((19.3, 20.6))
    return [left, right]


def segment_distance(a, b, px, py):
    dx, dy = b[0] - a[0], b[1] - a[1]
    len2 = dx * dx + dy * dy
    t = 0.0 if len2 == 0 else max(0.0, min(1.0, ((px - a[0]) * dx + (py - a[1]) * dy) / len2))
    return math.hypot(px - (a[0] + t * dx), py - (a[1] + t * dy))


def polyline_distance(pts, px, py):
    return min(segment_distance(pts[i], pts[i + 1], px, py) for i in range(len(pts) - 1))


def rounded_rect_distance(px, py, half, radius, center=16.0):
    qx = abs(px - center) - (half - radius)
    qy = abs(py - center) - (half - radius)
    return math.hypot(max(qx, 0.0), max(qy, 0.0)) + min(max(qx, qy), 0.0) - radius


def over(dst, src, alpha):
    return tuple(round(s * alpha + d * (1 - alpha)) for s, d in zip(src, dst))


def render(size):
    strokes = letter_m()
    scale = size / 32.0 * (1 - 2 * MARGIN)
    offset = size * MARGIN
    rows = []
    for y in range(size):
        row = bytearray()
        for x in range(size):
            px = ((x + 0.5) - offset) / scale
            py = ((y + 0.5) - offset) / scale
            plate = coverage(rounded_rect_distance(px, py, 16.0, CORNER), scale)
            if plate == 0.0:
                row += bytes((0, 0, 0, 0))
                continue
            color = PLATE
            m = min(polyline_distance(pts, px, py) for pts in strokes) - HALF_STROKE
            color = over(color, STROKE, coverage(m, scale))
            cursor = segment_distance((22.3, 20.6), (25.6, 20.6), px, py) - HALF_STROKE
            if cursor < 1.0:
                t = max(0.0, min(1.0, (px - 22.3) / 3.3))
                tint = tuple(round(a + (b - a) * t) for a, b in zip(CURSOR_FROM, CURSOR_TO))
                color = over(color, tint, coverage(cursor, scale))
            row += bytes((*color, round(plate * 255)))
        rows.append(bytes(row))
    return rows


def coverage(distance, scale):
    """Antialiased inside-ness: one device pixel of falloff across the edge."""
    return max(0.0, min(1.0, 0.5 - distance * scale))


def png(path, rows, size):
    raw = b"".join(b"\x00" + r for r in rows)

    def chunk(tag, data):
        body = tag + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    header = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    path.write_bytes(
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def main():
    with tempfile.TemporaryDirectory() as tmp:
        iconset = Path(tmp) / "monk.iconset"
        iconset.mkdir()
        for base in (16, 32, 128, 256, 512):
            for factor, suffix in ((1, ""), (2, "@2x")):
                size = base * factor
                png(iconset / f"icon_{base}x{base}{suffix}.png", render(size), size)
        OUT.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(["iconutil", "-c", "icns", str(iconset), "-o", str(OUT)], check=True)
    print(f"wrote {OUT.relative_to(ROOT)} ({OUT.stat().st_size} bytes)")


if __name__ == "__main__":
    sys.exit(main())
