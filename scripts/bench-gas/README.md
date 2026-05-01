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

## Coverage matrix (V1)

| Contract       | deploy | create_* | verify_membership | update_commitment | admin ops |
|----------------|:------:|:--------:|:-----------------:|:-----------------:|:---------:|
| sep-oneonone   | ✓      | ✓        | (V2)              | n/a               | ✓         |
| sep-oligarchy  | ✓      | ✓        | ✓ (revert-mode)   | ✓ (revert-mode)   | ✓         |
| sep-anarchy    | ✓      | (V2)     | (V2)              | (V2)              | ✓         |
| sep-democracy  | ✓      | (V2)     | (V2)              | (V2)              | ✓         |
| sep-tyranny    | ✓      | (V2)     | (V2)              | (V2)              | ✓         |

### Bench mechanics

* **`verify_membership`** returns `Ok(false)` on `InvalidProof`, so
  the captured fee equals the success-path cost — the verifier runs
  the full PLONK pairing check identically in both arms.
* **`update_commitment`** errors out on `InvalidProof`, so the tx
  reverts after the verifier. The captured fee misses the
  post-verify storage writes (history archive + new entry + TTL
  bumps), which PR #206 measured at ~75K CPU / ~75KB mem (≈1% of
  total). The release notes flag this caveat.

## V2 follow-up

The contracts deferred above use `MEMBERSHIP_VK` for create
(anarchy/democracy) or per-tier `VK_CREATE_D{5,8,11}` (tyranny).
None has a fixture-only path that produces a successful
`create_group` from a fresh deploy without runtime proof generation.

V2 plan:
1. Workflow step builds `gen-membership-proof` + `gen-update-proof`
   under `--features gen-proof-tool`.
2. For each tier, generate a fresh `(proof, public_inputs)` bundle
   matching the deploy-time witness defaults (`secret-keys`,
   `prover-index`, `salt`).
3. Drive `create_group` → `verify_membership` → `update_commitment`
   against the fresh proofs, capturing real success-path fees.

Trade-off vs V1: extra ~30 s of CI wall-time per contract for the
proof gen, plus the cost of the proofs hitting BLS arithmetic in
release mode. Tracked in the next-up follow-up issue.

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
