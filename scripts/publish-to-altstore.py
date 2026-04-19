#!/usr/bin/env python3
"""
Publish an Apple-approved iOS build to AltStore PAL in one run.

Usage:
    scripts/publish-to-altstore.py --version 1.6.6

The notarization submission ID is NOT needed: we look up the
AlternativeDistributionPackageVersion in App Store Connect by versionString,
download manifest + signature + every variant IPA from the ASC API, assemble
the bundle locally, upload it to the droplet at
/opt/onym-chat/deploy/website/adp/<version>/, then update altstore-source.json
so the "Open in AltStore PAL" button on onym.chat offers <version>.

Arguments:
    --version X.Y.Z           (required) iOS shortVersionString.
    --announce-to-altstore    Also POST api.altstore.io/adps so AltStore's
                              federated index learns about the new version
                              (off by default — self-hosted source.json is
                              enough for users tapping "Open in AltStore PAL"
                              on onym.chat).
    --skip-server             Build the bundle locally but do not SCP to the
                              droplet or commit/push altstore-source.json.
    --dry-run                 Query ASC only; do not download variants, do not
                              mutate files, do not SSH/SCP.

Prerequisites:
    clients/apple.env with ASC_KEY_ID, ASC_ISSUER_ID, ASC_PRIVATE_KEY_P8
    .env with DROPLET_IP
    pyjwt installed (python3 -m pip install --user pyjwt)
"""
from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import zipfile
from pathlib import Path

try:
    import jwt  # pyjwt
except ImportError:
    sys.exit("pyjwt is required:  python3 -m pip install --user pyjwt")

REPO_ROOT = Path(__file__).resolve().parents[1]
APPLE_ENV = REPO_ROOT / "clients" / "apple.env"
DOT_ENV = REPO_ROOT / ".env"
SOURCE_JSON = REPO_ROOT / "deploy" / "website" / "altstore-source.json"
BUNDLE_ID = "chat.onym.ios"
ASC_API = "https://api.appstoreconnect.apple.com"
ALTSTORE_API = "https://api.altstore.io"
DROPLET_USER = "root"
DROPLET_ADP_ROOT = "/opt/onym-chat/deploy/website/adp"
ADP_URL_BASE = "https://onym.chat/adp"


def info(msg: str) -> None: print(f"\033[36m==>\033[0m {msg}")
def ok(msg: str)   -> None: print(f"\033[32m==>\033[0m {msg}")
def warn(msg: str) -> None: print(f"\033[33m==>\033[0m {msg}")
def err(msg: str)  -> None: print(f"\033[31m==> ERROR:\033[0m {msg}", file=sys.stderr)


def parse_env_file(path: Path) -> dict[str, str]:
    out: dict[str, str] = {}
    for raw in path.read_text().splitlines():
        line = raw.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        k, _, v = line.partition("=")
        out[k.strip()] = v.strip()
    return out


class ASC:
    def __init__(self, key_id: str, issuer_id: str, p8: str):
        self.key_id = key_id
        self.issuer_id = issuer_id
        if "-----BEGIN" not in p8 and os.path.isfile(p8):
            p8 = Path(p8).read_text()
        # clients/apple.env stores the key on one line with literal \n escapes.
        if "\\n" in p8 and "\n" not in p8.strip().strip("-"):
            p8 = p8.replace("\\n", "\n")
        self.p8 = p8
        self._token: str | None = None
        self._exp = 0.0

    def token(self) -> str:
        now = time.time()
        if self._token and self._exp - now > 60:
            return self._token
        self._exp = now + 1200
        self._token = jwt.encode(
            {"iss": self.issuer_id, "exp": int(self._exp), "aud": "appstoreconnect-v1"},
            self.p8,
            algorithm="ES256",
            headers={"kid": self.key_id, "typ": "JWT"},
        )
        return self._token

    def get(self, path: str, params: dict | None = None) -> dict:
        url = ASC_API + path
        if params:
            url += "?" + urllib.parse.urlencode(params)
        req = urllib.request.Request(url, headers={"Authorization": f"Bearer {self.token()}"})
        try:
            with urllib.request.urlopen(req) as r:
                return json.loads(r.read())
        except urllib.error.HTTPError as e:
            raise RuntimeError(
                f"ASC {e.code} on {url}\n{e.read().decode('utf-8','replace')}"
            ) from None


def pick_adp_version(asc: ASC, version: str) -> tuple[str, str]:
    """Return (adpVersionId, adpZipUrl) for the COMPLETED ADP of this version."""
    apps = asc.get("/v1/apps", {"filter[bundleId]": BUNDLE_ID})
    if not apps["data"]:
        raise RuntimeError(f"No ASC app with bundleId {BUNDLE_ID}")
    app_id = apps["data"][0]["id"]

    asv = asc.get(f"/v1/apps/{app_id}/appStoreVersions",
                  {"filter[versionString]": version, "limit": 10})
    if not asv["data"]:
        raise RuntimeError(f"No AppStoreVersion for versionString={version}")
    asv_id = asv["data"][0]["id"]

    adp = asc.get(f"/v1/appStoreVersions/{asv_id}/alternativeDistributionPackage", {
        "include": "versions",
        "fields[alternativeDistributionPackageVersions]":
            "version,state,url,urlExpirationDate,fileChecksum",
        "limit[versions]": 50,
    })
    completed = [o for o in adp.get("included", [])
                 if o.get("type") == "alternativeDistributionPackageVersions"
                 and o.get("attributes", {}).get("state") == "COMPLETED"]
    if not completed:
        states = [(o["id"], o["attributes"].get("state"))
                  for o in adp.get("included", [])
                  if o.get("type") == "alternativeDistributionPackageVersions"]
        raise RuntimeError(
            f"No COMPLETED AlternativeDistributionPackageVersion for {version}. "
            f"States found: {states}. Has Apple approved notarization?")
    completed.sort(key=lambda o: int(o["attributes"].get("version") or 0), reverse=True)
    best = completed[0]
    return best["id"], best["attributes"]["url"]


def fetch_variants(asc: ASC, adp_version_id: str) -> list[dict]:
    r = asc.get(f"/v1/alternativeDistributionPackageVersions/{adp_version_id}/variants",
                {"fields[alternativeDistributionPackageVariants]":
                 "url,urlExpirationDate,fileChecksum", "limit": 50})
    return r.get("data", [])


def download(url: str, dest: Path) -> int:
    with urllib.request.urlopen(url) as r, open(dest, "wb") as f:
        shutil.copyfileobj(r, f)
    return dest.stat().st_size


def assemble_bundle(asc: ASC, version: str, adp_version_id: str,
                    adp_zip_url: str, workdir: Path) -> tuple[Path, int]:
    """Produce workdir/bundle/{manifest.json, signature, variant/<id>.ipa},
    returning (bundle_dir, total_size_bytes)."""
    bundle = workdir / "bundle"
    bundle.mkdir()
    (bundle / "variant").mkdir()

    # 1. manifest + signature from the ADP zip (~5 KB)
    adp_zip = workdir / "adp.zip"
    info(f"Downloading manifest+signature zip ({download(adp_zip_url, adp_zip)} bytes)")
    with zipfile.ZipFile(adp_zip) as zf:
        for name in ("manifest.json", "signature"):
            if name not in zf.namelist():
                raise RuntimeError(f"ADP zip missing {name}: {zf.namelist()}")
            zf.extract(name, bundle)

    manifest = json.loads((bundle / "manifest.json").read_text())
    if manifest.get("shortVersionString") != version:
        raise RuntimeError(
            f"manifest shortVersionString={manifest.get('shortVersionString')!r} != {version!r}")

    expected_ids = {v["publicId"] for v in manifest.get("variants", [])}
    info(f"Manifest declares {len(expected_ids)} variants: {sorted(expected_ids)}")

    # 2. Download every variant IPA from ASC.
    variants = fetch_variants(asc, adp_version_id)
    if {v["id"] for v in variants} != expected_ids:
        warn(f"ASC returned variant ids {sorted(v['id'] for v in variants)} "
             f"but manifest expects {sorted(expected_ids)}")
    for v in variants:
        vid = v["id"]
        url = v["attributes"]["url"]
        dest = bundle / "variant" / f"{vid}.ipa"
        info(f"Downloading variant {vid}...")
        sz = download(url, dest)
        ok(f"  variant {vid}: {sz:,} bytes")

    # 3. Total size for the source.json entry (sum of all files).
    total = sum(p.stat().st_size for p in bundle.rglob("*") if p.is_file())
    return bundle, total


def update_source_json(version: str, build_version: str, size: int, min_os: str,
                       dry_run: bool) -> bool:
    src = json.loads(SOURCE_JSON.read_text())
    versions = src["apps"][0]["versions"]
    # AltStore's parser rejects the entire source if any notarized (PAL)
    # entry is missing buildVersion, so it is always populated from the
    # manifest's bundleVersion.
    entry = {
        "version": version,
        "buildVersion": build_version,
        "date": time.strftime("%Y-%m-%d"),
        "localizedDescription": f"Release {version}",
        "downloadURL": f"{ADP_URL_BASE}/{version}/manifest.json",
        "size": size,
        "minOSVersion": min_os,
    }
    idx = next((i for i, v in enumerate(versions) if v.get("version") == version), None)
    if idx is None:
        versions.insert(0, entry)
    else:
        versions[idx] = entry
    new = json.dumps(src, indent=2) + "\n"
    if new == SOURCE_JSON.read_text():
        info("altstore-source.json is already up to date")
        return False
    if dry_run:
        warn("--dry-run: not writing altstore-source.json")
        return False
    SOURCE_JSON.write_text(new)
    ok(f"Updated {SOURCE_JSON.relative_to(REPO_ROOT)}")
    return True


def run(cmd: list[str], check: bool = True) -> subprocess.CompletedProcess:
    info("$ " + " ".join(cmd))
    return subprocess.run(cmd, check=check, text=True)


def ssh(host: str, shell_cmd: str, check: bool = True) -> None:
    run(["ssh", "-o", "StrictHostKeyChecking=no", f"{DROPLET_USER}@{host}", shell_cmd], check=check)


def scp_tree(local_dir: Path, host: str, remote_dir: str) -> None:
    ssh(host, f"mkdir -p {remote_dir}")
    run(["scp", "-o", "StrictHostKeyChecking=no", "-rq",
         f"{local_dir}/.", f"{DROPLET_USER}@{host}:{remote_dir}/"])


def announce_to_altstore(adp_version_id: str) -> None:
    info(f"POST {ALTSTORE_API}/adps  adpID={adp_version_id}")
    req = urllib.request.Request(
        f"{ALTSTORE_API}/adps",
        data=json.dumps({"adpID": adp_version_id}).encode(),
        method="POST",
        headers={"Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req) as r:
            ok(f"AltStore POST /adps -> HTTP {r.status}")
    except urllib.error.HTTPError as e:
        warn(f"AltStore POST /adps -> HTTP {e.code}: "
             f"{e.read().decode('utf-8','replace')[:200]}")


def main() -> int:
    ap = argparse.ArgumentParser(description="Publish an Apple-approved iOS build to AltStore PAL.")
    ap.add_argument("--version", required=True, help="iOS shortVersionString, e.g. 1.6.6")
    ap.add_argument("--announce-to-altstore", action="store_true",
                    help="POST api.altstore.io/adps so AltStore's federated index learns this version")
    ap.add_argument("--skip-server", action="store_true",
                    help="Assemble bundle locally; skip droplet upload and git push")
    ap.add_argument("--dry-run", action="store_true",
                    help="Query ASC only; no downloads, no mutations")
    args = ap.parse_args()
    version = args.version

    info(f"=== Publishing Onym Chat {version} to AltStore PAL ===")

    if not args.skip_server and not args.dry_run:
        r = subprocess.run(["git", "-C", str(REPO_ROOT), "status", "--porcelain"],
                           capture_output=True, text=True, check=True)
        if r.stdout.strip():
            err("Working tree is not clean. Commit or stash first:\n" + r.stdout)
            return 2

    apple = parse_env_file(APPLE_ENV)
    for k in ("ASC_KEY_ID", "ASC_ISSUER_ID", "ASC_PRIVATE_KEY_P8"):
        if k not in apple:
            err(f"{APPLE_ENV} is missing {k}")
            return 2

    dotenv = parse_env_file(DOT_ENV) if DOT_ENV.exists() else {}
    droplet_ip = dotenv.get("DROPLET_IP")
    if not args.skip_server and not droplet_ip:
        err(f"{DOT_ENV} has no DROPLET_IP; pass --skip-server to only update local files")
        return 2

    asc = ASC(apple["ASC_KEY_ID"], apple["ASC_ISSUER_ID"], apple["ASC_PRIVATE_KEY_P8"])
    info(f"Looking up AlternativeDistributionPackage for {version} in App Store Connect...")
    adp_version_id, adp_zip_url = pick_adp_version(asc, version)
    ok(f"ADP version id: {adp_version_id}")

    if args.announce_to_altstore and not args.dry_run:
        announce_to_altstore(adp_version_id)

    if args.dry_run:
        ok("Dry run complete. Re-run without --dry-run to actually assemble and publish.")
        return 0

    with tempfile.TemporaryDirectory(prefix=f"adp-{version}-") as td:
        workdir = Path(td)
        bundle, total = assemble_bundle(asc, version, adp_version_id, adp_zip_url, workdir)
        manifest = json.loads((bundle / "manifest.json").read_text())
        min_os = manifest.get("minimumSystemVersions", {}).get("ios", "18.0")
        build_version = str(manifest.get("bundleVersion", ""))
        if not build_version:
            err("manifest.json has no bundleVersion; cannot build a valid PAL source entry")
            return 3
        ok(f"Bundle assembled: {total:,} bytes ({len(list((bundle/'variant').iterdir()))} variants, buildVersion={build_version})")

        if args.skip_server:
            warn(f"--skip-server: bundle remains at {bundle} (temp dir cleaned on exit)")
            # Leave a copy so the user can inspect.
            persisted = REPO_ROOT / f"adp-{version}"
            if persisted.exists():
                shutil.rmtree(persisted)
            shutil.copytree(bundle, persisted)
            ok(f"Copied to {persisted}")
        else:
            info(f"Uploading bundle to {droplet_ip}:{DROPLET_ADP_ROOT}/{version}/...")
            ssh(droplet_ip, f"rm -rf {DROPLET_ADP_ROOT}/{version}")
            scp_tree(bundle, droplet_ip, f"{DROPLET_ADP_ROOT}/{version}")

            # Verify nginx serves the new manifest.
            try:
                with urllib.request.urlopen(f"{ADP_URL_BASE}/{version}/manifest.json") as r:
                    served = json.loads(r.read())
                if served.get("shortVersionString") != version:
                    warn(f"Served manifest shortVersionString = {served.get('shortVersionString')}")
                else:
                    ok(f"nginx serves {ADP_URL_BASE}/{version}/manifest.json")
            except Exception as e:
                warn(f"Could not verify served manifest: {e}")

        changed = update_source_json(version, build_version, total, min_os, False)

    if not args.skip_server and changed:
        info("Committing altstore-source.json...")
        rel = str(SOURCE_JSON.relative_to(REPO_ROOT))
        run(["git", "-C", str(REPO_ROOT), "add", rel])
        msg = f"altstore: publish {version} via AltStore PAL (ADP manifest)"
        run(["git", "-C", str(REPO_ROOT), "commit", "-m", msg])
        run(["git", "-C", str(REPO_ROOT), "pull", "--rebase", "origin", "main"], check=False)
        run(["git", "-C", str(REPO_ROOT), "push", "origin", "main"])
        info("Fast-forwarding droplet clone of main...")
        ssh(droplet_ip, "cd /opt/onym-chat && git fetch origin && git reset --hard origin/main")
        ok("Droplet is now on origin/main; nginx serves the updated catalog")

    ok(f"Done. Tap 'Open in AltStore PAL' on onym.chat to install {version}.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
