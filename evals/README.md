# Skill evaluations (A/B harness)

For each fixture, runs the prompt **twice** — once with the skill content attached as a system prompt (`with-skill`), once without (`without-skill`) — and scores both outputs against fixture-defined keyword scorers. The meaningful signal is the **score gap** per fixture and the **average gap** across the suite:

- **+10% to +50%** — skill is doing real work; specific knowledge is reaching the model.
- **0% ± 5%** — skill is dead weight on this fixture; the base model already knows it.
- **negative** — skill is *regressing* the base model (likely a prompt-format issue or stale content). The runner exits non-zero in this case.

## Layout

```
evals/
├── README.md
├── eval.py                 ← runner (fully self-contained)
├── requirements.txt        ← anthropic
├── .gitignore              ← excludes outputs/
└── skills/
    └── <skill-name>/
        └── fixtures/
            ├── 01_*.py
            └── …
```

Each fixture is a Python module exporting:
- `PROMPT: str` — the user message sent to the API.
- `SCORERS: list[tuple[str, Callable[[str], bool]]]` — labeled predicates run against the assistant's response. Score = `passes / total`.

## Run

```sh
pip install -r evals/requirements.txt
export ANTHROPIC_API_KEY=...

# Run all fixtures for stellar-dev, both modes (default):
python evals/eval.py --skill stellar-dev

# Run a single fixture:
python evals/eval.py --skill stellar-dev --fixture 01_classic_vs_soroban_token

# Only one mode:
python evals/eval.py --skill stellar-dev --mode with-skill
python evals/eval.py --skill stellar-dev --mode without-skill

# Skip referenced .md files (test the entry-point alone):
python evals/eval.py --skill stellar-dev --no-references

# Different model (default: claude-haiku-4-5; cheap+fast):
python evals/eval.py --skill stellar-dev --model claude-opus-4-7

# Save raw responses + summary.json under evals/outputs/<skill>/:
python evals/eval.py --skill stellar-dev --save
```

## What gets sent to the API

**With-skill mode**: the system prompt is the **`SKILL.md` body** (frontmatter stripped) **plus every sibling `*.md` file** in the skill directory (e.g., `contracts-soroban.md`, `api-rpc-horizon.md`). This mirrors what Claude Code surfaces when the skill is active — referenced files are reachable. Pass `--no-references` to inject `SKILL.md` only.

The skill block carries `cache_control: ephemeral`, so subsequent fixtures hit the prompt cache. With 3 fixtures × 2 modes = 6 calls, only the first with-skill call pays full system-prompt cost.

**Without-skill mode**: no system prompt, just the fixture's `PROMPT` as the user message.

Both modes use `temperature=0` for repeatability (not strict determinism — the API can still vary slightly).

## Cost estimate (default haiku-4-5)

For the 3 stellar-dev fixtures × 2 modes:
- ~50KB skill content × 1 (cached for the rest) → ~$0.01 first call, ~$0.001 each cached call.
- ~2KB output per call × 6 → ~$0.005.
- **Total per run: ~$0.05**. With Opus 4.7: ~$0.50.

## Adding a fixture

```python
# evals/skills/<skill>/fixtures/04_my_fixture.py
"""Brief description of what knowledge this fixture targets."""

PROMPT = """Multi-line user prompt that exercises the skill's domain."""

SCORERS = [
    # (label, predicate)
    ("mentions_X",     lambda t: "X" in t.lower()),
    ("mentions_Y",     lambda t: "Y" in t.lower()),
    ("recommends_Z",   lambda t: "Z" in t.lower() and "use" in t.lower()),
]
```

Tips:
- Keep `PROMPT` realistic — the kind of question a real user would ask.
- Aim for 5–10 scorers per fixture. Fewer than 4 makes the gap noisy; more than 12 turns into keyword soup.
- Target knowledge the **base model lacks**: project-specific patterns, recent SDK updates, niche topics. Generic Stellar facts that Claude already knows well will produce small gaps and waste eval budget.
- **Anti-signals are valid**: a scorer can check that a known-wrong term is *absent*. Flip the predicate: `lambda t: "old_api" not in t.lower()`.

## CI

`.github/workflows/skill-evals.yml` runs the harness on every PR that touches `.claude/skills/**` or `evals/**`. Requires the `ANTHROPIC_API_KEY` repo secret. Outputs land as a workflow artifact.

## What this harness is NOT

- **Not a substitute for `cargo test`.** It scores natural-language responses; brittle by nature. Use it as a *trend signal*, not a hard gate. The runner only fails CI when the average gap goes negative (skill regresses the model) — that's the bar worth blocking on.
- **Not a replacement for the Anthropic Console eval tool.** That tool targets prompt + tool-use evals on real conversations. This harness is specifically for "does adding a skill change model behavior on a known fixture set."
- **Not deterministic.** Same fixtures on different days will show small score variation. Run the suite a few times if a single failure looks like noise.
