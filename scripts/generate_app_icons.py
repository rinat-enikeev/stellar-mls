#!/usr/bin/env python3
"""Regenerate StellarChat app icons from the shared source PNG.

Reads `clients/assets/app-icon/source.png` (the canonical square artwork)
and produces every derived asset consumed by iOS, Android, and the
marketing website. The macOS ceremony tool has its own generator
(`clients/mac-ceremony/assets/make-iconset.swift`) which sources the
same `source.png`.

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
PLAY_STORE_ICON = (
    ROOT / "clients" / "fastlane" / "metadata" / "android" / "en-US" / "images" / "icon.png"
)

MASTER_SIZE = 1024
ANDROID_LAYER_SIZE = 432
WEBSITE_SIZE = 512
PLAY_STORE_ICON_SIZE = 512

# Android adaptive-icon safe zone is the inner 66dp of 108dp ≈ 61.1%.
# Target 58% so the mark clears the safe-zone boundary with a small margin
# on any launcher mask (circle, squircle, rounded-square, teardrop).
ANDROID_SAFE_FRACTION = 0.58


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


def alpha_from_luminance(source: Image.Image) -> Image.Image:
    """Derive RGBA from a light-background source so near-white is transparent.

    alpha = 255 − L (ITU-R 601-2 luma). This preserves anti-aliased edges
    of the mark — pure-white pixels become fully transparent, pure-black
    pixels become fully opaque, and AA edge pixels retain fractional alpha.
    """
    rgba = source.convert("RGBA")
    luma = rgba.convert("L")
    alpha = luma.point(lambda v: 255 - v)
    r, g, b, _ = rgba.split()
    return Image.merge("RGBA", (r, g, b, alpha))


def force_rgb_black(img: Image.Image) -> Image.Image:
    """Flatten RGB to pure black while preserving the alpha channel."""
    _, _, _, a = img.split()
    zero = a.point(lambda _v: 0)
    return Image.merge("RGBA", (zero, zero, zero, a))


def fit_to_safe_zone(layer: Image.Image, size: int, safe_fraction: float) -> Image.Image:
    """Crop layer to its content bbox, scale the longer side to
    `safe_fraction * size`, and paste centered on a transparent `size × size`
    canvas. Keeps the mark inside Android's adaptive-icon safe zone."""
    bbox = layer.getbbox()
    if bbox is None:
        return Image.new("RGBA", (size, size), (0, 0, 0, 0))
    cropped = layer.crop(bbox)
    cw, ch = cropped.size
    target = max(1, int(round(size * safe_fraction)))
    if cw >= ch:
        new_w = target
        new_h = max(1, int(round(ch * target / cw)))
    else:
        new_h = target
        new_w = max(1, int(round(cw * target / ch)))
    scaled = cropped.resize((new_w, new_h), Image.LANCZOS)
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    offset = ((size - new_w) // 2, (size - new_h) // 2)
    canvas.alpha_composite(scaled, dest=offset)
    return canvas


def make_android_foreground(source: Image.Image) -> Image.Image:
    return fit_to_safe_zone(alpha_from_luminance(source), ANDROID_LAYER_SIZE, ANDROID_SAFE_FRACTION)


def make_android_monochrome(source: Image.Image) -> Image.Image:
    return fit_to_safe_zone(
        force_rgb_black(alpha_from_luminance(source)),
        ANDROID_LAYER_SIZE,
        ANDROID_SAFE_FRACTION,
    )


def make_website_icon(master: Image.Image) -> Image.Image:
    return master.resize((WEBSITE_SIZE, WEBSITE_SIZE), Image.LANCZOS)


def make_play_store_icon(master: Image.Image) -> Image.Image:
    return master.resize((PLAY_STORE_ICON_SIZE, PLAY_STORE_ICON_SIZE), Image.LANCZOS)


def main() -> None:
    source = load_source()

    master = make_master(source)
    foreground = make_android_foreground(source)
    monochrome = make_android_monochrome(source)
    website_icon = make_website_icon(master)
    play_store_icon = make_play_store_icon(master)

    for d in (SHARED_DIR, IOS_DIR, ANDROID_DRAWABLE_DIR, WEBSITE_ICON.parent, PLAY_STORE_ICON.parent):
        d.mkdir(parents=True, exist_ok=True)

    master.save(SHARED_DIR / "master.png", optimize=True)
    master.save(IOS_DIR / "Icon-1024.png", optimize=True)
    foreground.save(SHARED_DIR / "android-foreground.png", optimize=True)
    foreground.save(ANDROID_DRAWABLE_DIR / "ic_launcher_foreground.png", optimize=True)
    monochrome.save(SHARED_DIR / "android-monochrome.png", optimize=True)
    monochrome.save(ANDROID_DRAWABLE_DIR / "ic_launcher_monochrome.png", optimize=True)
    website_icon.save(WEBSITE_ICON, optimize=True)
    play_store_icon.save(PLAY_STORE_ICON, optimize=True)


if __name__ == "__main__":
    main()
