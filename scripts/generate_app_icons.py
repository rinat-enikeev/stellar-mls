#!/usr/bin/env python3

from __future__ import annotations

import math
import struct
import zlib
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
SHARED_DIR = ROOT / "clients" / "assets" / "app-icon"
IOS_DIR = ROOT / "clients" / "ios" / "StellarChat" / "StellarChat" / "Assets.xcassets" / "AppIcon.appiconset"
ANDROID_DRAWABLE_DIR = ROOT / "clients" / "android" / "StellarChat" / "app" / "src" / "main" / "res" / "drawable-nodpi"

MASTER_SIZE = 1024
ANDROID_LAYER_SIZE = 432


def clamp8(value: float) -> int:
    return max(0, min(255, int(round(value))))


def blend_pixel(dst: bytearray, index: int, src: tuple[int, int, int, int]) -> None:
    sr, sg, sb, sa = src
    if sa <= 0:
        return
    dr, dg, db, da = dst[index : index + 4]
    src_a = sa / 255.0
    dst_a = da / 255.0
    out_a = src_a + dst_a * (1.0 - src_a)
    if out_a <= 0:
        dst[index : index + 4] = bytes((0, 0, 0, 0))
        return
    out_r = (sr * src_a + dr * dst_a * (1.0 - src_a)) / out_a
    out_g = (sg * src_a + dg * dst_a * (1.0 - src_a)) / out_a
    out_b = (sb * src_a + db * dst_a * (1.0 - src_a)) / out_a
    dst[index : index + 4] = bytes(
        (clamp8(out_r), clamp8(out_g), clamp8(out_b), clamp8(out_a * 255.0))
    )


class Canvas:
    def __init__(self, width: int, height: int, background: tuple[int, int, int, int] = (0, 0, 0, 0)) -> None:
        self.width = width
        self.height = height
        self.pixels = bytearray(background * (width * height))

    def composite(self, x: int, y: int, color: tuple[int, int, int, int]) -> None:
        if x < 0 or y < 0 or x >= self.width or y >= self.height:
            return
        index = (y * self.width + x) * 4
        blend_pixel(self.pixels, index, color)

    def fill_linear_gradient(self, start: tuple[int, int, int], end: tuple[int, int, int]) -> None:
        denom = max(1, self.width + self.height - 2)
        for y in range(self.height):
            for x in range(self.width):
                t = (x + y) / denom
                color = (
                    clamp8(start[0] + (end[0] - start[0]) * t),
                    clamp8(start[1] + (end[1] - start[1]) * t),
                    clamp8(start[2] + (end[2] - start[2]) * t),
                    255,
                )
                index = (y * self.width + x) * 4
                self.pixels[index : index + 4] = bytes(color)

    def add_radial_glow(
        self,
        center_x: float,
        center_y: float,
        radius: float,
        color: tuple[int, int, int],
        alpha: float,
    ) -> None:
        x0 = max(0, int(center_x - radius))
        x1 = min(self.width, int(center_x + radius) + 1)
        y0 = max(0, int(center_y - radius))
        y1 = min(self.height, int(center_y + radius) + 1)
        for y in range(y0, y1):
            for x in range(x0, x1):
                dx = x + 0.5 - center_x
                dy = y + 0.5 - center_y
                dist = math.hypot(dx, dy)
                if dist >= radius:
                    continue
                falloff = (1.0 - dist / radius) ** 2
                self.composite(
                    x,
                    y,
                    (
                        color[0],
                        color[1],
                        color[2],
                        clamp8(255.0 * alpha * falloff),
                    ),
                )

    def fill_round_rect(
        self,
        x0: float,
        y0: float,
        x1: float,
        y1: float,
        radius: float,
        color: tuple[int, int, int, int],
    ) -> None:
        ix0 = max(0, int(math.floor(x0)))
        ix1 = min(self.width, int(math.ceil(x1)))
        iy0 = max(0, int(math.floor(y0)))
        iy1 = min(self.height, int(math.ceil(y1)))
        left = x0 + radius
        right = x1 - radius
        top = y0 + radius
        bottom = y1 - radius
        for y in range(iy0, iy1):
            py = y + 0.5
            for x in range(ix0, ix1):
                px = x + 0.5
                in_rect = left <= px <= right or top <= py <= bottom
                in_corner = False
                if px < left and py < top:
                    in_corner = math.hypot(px - left, py - top) <= radius
                elif px > right and py < top:
                    in_corner = math.hypot(px - right, py - top) <= radius
                elif px < left and py > bottom:
                    in_corner = math.hypot(px - left, py - bottom) <= radius
                elif px > right and py > bottom:
                    in_corner = math.hypot(px - right, py - bottom) <= radius
                if in_rect or in_corner:
                    self.composite(x, y, color)

    def fill_circle(self, center_x: float, center_y: float, radius: float, color: tuple[int, int, int, int]) -> None:
        x0 = max(0, int(center_x - radius))
        x1 = min(self.width, int(center_x + radius) + 1)
        y0 = max(0, int(center_y - radius))
        y1 = min(self.height, int(center_y + radius) + 1)
        radius_sq = radius * radius
        for y in range(y0, y1):
            py = y + 0.5 - center_y
            for x in range(x0, x1):
                px = x + 0.5 - center_x
                if px * px + py * py <= radius_sq:
                    self.composite(x, y, color)

    def fill_polygon(self, points: list[tuple[float, float]], color: tuple[int, int, int, int]) -> None:
        min_y = max(0, int(math.floor(min(y for _, y in points))))
        max_y = min(self.height - 1, int(math.ceil(max(y for _, y in points))))
        total = len(points)
        for y in range(min_y, max_y + 1):
            py = y + 0.5
            intersections: list[float] = []
            for i in range(total):
                x1, y1 = points[i]
                x2, y2 = points[(i + 1) % total]
                if y1 == y2:
                    continue
                lower_y = min(y1, y2)
                upper_y = max(y1, y2)
                if not (lower_y <= py < upper_y):
                    continue
                t = (py - y1) / (y2 - y1)
                intersections.append(x1 + t * (x2 - x1))
            intersections.sort()
            for i in range(0, len(intersections), 2):
                if i + 1 >= len(intersections):
                    break
                start = max(0, int(math.floor(intersections[i])))
                end = min(self.width - 1, int(math.ceil(intersections[i + 1])))
                for x in range(start, end + 1):
                    self.composite(x, y, color)

    def write_png(self, path: Path) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        raw = bytearray()
        stride = self.width * 4
        for y in range(self.height):
            raw.append(0)
            start = y * stride
            raw.extend(self.pixels[start : start + stride])
        compressed = zlib.compress(bytes(raw), level=9)
        png = bytearray(b"\x89PNG\r\n\x1a\n")
        png.extend(png_chunk(b"IHDR", struct.pack(">IIBBBBB", self.width, self.height, 8, 6, 0, 0, 0)))
        png.extend(png_chunk(b"IDAT", compressed))
        png.extend(png_chunk(b"IEND", b""))
        path.write_bytes(bytes(png))


def png_chunk(kind: bytes, data: bytes) -> bytes:
    return (
        struct.pack(">I", len(data))
        + kind
        + data
        + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)
    )


def scale_points(points: list[tuple[float, float]], factor: float, offset_x: float = 0.0, offset_y: float = 0.0) -> list[tuple[float, float]]:
    return [(x * factor + offset_x, y * factor + offset_y) for x, y in points]


def sparkle_points(center_x: float, center_y: float, outer: float, inner: float) -> list[tuple[float, float]]:
    pts: list[tuple[float, float]] = []
    for i in range(8):
        angle = -math.pi / 2 + (math.pi / 4) * i
        radius = outer if i % 2 == 0 else inner
        pts.append((center_x + math.cos(angle) * radius, center_y + math.sin(angle) * radius))
    return pts


def draw_symbol(canvas: Canvas, scale: float, colored: bool, shadow: bool) -> None:
    bubble_color = (246, 251, 255, 255) if colored else (255, 255, 255, 255)
    sparkle_color = (125, 229, 255, 255) if colored else (255, 255, 255, 255)
    shadow_color = (5, 14, 28, 70)

    x0 = 220 * scale
    y0 = 330 * scale
    x1 = 780 * scale
    y1 = 700 * scale
    radius = 138 * scale
    tail = scale_points(
        [
            (365, 671),
            (308, 815),
            (480, 717),
        ],
        scale,
    )
    star = sparkle_points(718 * scale, 306 * scale, 116 * scale, 42 * scale)
    small_star = sparkle_points(315 * scale, 250 * scale, 38 * scale, 14 * scale)

    if shadow:
        shadow_offset = 18 * scale
        canvas.fill_round_rect(x0 + shadow_offset, y0 + shadow_offset, x1 + shadow_offset, y1 + shadow_offset, radius, shadow_color)
        canvas.fill_polygon(
            [(x + shadow_offset, y + shadow_offset) for x, y in tail],
            shadow_color,
        )
        canvas.fill_polygon(
            [(x + shadow_offset * 0.55, y + shadow_offset * 0.55) for x, y in star],
            (5, 14, 28, 50),
        )

    canvas.fill_round_rect(x0, y0, x1, y1, radius, bubble_color)
    canvas.fill_polygon(tail, bubble_color)

    # Message dots keep the bubble legible at smaller launcher sizes.
    dot_y = 508 * scale
    for dot_x in (386 * scale, 512 * scale, 638 * scale):
        dot_color = (22, 63, 106, 125) if colored else (255, 255, 255, 255)
        canvas.fill_circle(dot_x, dot_y, 24 * scale, dot_color)

    canvas.fill_polygon(star, sparkle_color)
    canvas.fill_polygon(small_star, (208, 244, 255, 220) if colored else (255, 255, 255, 255))


def make_master_icon() -> Canvas:
    canvas = Canvas(MASTER_SIZE, MASTER_SIZE)
    canvas.fill_linear_gradient((6, 18, 34), (24, 76, 125))
    canvas.add_radial_glow(212, 194, 360, (56, 139, 214), 0.28)
    canvas.add_radial_glow(842, 880, 410, (19, 120, 255), 0.18)
    canvas.add_radial_glow(768, 192, 180, (110, 236, 255), 0.14)
    for x, y, radius, alpha in (
        (194, 138, 5, 0.55),
        (290, 232, 3, 0.35),
        (802, 174, 4, 0.45),
        (846, 272, 3, 0.28),
        (724, 806, 4, 0.26),
    ):
        canvas.fill_circle(x, y, radius, (238, 247, 255, clamp8(255 * alpha)))
    draw_symbol(canvas, 1.0, colored=True, shadow=True)
    return canvas


def make_foreground(colored: bool) -> Canvas:
    canvas = Canvas(ANDROID_LAYER_SIZE, ANDROID_LAYER_SIZE)
    scale = ANDROID_LAYER_SIZE / MASTER_SIZE
    draw_symbol(canvas, scale, colored=colored, shadow=False)
    return canvas


def main() -> None:
    SHARED_DIR.mkdir(parents=True, exist_ok=True)
    IOS_DIR.mkdir(parents=True, exist_ok=True)
    ANDROID_DRAWABLE_DIR.mkdir(parents=True, exist_ok=True)

    master = make_master_icon()
    foreground = make_foreground(colored=True)
    monochrome = make_foreground(colored=False)

    master.write_png(SHARED_DIR / "master.png")
    master.write_png(IOS_DIR / "Icon-1024.png")
    foreground.write_png(SHARED_DIR / "android-foreground.png")
    foreground.write_png(ANDROID_DRAWABLE_DIR / "ic_launcher_foreground.png")
    monochrome.write_png(SHARED_DIR / "android-monochrome.png")
    monochrome.write_png(ANDROID_DRAWABLE_DIR / "ic_launcher_monochrome.png")


if __name__ == "__main__":
    main()
