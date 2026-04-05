# Phase 1 Coordinator Playbook

This playbook is for the person coordinating the public Phase 1 contribution sequence.

## Coordinator responsibilities

- Initialize the first state
- Maintain the round order
- Hand the current state to the next participant
- Verify every returned contribution
- Keep a clean archive of each round
- Publish the public contribution trail in the issue
- Freeze the final Phase 1 input before Phase 2

## Start the ceremony

Create the initial state:

```bash
bash tools/ceremony/run.sh \
  init \
  --tier small \
  --out-dir /path/to/round-000 \
  --participant coordinator-init
```

Verify it:

```bash
bash tools/ceremony/run.sh \
  verify-state \
  --state-dir /path/to/round-000
```

## For each participant

1. Send the latest round directory to the next participant
2. Wait for the returned output state directory
3. Verify the transition:

```bash
bash tools/ceremony/run.sh \
  verify-contribution \
  --before-state-dir /path/to/current-round \
  --after-state-dir /path/to/next-round
```

4. Verify the resulting state:

```bash
bash tools/ceremony/run.sh \
  verify-state \
  --state-dir /path/to/next-round
```

5. Confirm the participant has posted their `contribution_id`
6. Archive the new round as the current canonical state

## Recommended archive structure

```text
ceremony/
  round-000/
  round-001/
  round-002/
  ...
```

Do not overwrite previous rounds. Keep every round directory intact.

## Freeze the final Phase 1 input

When contributions are complete, generate a public Phase 2 handoff summary:

```bash
bash tools/ceremony/run.sh \
  phase2-summary \
  --state-dir /path/to/final-round \
  --out-file /path/to/phase2-summary.txt
```

Publish that summary before starting the public Phase 2 contribution process.

## What to publish before Phase 2

- final Phase 1 round number
- final `contribution_id`
- final `phase1_srs_hash`
- archive hash of `state.srs`
- exact trust-model statement for the planned public Phase 2 process

## Next step

Use the published Phase 1 freeze summary together with the Phase 2 participant playbook to start the public Phase 2 contribution sequence.
