# Phase 2 Participant Playbook

This playbook explains how Phase 1 contributors and other participants will take part in the public Phase 2 ceremony.

## Goal

Phase 2 is the circuit-specific Groth16 ceremony step that derives the final proving key and verification key for this protocol.

The goal of running Phase 2 publicly is to avoid a single-operator trust assumption for the final keys.

## How participants will be involved

Participants will contribute one by one to the public Phase 2 artifact chain.

Each participant will:

1. Receive the current Phase 2 artifact from the coordinator
2. Verify the artifact and the published Phase 1 freeze summary
3. Run the Phase 2 contribution step locally on their own machine
4. Produce an updated Phase 2 artifact
5. Publish their public receipt, hash, or contribution id in the GitHub issue
6. Send the updated artifact back to the coordinator
7. Delete temporary local files after their step is confirmed

This can be done asynchronously. A live call is optional for coordination, not required for cryptographic validity.

## What participants should verify before contributing

Before running their step, participants should confirm:

- the final Phase 1 input has been frozen publicly
- the published `phase1_srs_hash` matches the announced final state
- the published circuit artifact hash matches the intended protocol circuit
- the current Phase 2 artifact they received matches the announced current round

## Expected participant flow

The exact commands depend on the final Phase 2 toolchain, but the operational flow will be:

1. Download the current Phase 2 artifact for your round
2. Run the Phase 2 contribution command locally
3. Save the updated artifact produced by your contribution
4. Record the produced public receipt or artifact hash
5. Post the receipt or hash in the GitHub issue
6. Return the updated artifact to the coordinator

## What will be published publicly

For transparency, the ceremony process should publish:

- the final frozen Phase 1 summary
- the circuit artifact hash used for Phase 2
- each Phase 2 round number
- each participant's public receipt or contribution hash
- the final beacon input
- the final proving key hash
- the final verification key hash
- verification artifacts for the completed ceremony

## What happens after all Phase 2 contributions

After all participant contributions are complete:

1. A final public beacon step is applied
2. The final Phase 2 artifact is verified
3. The final proving key and verification key are exported
4. The resulting hashes and verification artifacts are published
5. The app and contract deployment use only those final public ceremony outputs

## Trust model

Phase 1 contributors are not excluded from Phase 2. They are encouraged to participate again.

The intended production trust model is:

- public multi-party Phase 1
- public multi-party Phase 2
- no single participant or operator should be able to control the final Groth16 setup alone

## Coordinator responsibilities

The coordinator is responsible for:

- freezing and publishing the final Phase 1 input before Phase 2 starts
- publishing the exact circuit artifact hash
- distributing the current Phase 2 artifact to the next participant
- verifying each returned contribution
- archiving all rounds
- publishing the final beacon details and output hashes

## Important note

This playbook describes the intended public Phase 2 participation model.

Phase 2 should not be described as production-grade or non-single-party unless the final circuit-specific key generation step is itself run as a public multi-party process.
