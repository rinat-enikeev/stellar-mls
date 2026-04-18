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
import os
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
    ap.add_argument(
        "--minisign-pubkey",
        default=None,
        help="Minisign public key string (RWS...). Read from MINISIGN_PUBKEY env var if flag omitted.",
    )
    args = ap.parse_args()

    # CLI filenames: "ceremony_tool-<tag>-<target>[.exe]". GUI filename:
    # "CeremonyTool-<tag>.dmg". We anchor parsing on the caller-supplied
    # --tag — arches like "aarch64" collide with regex tag heuristics.
    cli_prefix = f"ceremony_tool-{args.tag}-"
    gui_filename = f"CeremonyTool-{args.tag}.dmg"

    assets = []
    for binary in sorted(args.dist_dir.iterdir()):
        if binary.is_dir():
            continue
        # Skip sidecars; we only enumerate primary artifacts.
        if any(binary.name.endswith(sfx) for sfx in (".sha256", ".minisig", ".sig",
                                                     ".pem", ".buildinfo.json",
                                                     ".json")):
            continue
        if binary.name == gui_filename:
            sha256 = sha256_of(binary)
            url = RELEASE_URL_FMT.format(tag=args.tag, name=binary.name)
            assets.append({
                "kind": "gui",
                "target": "macos-universal-gui",
                "filename": binary.name,
                "url": url,
                "sha256": sha256,
                "size": binary.stat().st_size,
                "minisign": False,
                "cosign": False,
                "notarized": True,
            })
            continue
        if not binary.name.startswith(cli_prefix):
            continue
        remainder = binary.name[len(cli_prefix):]
        if remainder.endswith(".exe"):
            target = remainder[: -len(".exe")]
        else:
            target = remainder
        if not target:
            continue
        sha256 = sha256_of(binary)
        url = RELEASE_URL_FMT.format(tag=args.tag, name=binary.name)
        assets.append({
            "kind": "cli",
            "target": target,
            "filename": binary.name,
            "url": url,
            "sha256": sha256,
            "size": binary.stat().st_size,
            "minisign": (args.dist_dir / (binary.name + ".minisig")).exists(),
            "cosign": (args.dist_dir / (binary.name + ".sig")).exists(),
            "notarized": "apple-darwin" in target,
        })

    minisign_pubkey = args.minisign_pubkey or os.environ.get("MINISIGN_PUBKEY") or None
    if minisign_pubkey:
        minisign_pubkey = minisign_pubkey.strip() or None

    manifest = {
        "tag": args.tag,
        "assets": assets,
        "minisign_pubkey": minisign_pubkey,
        "cosign_identity": None,
    }
    args.out.write_text(json.dumps(manifest, indent=2) + "\n")
    print(f"wrote {len(assets)} assets to {args.out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
