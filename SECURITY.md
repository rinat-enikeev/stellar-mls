# Security Policy

Stellar MLS is a privacy-critical cryptographic system. Group membership confidentiality relies on the soundness of the Groth16 circuits, the correctness of the Soroban verifier, and the key handling inside the Swift/Kotlin SDKs. Security issues in any of these components can deanonymize members, forge membership, or break epoch monotonicity. We take reports seriously and ask that they be disclosed privately.

## Supported Versions

Only the latest minor release line receives security updates. Older tags are archived and will not be patched.

| Version | Supported |
| ------- | --------- |
| `main` (HEAD) | :white_check_mark: |
| Latest tagged release | :white_check_mark: |
| Prior releases | :x: |

The on-chain Soroban contract is versioned independently. Deployed contract addresses and their corresponding commit hashes are listed in [`docs/mainnet-deployment.md`](docs/mainnet-deployment.md).

## Reporting a Vulnerability

**Do not open a public GitHub issue for security reports.**

Please report vulnerabilities via one of:

1. **GitHub Private Vulnerability Reporting** — preferred. Use the "Report a vulnerability" button under the Security tab at https://github.com/rinat-enikeev/stellar-mls/security.
2. **Email** — `rinat.enikeev@gmail.com`. For sensitive reports, request a PGP key in your first message and we will reply with one before you send details.

Please include:

- Affected component (`src/`, `contracts/sep-xxxx/`, `swift-mls/`, `kotlin-mls/`, `relayer/`, `deploy/`, or a client app)
- Commit hash or release tag
- Reproduction steps or a proof-of-concept
- Impact assessment (e.g. membership deanonymization, proof forgery, key disclosure, denial of service)
- Whether the issue is already public or known to third parties

### In scope

- Soundness or zero-knowledge breaks in the Groth16 circuits
- Verifier bugs in the Soroban contract (including BLS12-381 host-call misuse)
- Epoch-ordering, replay, or commitment-binding flaws
- Key handling issues in the Swift/Kotlin SDKs and reference apps
- Relayer vulnerabilities that leak fee-payer identity or enable request forgery
- Nostr transport issues that leak plaintext, metadata beyond what [`docs/nip-private-group-transport.md`](docs/nip-private-group-transport.md) documents, or break AES-256-GCM framing
- Build and release supply-chain issues (reproducibility, signing, distributed artifacts)

### Out of scope

- Traffic analysis against public Nostr relays (acknowledged limitation, see README)
- Attacks requiring compromise of a member's device
- Recovery from BLS key compromise without re-keying (documented non-goal)
- Denial of service against self-hosted infrastructure that requires control of the network path
- Findings already listed in [`docs/audit-report.md`](docs/audit-report.md), [`docs/audit-report-v2.md`](docs/audit-report-v2.md), [`docs/audit-report-v3.md`](docs/audit-report-v3.md), or [`docs/audit-4.md`](docs/audit-4.md)

## Response Process

| Stage | Target |
| ----- | ------ |
| Acknowledgement of report | within 72 hours |
| Initial triage and severity assessment | within 7 days |
| Status update cadence during investigation | at least every 14 days |
| Fix, coordinated disclosure window, or decline with rationale | within 90 days of acknowledgement |

If the vulnerability affects a deployed Soroban contract or live relayer, we will also publish a post-mortem under [`docs/`](docs/) after the fix is released, following the pattern of the existing post-mortems (`postmortem-co-membership-leak.md`, `postmortem-secure-member-removal.md`, `postmortem-unbound-new-commitment.md`).

## Disclosure

We prefer coordinated disclosure. Once a fix is released:

- A CVE will be requested for issues affecting deployed infrastructure or published SDKs.
- The reporter will be credited in the release notes and post-mortem unless they request anonymity.
- Proof-of-concept code and detailed technical write-ups may be published after users have had a reasonable window to upgrade (typically 30 days after the patched release).

## Safe Harbor

Good-faith security research against your own instances, test deployments, or the public testnet is welcome. Please do not:

- Access, modify, or destroy data belonging to other users
- Run denial-of-service or resource-exhaustion attacks against the hosted relay at `relay.onym.chat` or the public Nostr relay
- Exploit a vulnerability beyond what is necessary to demonstrate it

Researchers acting within these bounds will not be pursued under applicable computer-misuse laws by the maintainers.
