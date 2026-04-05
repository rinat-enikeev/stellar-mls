# Call For Participants: Trusted Setup Ceremony for Production Launch

We are preparing a Groth16 trusted setup ceremony for the production launch of `stellar-mls`.

This ceremony will generate the proving and verification keys used by the protocol. Its purpose is to remove the current single-operator trust assumption and strengthen the production security model. If at least one participant honestly contributes randomness and destroys their secret contribution, the ceremony remains secure.

We are looking for independent participants to contribute entropy and help produce the final public parameters.

## Why participate

- Help secure the production launch of the protocol
- Reduce trust in any single operator
- Contribute to a transparent, auditable setup process
- Be listed publicly as a ceremony participant if you want

## What participation involves

- Running a small ceremony step on your machine or in an ephemeral environment
- Contributing randomness
- Publishing the resulting artifact and transcript step
- Destroying your secret contribution after completion

We will publish detailed instructions, tooling, timelines, and verification steps before the ceremony begins.

## Who should join

We welcome:

- Security researchers
- Cryptography engineers
- Infrastructure operators
- Open source contributors
- Independent community members

No prior Groth16 ceremony experience is required as long as you are comfortable following technical instructions carefully.

## How to register

If you want to participate, comment on this issue with:

```text
I want to participate in the ceremony.
Name/handle:
Timezone:
Preferred contact:
```

If you are interested but have questions first, comment here as well.

## Call to action

If you want to help secure the production setup, register in the comments on this issue to participate in the ceremony.

---

## Suggested follow-up comment

```md
Why this is required

This protocol uses Groth16 proofs. Groth16 requires a setup phase that generates the proving keys used by clients and the verification keys used by the contract.

For development and testing, a single-operator or deterministic setup is acceptable. For production, it is not a strong enough trust model. If one party controls or retains the setup secret material ("toxic waste"), they could generate fake proofs that still verify correctly.

How this affects app security

This does not affect chat encryption or Nostr transport directly.

It affects the integrity of the membership proof system. In practice, the trusted setup protects the guarantee that only real group members can produce valid proofs for on-chain group actions. A proper multi-party ceremony removes the single-party trust assumption: if at least one participant contributes honestly and destroys their secret contribution, no one can later forge proofs from setup material alone.

What to expect as a participant

The expected process will be simple and public:

- You will run a small ceremony tool locally on your machine or in an ephemeral VM
- The tool will take the current ceremony state, add your randomness, and produce an updated output
- You will publish the resulting contribution artifact, transcript data, and hash
- You will post your contribution ID / hash / transcript reference in this issue
- You will destroy your local secret contribution material after completion

In other words: yes, expect to run a small app locally and post a resulting public string or hash in this issue as proof of participation.

We will publish exact instructions, tooling, verification steps, and the required comment format before the ceremony starts.
```
