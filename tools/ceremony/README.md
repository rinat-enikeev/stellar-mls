# Ceremony Tool

Standalone CLI wrapper for local SEP-XXXX Phase 1 ceremony contributions.

The wrapper runs the binary in `--release` mode because contribution verification
does a large number of pairing checks and is noticeably slower in debug builds.

This tool is meant for participants to:

- initialize a new ceremony state
- contribute randomness to the current state
- verify a single contribution
- verify a single state directory
- freeze a final Phase 1 state into a Phase 2 handoff summary
- print a short contribution id suitable for posting in a GitHub issue

## Run

```bash
bash tools/ceremony/run.sh help
```

## Commands

### 1. Initialize a state

```bash
bash tools/ceremony/run.sh \
  init \
  --tier small \
  --out-dir /tmp/ceremony-round-000 \
  --participant coordinator-init
```

This creates:

- `state.txt`
- `state.srs`
- `receipt.txt`

### 2. Make a contribution

```bash
bash tools/ceremony/run.sh \
  contribute \
  --state-dir /tmp/ceremony-round-000 \
  --out-dir /tmp/ceremony-round-001 \
  --participant alice
```

The tool prints a `contribution_id` like:

```text
sepceremony1:small:r1:<sha256>
```

That is the short public string participants can post in the GitHub issue.

### 3. Verify a contribution

```bash
bash tools/ceremony/run.sh \
  verify-contribution \
  --before-state-dir /tmp/ceremony-round-000 \
  --after-state-dir /tmp/ceremony-round-001
```

### 4. Print the receipt again

```bash
bash tools/ceremony/run.sh \
  show-receipt \
  --state-dir /tmp/ceremony-round-001
```

### 5. Verify a state directory

```bash
bash tools/ceremony/run.sh \
  verify-state \
  --state-dir /tmp/ceremony-round-001
```

### 6. Generate a Phase 2 handoff summary

```bash
bash tools/ceremony/run.sh \
  phase2-summary \
  --state-dir /tmp/ceremony-round-001 \
  --out-file /tmp/phase2-summary.txt
```

## Artifact model

Each round is represented by a directory containing the full current SRS plus
the receipt for the contribution that produced it.

- `state.srs` is the current Phase 1 SRS in the existing arkworks export format
- `state.txt` stores the round metadata and SRS hash
- `receipt.txt` stores the participant identity, proof, and contribution id

Anyone can verify a participant's contribution using two adjacent round
directories and the `verify-contribution` command.

The final Phase 1 state can be frozen into a publication-ready Phase 2 handoff summary
using `phase2-summary`.

## Notes

- This tool uses `OsRng` for real randomness.
- It is a practical local Phase 1 contribution tool, not a distributed coordinator.
- The short `contribution_id` is intended for issue comments and public attestation.
- See the playbooks in `docs/` for participant, coordinator, and public Phase 2 participation flows.
