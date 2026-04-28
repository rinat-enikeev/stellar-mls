# StellarChat App Icon

Generated source assets for the mobile app icon live here.

- `source.png`: canonical logo artwork (≥1024×1024, square, black mark on white)
- `master.png`: 1024×1024 launcher icon rendered from `source.png`
- `android-foreground.png`: transparent foreground layer for Android adaptive icons
- `android-monochrome.png`: monochrome foreground layer for Android themed icons

Design brief:

- original icon, no third-party brand asset
- broken-ring mark in near-black on a solid white field
- same artwork family across iOS, Android, macOS ceremony tool, and the website

Regenerate all committed PNGs (including `deploy/website/icon.png`) with:

```bash
./scripts/generate_app_icons.py
```

The generator depends on [Pillow](https://pypi.org/project/Pillow/):

```bash
python3 -m pip install Pillow
```
