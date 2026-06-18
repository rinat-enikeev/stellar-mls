"""Audit sep-democracy contract source against stellar-dev best practices.

Unlike the keyword fixtures, this one isn't testing whether the model
*knows* a fact in isolation — it's testing whether the skill helps the
model produce a more thorough, category-organized audit when handed a
real ~1000-line Soroban contract. The without-skill baseline is what
the model produces from generic Soroban knowledge alone; the gap is
the marginal value of `security.md` + `common-pitfalls.md` +
`contracts-soroban.md` showing up in its context.

Save raw output with `--save`; the .txt file in
evals/outputs/stellar-dev/with-skill/ is the actual deliverable. The
SCORERS are coarse heuristics that flag whether the audit at least
*touches* the categories we care about.
"""
import re
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[4]
SOURCE = (REPO_ROOT / "contracts" / "sep-democracy" / "src" / "lib.rs").read_text()

# Audits run long; bump from default 2048.
MAX_TOKENS = 8192

PROMPT = f"""You are auditing the Soroban contract below against Stellar best
practices. Use the security, common-pitfalls, contracts-soroban, and testing
references available to you.

Produce a structured report with these headings (in this order). Under each
heading list concrete findings with file-relative line numbers; if a heading
has no issues, write "No issues found" — do not skip the heading.

1. Authorization & access control (require_auth, admin gating, restricted mode)
2. Storage tier choice (persistent / instance / temporary) and TTL handling
3. Proof / replay protection (nullifier scoping, used-proof keys)
4. Cryptographic input validation (canonical Fr, BLS12-381 small-subgroup
   point checks, IC arity)
5. Bounds & enumeration (tier limits, group caps, epoch overflow)
6. Error code coverage (unreachable variants, missing guards)

End with a "Top 3 highest-impact fixes" section ranked by severity.

Contract source ({len(SOURCE):,} bytes, contracts/sep-democracy/src/lib.rs):

```rust
{SOURCE}
```
"""


def _section_present(text: str, keywords: list[str]) -> bool:
    """A heading is 'covered' if the audit mentions any of these terms."""
    lower = text.lower()
    return any(k in lower for k in keywords)


SCORERS = [
    ("covers_authorization", lambda t: _section_present(t, [
        "require_auth", "authorization", "admin gat", "restricted mode",
    ])),
    ("covers_storage_tiers", lambda t: _section_present(t, [
        "persistent", "instance storage", "temporary storage",
    ]) and "ttl" in t.lower()),
    ("covers_replay_protection", lambda t: _section_present(t, [
        "replay", "nullifier", "usedproof", "used_proof", "proof_hash",
    ])),
    ("covers_input_validation", lambda t: _section_present(t, [
        "canonical", "subgroup", "is_canonical_fr", "validate_proof_points",
        "validate_vk_points",
    ])),
    ("covers_bounds_overflow", lambda t: _section_present(t, [
        "checked_add", "overflow", "tier limit", "group cap",
        "tier_group_limit", "max_groups",
    ])),
    ("covers_error_coverage", lambda t: _section_present(t, [
        "error code", "error variant", "unreachable", "missing guard",
        "reserved",
    ])),
    # At least 3 line-number citations of the form "line 326", "L326", or ":326"
    ("cites_line_numbers", lambda t: len(re.findall(
        r"\b(?:line|L|:)\s*\d{2,4}\b", t, re.IGNORECASE,
    )) >= 3),
    ("ranks_top_fixes", lambda t: "top 3" in t.lower() or "highest-impact" in t.lower()),
]
