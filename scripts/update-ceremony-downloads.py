#!/usr/bin/env python3
"""Build the ceremony-downloads.json manifest for the download page.

Reads built artifacts from --dist-dir and emits a JSON file with one entry
per target (binary + sha256 + minisig + buildinfo). Consumed by the static
download page at /download.html via /api/v1/downloads.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

RELEASE_URL_FMT = (
    "https://github.com/rinat-enikeev/stellar-mls/releases/download/{tag}/{name}"
)


def sha256_of(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--tag", required=True)
    ap.add_argument("--dist-dir", required=True, type=Path)
    ap.add_argument("--out", required=True, type=Path)
    args = ap.parse_args()

    # Filenames are always "ceremony_tool-<tag>-<target>[.exe]". We parse by
    # anchoring on the caller-supplied --tag rather than by regex — arches like
    # "aarch64" collide with regex-based tag-suffix heuristics.
    prefix = f"ceremony_tool-{args.tag}-"

    assets = []
    for binary in sorted(args.dist_dir.iterdir()):
        if binary.is_dir():
            continue
        # Skip non-binary files on this first pass; pick up only primary artifacts.
        if any(binary.name.endswith(sfx) for sfx in (".sha256", ".minisig", ".sig",
                                                     ".pem", ".buildinfo.json",
                                                     ".json")):
            continue
        if not binary.name.startswith(prefix):
            continue
        remainder = binary.name[len(prefix):]
        if remainder.endswith(".exe"):
            target = remainder[: -len(".exe")]
        else:
            target = remainder
        if not target:
            continue
        sha256 = sha256_of(binary)
        url = RELEASE_URL_FMT.format(tag=args.tag, name=binary.name)
        assets.append({
            "target": target,
            "filename": binary.name,
            "url": url,
            "sha256": sha256,
            "size": binary.stat().st_size,
            "minisign": (args.dist_dir / (binary.name + ".minisig")).exists(),
            "cosign": (args.dist_dir / (binary.name + ".sig")).exists(),
        })

    manifest = {
        "tag": args.tag,
        "assets": assets,
        "minisign_pubkey": None,
        "cosign_identity": None,
    }
    args.out.write_text(json.dumps(manifest, indent=2) + "\n")
    print(f"wrote {len(assets)} assets to {args.out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
