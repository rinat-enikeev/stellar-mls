#!/usr/bin/env python3
"""Regenerate StellarChat app icons from the shared source PNG.

Reads `clients/assets/app-icon/source.png` (the canonical square artwork)
and produces every derived asset consumed by iOS, Android, and the
marketing website. The macOS ceremony tool has its own generator
(`clients/mac-ceremony/assets/make-iconset.swift`) which also sources
from the same `source.png`.

Requires Pillow: `python3 -m pip install Pillow`.
"""

from __future__ import annotations

from pathlib import Path

from PIL import Image


ROOT = Path(__file__).resolve().parent.parent
SHARED_DIR = ROOT / "clients" / "assets" / "app-icon"
SOURCE = SHARED_DIR / "source.png"
IOS_DIR = ROOT / "clients" / "ios" / "StellarChat" / "StellarChat" / "Assets.xcassets" / "AppIcon.appiconset"
ANDROID_DRAWABLE_DIR = (
    ROOT / "clients" / "android" / "StellarChat" / "app" / "src" / "main" / "res" / "drawable-nodpi"
)
WEBSITE_ICON = ROOT / "deploy" / "website" / "icon.png"

MASTER_SIZE = 1024
ANDROID_LAYER_SIZE = 432
WEBSITE_SIZE = 512
WHITE_THRESHOLD = 240  # RGB channel cutoff for turning background transparent


def load_source() -> Image.Image:
    if not SOURCE.exists():
        raise SystemExit(f"Missing source image at {SOURCE}")
    img = Image.open(SOURCE).convert("RGBA")
    w, h = img.size
    if w != h:
        side = min(w, h)
        left = (w - side) // 2
        top = (h - side) // 2
        img = img.crop((left, top, left + side, top + side))
    return img


def make_master(source: Image.Image) -> Image.Image:
    img = source.resize((MASTER_SIZE, MASTER_SIZE), Image.LANCZOS)
    white = Image.new("RGBA", img.size, (255, 255, 255, 255))
    white.alpha_composite(img)
    return white.convert("RGB")


def strip_white_to_transparent(img: Image.Image) -> Image.Image:
    img = img.convert("RGBA")
    pixels = img.load()
    w, h = img.size
    for y in range(h):
        for x in range(w):
            r, g, b, _ = pixels[x, y]
            if r >= WHITE_THRESHOLD and g >= WHITE_THRESHOLD and b >= WHITE_THRESHOLD:
                pixels[x, y] = (255, 255, 255, 0)
    return img


def make_android_foreground(source: Image.Image) -> Image.Image:
    resized = source.resize((ANDROID_LAYER_SIZE, ANDROID_LAYER_SIZE), Image.LANCZOS)
    return strip_white_to_transparent(resized)


def make_android_monochrome(foreground: Image.Image) -> Image.Image:
    mono = foreground.copy()
    pixels = mono.load()
    w, h = mono.size
    for y in range(h):
        for x in range(w):
            r, g, b, a = pixels[x, y]
            if a == 0:
                continue
            pixels[x, y] = (0, 0, 0, a)
    return mono


def make_website_icon(master: Image.Image) -> Image.Image:
    return master.resize((WEBSITE_SIZE, WEBSITE_SIZE), Image.LANCZOS)


def main() -> None:
    source = load_source()

    master = make_master(source)
    foreground = make_android_foreground(source)
    monochrome = make_android_monochrome(foreground)
    website_icon = make_website_icon(master)

    for d in (SHARED_DIR, IOS_DIR, ANDROID_DRAWABLE_DIR, WEBSITE_ICON.parent):
        d.mkdir(parents=True, exist_ok=True)

    master.save(SHARED_DIR / "master.png", optimize=True)
    master.save(IOS_DIR / "Icon-1024.png", optimize=True)
    foreground.save(SHARED_DIR / "android-foreground.png", optimize=True)
    foreground.save(ANDROID_DRAWABLE_DIR / "ic_launcher_foreground.png", optimize=True)
    monochrome.save(SHARED_DIR / "android-monochrome.png", optimize=True)
    monochrome.save(ANDROID_DRAWABLE_DIR / "ic_launcher_monochrome.png", optimize=True)
    website_icon.save(WEBSITE_ICON, optimize=True)


if __name__ == "__main__":
    main()
