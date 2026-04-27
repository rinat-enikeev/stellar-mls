#!/usr/bin/env python3
"""Render evals/outputs/<skill>/ into a timestamped human-readable
Markdown report under evals/reports/.

Outputs are gitignored (raw API responses); reports are tracked. Run
this after `eval.py --save` to capture the run as a reviewable
artifact alongside the rest of the codebase.

Usage:
    python evals/report.py --skill stellar-dev
    python evals/report.py --skill stellar-dev --tag opus-baseline
"""
from __future__ import annotations

import argparse
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
EVALS_ROOT = REPO_ROOT / "evals"


def render_report(skill: str, tag: str | None) -> Path:
    out_dir = EVALS_ROOT / "outputs" / skill
    summary_path = out_dir / "summary.json"
    if not summary_path.exists():
        sys.exit(
            f"No summary at {summary_path}. Run "
            f"`python evals/eval.py --skill {skill} --save` first."
        )

    summary = json.loads(summary_path.read_text())
    now = datetime.now(timezone.utc)
    stamp_human = now.strftime("%Y-%m-%d %H:%M:%S UTC")
    stamp_file = now.strftime("%Y-%m-%d_%H-%M-%S")

    fixtures = summary["fixtures"]
    avg_gap = summary["average_gap"]
    avg_sign = "+" if avg_gap >= 0 else ""

    lines: list[str] = []
    lines.append(f"# Skill eval report: `{skill}`")
    lines.append("")
    lines.append(f"- **Generated:** {stamp_human}")
    lines.append(f"- **Model:** `{summary['model']}`")
    lines.append(f"- **References included:** {summary['include_references']}")
    lines.append(f"- **Fixtures:** {len(fixtures)}")
    lines.append(f"- **Average gap:** {avg_sign}{avg_gap:.0%}")
    if tag:
        lines.append(f"- **Tag:** `{tag}`")
    lines.append("")

    # ---- Summary table ----
    lines.append("## Summary")
    lines.append("")
    lines.append("| Fixture | Without | With | Gap |")
    lines.append("|---|---:|---:|---:|")
    for name, data in fixtures.items():
        without = data.get("without-skill")
        with_s = data.get("with-skill")
        gap = data.get("gap", 0.0)
        sign = "+" if gap >= 0 else ""
        without_str = f"{without:.0%}" if without is not None else "—"
        with_str = f"{with_s:.0%}" if with_s is not None else "—"
        lines.append(
            f"| `{name}` | {without_str} | {with_str} | {sign}{gap:.0%} |"
        )
    lines.append("")

    # ---- Interpretation guide (one-liner) ----
    lines.append(
        "> Gap = `with-skill − without-skill`. **+10% to +50%** = skill is "
        "doing real work. **0% ± 5%** = base model already knew it, or scorers "
        "are too coarse to distinguish. **Negative** = skill regressed the model."
    )
    lines.append("")

    # ---- Per-fixture detail ----
    lines.append("## Fixtures")
    lines.append("")
    for name, data in fixtures.items():
        gap = data.get("gap", 0.0)
        sign = "+" if gap >= 0 else ""
        lines.append(f"### `{name}` (gap: {sign}{gap:.0%})")
        lines.append("")

        # Per-mode scorer table
        scores = data.get("scores", {})
        labels = [lbl for lbl, _ in scores.get("with-skill", scores.get("without-skill", []))]
        if labels:
            lines.append("| Scorer | Without | With |")
            lines.append("|---|:-:|:-:|")
            without_map = dict(scores.get("without-skill", []))
            with_map = dict(scores.get("with-skill", []))
            for label in labels:
                w = "✓" if without_map.get(label) else "✗"
                ws = "✓" if with_map.get(label) else "✗"
                lines.append(f"| `{label}` | {w} | {ws} |")
            lines.append("")

        # Embed full response bodies as collapsible sections so the report
        # is one self-contained file per run.
        for mode in ("with-skill", "without-skill"):
            txt_path = out_dir / mode / f"{name}.txt"
            if not txt_path.exists():
                continue
            body = txt_path.read_text().rstrip()
            lines.append(f"<details><summary><b>{mode} response</b> ({len(body):,} chars)</summary>")
            lines.append("")
            lines.append(body)
            lines.append("")
            lines.append("</details>")
            lines.append("")

    # ---- Write ----
    reports_dir = EVALS_ROOT / "reports"
    reports_dir.mkdir(parents=True, exist_ok=True)
    name = f"{skill}_{stamp_file}"
    if tag:
        name += f"_{tag}"
    report_path = reports_dir / f"{name}.md"
    report_path.write_text("\n".join(lines))
    return report_path


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Render evals/outputs/<skill>/ into a timestamped Markdown report.",
    )
    parser.add_argument("--skill", required=True, help="Skill name under evals/outputs/")
    parser.add_argument("--tag", help="Optional suffix on the filename (e.g. 'opus-baseline')")
    args = parser.parse_args()

    path = render_report(args.skill, args.tag)
    rel = path.relative_to(REPO_ROOT)
    print(f"Wrote {rel}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
