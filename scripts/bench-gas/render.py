#!/usr/bin/env python3
"""Render the bench JSONL into an ASCII table for the GitHub release.

Input:  one JSON object per line, schema produced by `lib.sh`'s
        `emit_row` (contract, op, tier, fee_stroops, hash, ...).
Output: a single ASCII table; rows sorted by (contract, op, tier),
        XLM = stroops / 10_000_000 with 7 decimal places.
"""
from __future__ import annotations

import argparse
import datetime
import json
import sys
from pathlib import Path

CONTRACT_ORDER = {
    "sep-anarchy": 0,
    "sep-democracy": 1,
    "sep-oligarchy": 2,
    "sep-oneonone": 3,
    "sep-tyranny": 4,
}

OP_ORDER = {
    "deploy": 0,
    "create_group": 1,
    "create_oligarchy_group": 1,
    "verify_membership": 2,
    "update_commitment": 3,
    "set_restricted_mode": 10,
    "bump_group_ttl": 11,
    "get_commitment": 12,
}


def stroops_to_xlm(stroops: int | None) -> str:
    if stroops is None:
        return "—"
    return f"{stroops / 10_000_000:.7f}"


def fmt_stroops(stroops: int | None) -> str:
    if stroops is None:
        return "—"
    return f"{stroops:,}"


def fmt_int(value: int | None) -> str:
    if value is None:
        return "—"
    return f"{int(value):,}"


def tier_str(t: str) -> str:
    if t in ("", "n/a", "none", None):
        return "—"
    return t


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser()
    p.add_argument("--jsonl", required=True, type=Path)
    p.add_argument("--output", required=True, type=Path)
    p.add_argument("--network", default="testnet")
    p.add_argument("--tag", default="(untagged)")
    return p.parse_args()


def load_rows(path: Path) -> list[dict]:
    rows: list[dict] = []
    with path.open() as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                rows.append(json.loads(line))
            except json.JSONDecodeError as exc:
                print(f"warning: skipping unparseable line: {exc}", file=sys.stderr)
    return rows


def sort_key(row: dict) -> tuple:
    return (
        CONTRACT_ORDER.get(row.get("contract", ""), 99),
        OP_ORDER.get(row.get("op", ""), 99),
        row.get("tier") or "",
    )


def build_table(rows: list[dict]) -> str:
    headers = ["Contract", "Operation", "Tier", "Fee (XLM)", "Stroops",
               "Inclusion", "Resource", "Refund"]
    body = []
    for row in sorted(rows, key=sort_key):
        body.append([
            row.get("contract", "?"),
            row.get("op", "?"),
            tier_str(row.get("tier", "")),
            stroops_to_xlm(row.get("fee_stroops")),
            fmt_stroops(row.get("fee_stroops")),
            fmt_int(row.get("inclusion_fee")),
            fmt_int(row.get("resource_fee")),
            fmt_int(row.get("refundable_fee_refund")),
        ])

    widths = [
        max(len(h), max((len(r[i]) for r in body), default=0))
        for i, h in enumerate(headers)
    ]

    def fmt_row(cells: list[str]) -> str:
        return "| " + " | ".join(c.ljust(widths[i]) for i, c in enumerate(cells)) + " |"

    sep = "|-" + "-|-".join("-" * w for w in widths) + "-|"
    lines = [fmt_row(headers), sep]
    lines.extend(fmt_row(r) for r in body)
    return "\n".join(lines)


def main() -> int:
    args = parse_args()
    rows = load_rows(args.jsonl)

    captured_at = datetime.datetime.now(datetime.timezone.utc).strftime(
        "%Y-%m-%dT%H:%M:%SZ"
    )
    header = (
        f"SEP MLS testnet gas benchmarks — {args.tag}\n"
        f"Network: {args.network}    Captured: {captured_at}\n"
        f"Total rows: {len(rows)}\n"
        "\n"
        "Notes:\n"
        "  * Stroops are testnet stroops; 1 XLM = 10,000,000 stroops.\n"
        "  * `verify_membership` rows are captured in revert-mode (well-\n"
        "    formed proof, non-matching PI) where applicable; the verifier\n"
        "    runs the full PLONK pairing check identically in success and\n"
        "    failure paths, so the fee equals the success-path cost.\n"
        "  * `update_commitment` revert-mode rows underestimate the true\n"
        "    cost by ~1% — the post-verify storage writes (history archive\n"
        "    + new entry + TTL bumps) are skipped on revert.\n"
        "  * sep-anarchy / sep-democracy / sep-tyranny verifier ops are\n"
        "    deferred to a follow-up release (require runtime proof gen).\n"
    )

    if not rows:
        body = "(no rows captured)"
    else:
        body = build_table(rows)

    args.output.write_text(header + "\n" + body + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
