# Phase 1 Participant Runbook

This runbook is for contributors participating in the public Phase 1 Powers of Tau process.

## What you will do

1. Receive the current Phase 1 state directory from the coordinator
2. Verify the state locally
3. Run one local contribution on your own machine
4. Optionally verify your contribution transition locally
5. Post your public `contribution_id` in the GitHub issue
6. Send the resulting output state directory back to the coordinator
7. Delete temporary local files after your step is confirmed

## Prerequisites

- Clone the repository
- Make sure Rust and Cargo are installed
- Make sure you can run local shell scripts
- Prepare a clean local working directory for ceremony files

## Verify the tool is available

```bash
bash tools/ceremony/run.sh help
```

## Verify the received state

```bash
bash tools/ceremony/run.sh \
  verify-state \
  --state-dir /path/to/current-state
```

This checks:

- the state SRS hash matches the metadata
- the SRS is internally consistent
- for round 0, the initial contribution proof is valid

## Make your contribution

```bash
bash tools/ceremony/run.sh \
  contribute \
  --state-dir /path/to/current-state \
  --out-dir /path/to/your-output-state \
  --participant your-name
```

## Verify your transition

```bash
bash tools/ceremony/run.sh \
  verify-contribution \
  --before-state-dir /path/to/current-state \
  --after-state-dir /path/to/your-output-state
```

## What to post publicly

After your contribution completes, the tool will print a line like:

```text
I contributed to the ceremony. participant=your-name, round=1, contribution_id=sepceremony1:small:r1:...
```

Post that line in the GitHub issue.

## What to send to the coordinator

Send the full output state directory:

- `state.srs`
- `state.txt`
- `receipt.txt`

The coordinator will verify it and pass the updated state to the next participant.

## After your step

After the coordinator confirms receipt:

- delete temporary local copies of the input state
- delete temporary local copies of your output state
- close temporary shells or VMs used for the contribution

## Hygiene recommendations

- Use a dedicated temporary directory
- Avoid screen-sharing local file contents unless needed
- Prefer a temporary or clean environment if you want stricter hygiene
- Keep only the public `contribution_id` in the issue; artifact handoff happens separately
