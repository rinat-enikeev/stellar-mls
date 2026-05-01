# Testnet gas benchmarks

Captures real `fee_charged` values (in stroops, rendered as XLM) for
every public entrypoint of the 5 governance contracts on Stellar
testnet. Output is an ASCII table attached to a GitHub release as
`gas-benchmarks-<tag>.txt`, plus a JSONL stream
(`gas-benchmarks-<tag>.jsonl`) for downstream tooling.

## Why this exists

PR #206 ("Phase C.5: gas benchmark suite") landed `bench_*` tests
that read the soroban-sdk budget tracker around each heavy op. Those
numbers are **lower bounds** — the sdk docstring flags that "CPU
instructions are likely to be underestimated when running Rust code
compared to running the WASM equivalent." This bench closes that gap
by submitting real testnet transactions and reading the
post-execution `fee_charged` from `stellar tx fetch fee`.

## Triggering

Manual workflow dispatch only — same convention as `Release`:

```
gh workflow run "Release testnet gas benchmarks" -f tag=vX.Y.Z
```

The tag must already exist (the regular `Release` workflow creates
it). This workflow doesn't push commits, doesn't bump versions, and
doesn't touch the published release notes — it only uploads two
files as release assets.

## Local dry-run

```
bash scripts/bench-gas/run.sh                 # all 5 contracts
bash scripts/bench-gas/run.sh sep-oneonone    # one contract
```

Requirements: `stellar` CLI, `cargo`, `jq`, `xxd`, `python3`.

Outputs land under `scripts/bench-gas/results.{txt,jsonl}`.

## Coverage matrix (V2)

| Contract       | deploy | create_* | verify_membership | update_commitment | admin ops |
|----------------|:------:|:--------:|:-----------------:|:-----------------:|:---------:|
| sep-oneonone   | ✓      | ✓        | (V3)              | n/a               | ✓         |
| sep-oligarchy  | ✓      | ✓        | ✓ (revert-mode)   | (V3)              | ✓         |
| sep-anarchy    | ✓      | ✓ × 3    | ✓ × 3             | ✓ × 3             | ✓         |
| sep-democracy  | ✓      | ✓ × 3    | ✓ × 3             | (V3)              | ✓         |
| sep-tyranny    | ✓      | (V3)     | (V3)              | (V3)              | ✓         |

`× 3` = covered at all three tiers (d=5, d=8, d=11).

### Bench mechanics

* **V1 contracts (`sep-oneonone`, `sep-oligarchy`)** use committed
  fixtures from `contracts/plonk-verifier/tests/fixtures/`. Their
  `create_*` paths use contract-specific create circuits we don't
  have generators for, so V2 doesn't extend them.
* **V2 contracts (`sep-anarchy`, `sep-democracy`)** generate fresh
  PLONK proofs at runtime via `gen-membership-proof` /
  `gen-update-proof`. Both contracts use `MEMBERSHIP_VK` for create
  (so a membership proof IS a create proof), and `sep-anarchy`'s
  update circuit matches `gen-update-proof`'s circuit shape.
* **`verify_membership` revert-mode caveat (oligarchy only)**: the
  verifier returns `Ok(false)` on `InvalidProof` without reverting,
  so the captured fee equals the success-path cost — the verifier
  runs the full PLONK pairing check identically in both arms. V2
  rows for anarchy/democracy use real verifying proofs and capture
  the genuine `Ok(true)` fee.
* **VK shape-only invariant**: for any depth-`d` circuit the baked
  VK depends on circuit topology, not witness values. So a witness
  generated from `(secret_keys, prover_index, salt)` of our choice
  produces a proof that verifies under the on-chain VK regardless
  of what the contract's bake-time witness was.

## V3 follow-up

V2 unlocks the contracts whose verifier circuits match the
existing `gen-membership-proof` / `gen-update-proof` shapes. V3
needs contract-specific gen tools to extend coverage further:

* `gen-democracy-update-proof` — for `sep-democracy.update_commitment`
  (uses `VK_DEMO_UPDATE_D{5,8,11}`; constrains a quorum threshold +
  occupancy commitment that the generic update circuit doesn't).
* `gen-oligarchy-create-proof` / `gen-oligarchy-update-proof` —
  oligarchy already has committed fixtures for these (V1 covers
  the create path), but `update_commitment` needs a fresh proof
  matching post-create state and there's no committed fixture
  that does so.
* `gen-tyranny-create-proof` / `gen-tyranny-update-proof` — both
  the create and update circuits are tyranny-specific (admin-tree
  binding, group_id_fr derivation). No coverage today.
* `gen-oneonone-create-proof` — the 1v1 create circuit is its own
  shape; without it, `verify_membership` can't be benched against
  a known commitment we set via `create_group`.

For the V2 contracts: generation adds ~30s wall-time per tier in
CI (heavier at d=11). With three tiers per contract × two contracts
× two op types (membership + update), the bench job grows from ~4 m
in V1 to ~10–12 m in V2.

## File layout

```
scripts/bench-gas/
├── README.md          (this file)
├── lib.sh             (encoding helpers, deploy/invoke/fee capture)
├── setup.sh           (identity, friendbot fund, contract builds)
├── run.sh             (orchestrator → JSONL → render.py)
├── render.py          (JSONL → ASCII table)
└── contracts/
    ├── sep-anarchy.sh
    ├── sep-democracy.sh
    ├── sep-oligarchy.sh
    ├── sep-oneonone.sh
    └── sep-tyranny.sh
```
