# Phase 1 Ceremony Start

We are starting the Phase 1 ceremony contribution process now.

This phase collects public Powers of Tau contributions and participant attestations. It is the first part of the trusted setup flow for production. Participants can now run a local contribution and post their contribution receipt in this issue.

## How to participate

1. Clone the repository
2. Get the current ceremony state directory from the coordinator
3. Run your contribution locally
4. Post your `contribution_id` in this issue
5. Send the resulting output state directory back to the coordinator
6. Delete your temporary local artifacts after your step is complete

## Commands

```bash
# show usage
bash tools/ceremony/run.sh help

# create your contribution
bash tools/ceremony/run.sh \
  contribute \
  --state-dir /path/to/current-state \
  --out-dir /path/to/your-output-state \
  --participant your-name
```

After it completes, the tool will print a line like:

```text
I contributed to the ceremony. participant=your-name, round=1, contribution_id=sepceremony1:small:r1:...
```

Please post that line in this issue.

## Optional verification

```bash
bash tools/ceremony/run.sh \
  verify-contribution \
  --before-state-dir /path/to/current-state \
  --after-state-dir /path/to/your-output-state
```

## Important note

This starts the Phase 1 contribution flow. It should be understood as the public contribution and transcript phase, not the complete final production key ceremony by itself. We will publish the full Phase 1 transcript and the next steps for the Phase 2 production key process after contributions are complete.

If you want to participate, reply in this issue and we will coordinate the current round order.
