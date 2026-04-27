#!/usr/bin/env python3
"""A/B eval harness for Claude Code skills.

For each fixture, runs the prompt twice — once with the skill content
attached as a system prompt (`with-skill`), once without it
(`without-skill`) — then scores both outputs against fixture-defined
keyword scorers. The meaningful signal is the **score gap**: positive
gap means the skill adds knowledge the model didn't have; near-zero
means the skill is dead weight on that fixture.

Skill content loading: by default loads `SKILL.md` body PLUS every
sibling `*.md` file in the skill directory (LICENSE excluded). This
mirrors what Claude Code makes available when the skill is active —
referenced files are reachable via Read. Use `--no-references` to
test the entry-point alone.

Prompt caching: the skill block is sent with `cache_control:
ephemeral` so the second fixture-run-with-skill hits the cache. With
3 fixtures × 2 modes = 6 calls, only the first with-skill call pays
full system-prompt cost.

Usage:
    pip install anthropic
    export ANTHROPIC_API_KEY=...
    python evals/eval.py --skill stellar-dev
    python evals/eval.py --skill stellar-dev --fixture 01_classic_vs_soroban
    python evals/eval.py --skill stellar-dev --mode with-skill
    python evals/eval.py --skill stellar-dev --model claude-opus-4-7
    python evals/eval.py --skill stellar-dev --save  # write outputs/ + summary.json
"""
from __future__ import annotations

import argparse
import importlib.util
import json
import os
import sys
from dataclasses import dataclass, field
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
SKILLS_ROOT = REPO_ROOT / ".claude" / "skills"
EVALS_ROOT = REPO_ROOT / "evals"
DEFAULT_MODEL = os.environ.get("EVAL_MODEL", "claude-haiku-4-5")


@dataclass
class FixtureResult:
    fixture: str
    mode: str  # "with-skill" or "without-skill"
    response: str
    scores: list[tuple[str, bool]] = field(default_factory=list)

    @property
    def passes(self) -> int:
        return sum(1 for _, ok in self.scores if ok)

    @property
    def total(self) -> int:
        return len(self.scores)

    @property
    def ratio(self) -> float:
        return self.passes / self.total if self.total else 0.0


def load_fixture(path: Path):
    spec = importlib.util.spec_from_file_location(path.stem, path)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader
    spec.loader.exec_module(module)
    if not hasattr(module, "PROMPT") or not hasattr(module, "SCORERS"):
        raise ValueError(f"{path}: fixture must define PROMPT and SCORERS")
    return module


def strip_frontmatter(text: str) -> str:
    if text.startswith("---\n"):
        end = text.find("\n---\n", 4)
        if end > 0:
            return text[end + 5 :].strip()
    return text.strip()


def load_skill_content(skill_name: str, include_references: bool = True) -> str:
    """Load SKILL.md body + (optionally) sibling reference .md files.

    Mirrors what Claude Code surfaces when the skill is active: the entry
    point is the SKILL.md body; deeper context comes from sibling files
    that the skill references.
    """
    skill_dir = SKILLS_ROOT / skill_name
    if not skill_dir.exists():
        raise FileNotFoundError(f"skill dir not found: {skill_dir}")

    skill_md = skill_dir / "SKILL.md"
    if not skill_md.exists():
        raise FileNotFoundError(f"SKILL.md not found: {skill_md}")

    body = strip_frontmatter(skill_md.read_text())
    parts = [f"# Skill: {skill_name}\n\n{body}"]

    if include_references:
        for ref in sorted(skill_dir.glob("*.md")):
            if ref.name == "SKILL.md":
                continue
            parts.append(f"\n\n# Reference: {ref.name}\n\n{ref.read_text()}")

    return "\n".join(parts)


def run_fixture(
    client, prompt: str, skill_content: str | None, model: str
) -> str:
    """Send `prompt` through the API; return the assistant's text reply.

    `skill_content=None` runs without-skill (no system prompt).
    `skill_content=<str>` runs with-skill; the block carries
    `cache_control: ephemeral` so subsequent with-skill fixtures hit cache.
    """
    kwargs = {
        "model": model,
        "max_tokens": 2048,
        "temperature": 0,
        "messages": [{"role": "user", "content": prompt}],
    }
    if skill_content:
        kwargs["system"] = [
            {
                "type": "text",
                "text": skill_content,
                "cache_control": {"type": "ephemeral"},
            }
        ]
    response = client.messages.create(**kwargs)
    # Concatenate any text blocks (single-block in normal flow)
    out = []
    for block in response.content:
        if hasattr(block, "text"):
            out.append(block.text)
    return "\n".join(out)


def score_response(fixture, response: str) -> list[tuple[str, bool]]:
    """Run fixture's SCORERS against `response`. Returns [(label, pass), ...]."""
    return [(label, pred(response)) for label, pred in fixture.SCORERS]


def main() -> int:
    parser = argparse.ArgumentParser(description="A/B eval harness for Claude Code skills.")
    parser.add_argument("--skill", required=True, help="Skill name under .claude/skills/")
    parser.add_argument("--fixture", help="Run a single fixture by file stem")
    parser.add_argument(
        "--mode",
        choices=["with-skill", "without-skill", "both"],
        default="both",
        help="Which mode(s) to run (default: both)",
    )
    parser.add_argument("--model", default=DEFAULT_MODEL)
    parser.add_argument(
        "--no-references",
        action="store_true",
        help="Inject SKILL.md body only; skip sibling reference .md files",
    )
    parser.add_argument(
        "--save",
        action="store_true",
        help="Write raw responses to evals/outputs/<skill>/<mode>/ + summary.json",
    )
    args = parser.parse_args()

    fixtures_dir = EVALS_ROOT / "skills" / args.skill / "fixtures"
    if not fixtures_dir.exists():
        print(f"No fixtures dir at {fixtures_dir}", file=sys.stderr)
        return 1

    fixture_paths = sorted(fixtures_dir.glob("*.py"))
    fixture_paths = [p for p in fixture_paths if not p.name.startswith("_")]
    if args.fixture:
        fixture_paths = [p for p in fixture_paths if p.stem == args.fixture]
    if not fixture_paths:
        print("No fixtures matched.", file=sys.stderr)
        return 1

    skill_content = load_skill_content(args.skill, include_references=not args.no_references)

    try:
        from anthropic import Anthropic
    except ImportError:
        print("Missing dependency. Run: pip install -r evals/requirements.txt", file=sys.stderr)
        return 1
    client = Anthropic()

    print(f"\n=== Evaluating skill: {args.skill} ===")
    print(f"Model:       {args.model}")
    print(f"Fixtures:    {len(fixture_paths)}")
    print(f"Skill bytes: {len(skill_content):,} (refs: {'on' if not args.no_references else 'off'})")
    print(f"Mode:        {args.mode}\n")

    modes = ["with-skill", "without-skill"] if args.mode == "both" else [args.mode]
    results: list[FixtureResult] = []

    for fixture_path in fixture_paths:
        fixture = load_fixture(fixture_path)
        name = fixture_path.stem
        prompt_preview = fixture.PROMPT[:120].replace("\n", " ")
        print(f"--- {name}")
        print(f"    prompt: {prompt_preview}{'…' if len(fixture.PROMPT) > 120 else ''}")

        for mode in modes:
            content = skill_content if mode == "with-skill" else None
            try:
                response = run_fixture(client, fixture.PROMPT, content, args.model)
            except Exception as e:
                print(f"    [{mode:>14s}] ERROR: {e}", file=sys.stderr)
                continue

            scores = score_response(fixture, response)
            r = FixtureResult(fixture=name, mode=mode, response=response, scores=scores)
            results.append(r)

            failed = [label for label, ok in scores if not ok]
            print(f"    [{mode:>14s}] {r.passes}/{r.total} ({r.ratio:.0%})", end="")
            if failed:
                print(f"  miss: {', '.join(failed)}")
            else:
                print()

            if args.save:
                out_dir = EVALS_ROOT / "outputs" / args.skill / mode
                out_dir.mkdir(parents=True, exist_ok=True)
                (out_dir / f"{name}.txt").write_text(response)

        print()

    # ---- Summary: with-vs-without gap per fixture, and average ----
    if args.mode == "both" and results:
        by_fixture: dict[str, dict[str, FixtureResult]] = {}
        for r in results:
            by_fixture.setdefault(r.fixture, {})[r.mode] = r

        print("=== Summary ===\n")
        print(f"  {'fixture':<40s}  {'without':>8s}  {'with':>6s}  {'gap':>6s}")
        print(f"  {'-' * 40}  {'-' * 8}  {'-' * 6}  {'-' * 6}")

        gaps = []
        for name, modes_r in by_fixture.items():
            with_s = modes_r["with-skill"].ratio if "with-skill" in modes_r else 0.0
            without_s = modes_r["without-skill"].ratio if "without-skill" in modes_r else 0.0
            gap = with_s - without_s
            gaps.append(gap)
            sign = "+" if gap >= 0 else ""
            print(f"  {name:<40s}  {without_s:>7.0%}   {with_s:>5.0%}   {sign}{gap:>5.0%}")

        avg_gap = sum(gaps) / len(gaps) if gaps else 0.0
        sign = "+" if avg_gap >= 0 else ""
        print(f"\n  Average gap: {sign}{avg_gap:.0%}")

        if args.save:
            summary = {
                "skill": args.skill,
                "model": args.model,
                "include_references": not args.no_references,
                "fixtures": {
                    name: {
                        "with-skill": modes_r["with-skill"].ratio if "with-skill" in modes_r else None,
                        "without-skill": modes_r["without-skill"].ratio if "without-skill" in modes_r else None,
                        "gap": (
                            (modes_r["with-skill"].ratio if "with-skill" in modes_r else 0.0)
                            - (modes_r["without-skill"].ratio if "without-skill" in modes_r else 0.0)
                        ),
                        "scores": {
                            mode: [(label, ok) for label, ok in r.scores]
                            for mode, r in modes_r.items()
                        },
                    }
                    for name, modes_r in by_fixture.items()
                },
                "average_gap": avg_gap,
            }
            (EVALS_ROOT / "outputs" / args.skill).mkdir(parents=True, exist_ok=True)
            (EVALS_ROOT / "outputs" / args.skill / "summary.json").write_text(
                json.dumps(summary, indent=2)
            )
            print(f"\n  Wrote evals/outputs/{args.skill}/summary.json")

        # Non-zero exit if average gap is negative (skill regressed model)
        return 0 if avg_gap >= 0 else 2

    return 0


if __name__ == "__main__":
    sys.exit(main())
