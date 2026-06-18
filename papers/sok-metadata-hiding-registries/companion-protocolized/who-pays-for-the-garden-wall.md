---
title: "Who Pays for the Garden Wall"
subtitle: "Incentive models for SNARK-gated community venues"
draft_for: protocolized.io
status: draft
date: 2026-04-28
---

## Who Pays for the Garden Wall

*Incentive models for SNARK-gated community venues*

---

It's a Tuesday in 2028 and the seventy-three members of a small
research collective — let's call it Garden — are watching a slack
in their treasury balance with mild alarm. Garden has been running
for fourteen months on a SNARK-gated registry: each member proves,
in zero knowledge, that they're authorized to speak in a given
channel before the channel's commitment is updated. The protocol
itself has been working. The economics have not.

Garden's founder underwrote the first year out of pocket — a few
hundred dollars a month in chain fees absorbed by a single account
that paid for everything. When a member proposed a new sub-channel,
or admitted a guest, or rotated a credential, the founder's wallet
quietly settled. By month fourteen, the founder is travelling and
the wallet is approaching empty, and Garden is having a conversation
it should have had on day one: who pays for the wall around the
garden, and what does asking that question reveal about the people
inside?

This is the question the cryptographic literature does not
generally answer, because the cryptographic literature is interested
in what the proof binds, not in who pays for the proof to be
checked. But the wall around any protocolized community is partly
made of fees, and the choice of who pays for those fees is a
choice that interacts with privacy, with governance, and with the
shape of the community itself. Garden's founder did not realize this
on day one. Most founders don't.

There are three honest answers to "who pays," and each of them
trades different things against different things.

---

### The first answer: each member pays for themselves

The simplest model is that the member who wants to act pays for
their own action. A member who proposes a new sub-channel funds the
transaction that creates the channel. A member who rotates their
own credential funds the transaction that records the rotation. The
protocol carries no economics of its own; it just gates actions on
proofs and lets the chain handle the money.

This is the model that looks cleanest on a whiteboard and is the
most poisonous in practice for a community that cares about
metadata-hiding. The reason is straightforward and rarely
acknowledged in the technical literature: every transaction has a
fee-payer, and every fee-payer has an account, and every account
has a history. If Alice pays for Alice's own actions, the chain
gains a permanent linked record of Alice's activity in the
community, regardless of what the SNARK proves.

The community thought it was hiding membership. What it was hiding
was the *content* of membership while broadcasting the *fact* of
membership through the fee-payment metadata. For some communities
this is fine. For most of the communities that build SNARK-gated
registries on purpose, it is the exact thing they were trying not
to do.

There are mitigations. A member can pay from a single-use account
funded through a mixer, or use anonymous-credential fee tokens, or
arrange to fund a fresh wallet for each session. Each mitigation is
work, and most members don't do it correctly the first time, and
the ones who don't get cargo-culted by the ones who do until the
collective behavior is uneven and the metadata channel is partial
but real.

Pay-per-action works best when the per-action fee is genuinely
cents-or-below, because cents-or-below per-action makes mitigations
proportionate, and when the community's threat model accepts the
chain-layer linkage that the model produces. Some communities do
accept it: a public DAO whose members are publicly known, a forum
whose membership is openly announced. For those, pay-per-action is
the right model. For Garden, which exists in part because its
members do not want their participation linked to their
real-world identities, it is the wrong model.

---

### The second answer: a relayer takes payment as a service

The model Garden's founder actually started with — without quite
calling it that — is the relayer model. Members produce proofs
locally and send them to a *relayer*, a service that submits the
proof to the chain and pays the fee. The relayer's account is what
shows up on chain; the member's account does not.

This is what the onym deployment that motivates the SoK uses, and
what most production deployments converge on after they encounter
the metadata-leak problem with pay-per-action. The privacy property
is no longer false — the chain doesn't see who submitted — but the
property has not gone away. It has been *moved*. The relayer now
sees what the chain didn't: which group, when, which member, what
shape of action. The relayer is the new metadata observation point.

A relayer can be operated under different incentive structures, and
the structure matters more than founders typically realize on day
one. Three sub-models are worth distinguishing:

**Sponsor.** A single party — usually the founder, or a treasury
they control — runs the relayer at their own expense. This is what
Garden has been doing. It is operationally easy in month one and
unsustainable in month fourteen, for the obvious reason that
unbounded sponsorship is unbounded.

**Public service.** A relayer is operated as a commercial service:
members or the community pay the relayer (off-chain, in a way that
preserves the chain-layer privacy) for a per-action submission fee
or a flat retainer. This is a real business model and it works,
but it introduces a counterparty: the relayer is now a vendor with
its own incentives, its own legal exposure, and its own ability to
deplatform.

**Federated relayers.** Members can submit through any of several
relayers, none of which sees the full picture. The privacy posture
improves, but coordination overhead rises and the per-relayer
economic model has to remain viable on a fraction of the demand.

In all three sub-models, the relayer is the metadata channel the
deployment is choosing to accept. A community that adopts the
relayer model and does not document its surveillance posture — what
the relayer logs, who at the relayer can see what, what subpoena
process the relayer follows — is being dishonest about its threat
model. The technical guarantee was never that no party in the
universe could see what the community is doing. It was that the
chain doesn't see. The relayer does.

---

### The third answer: the community itself pays

The third model is the one Garden is moving toward as its founder's
patience runs out: members pool their contributions into a treasury,
and the treasury — operated by the protocol itself, not by a single
party — pays the relayer when authorized actions need to be
submitted.

This is the DAO-shaped answer, and it is structurally different from
the first two because the treasury is a party in the protocol. The
treasury has its own authorization predicate: maybe a multi-signature
of long-tenured members, maybe a threshold-cryptographic key whose
shares are distributed across the community, maybe an on-chain
governance vote. The treasury holds funds, the funds pay for
operations, and the protocol — not the founder — decides when funds
disburse.

What is interesting about the treasury model, and what is rarely
implemented well in practice, is that the *contributions* to the
treasury are themselves on-chain transactions. If members contribute
from real-world-linked accounts, the dues channel becomes a
metadata channel: the chain sees who paid what when, even if it
doesn't see what they did with their membership. The privacy
guarantee that was preserved at the relayer layer leaks back through
the dues-payment layer.

To preserve privacy properly, the dues mechanism has to be
SNARK-gated *too*. A contribution is a proof of the form: "I am a
member, I have not yet paid this period's dues, and the
contribution I am submitting opens a treasury commitment by exactly
the dues amount." The construction is real and is getting easier
to build with modern SNARK toolchains, but it is not common in
deployed systems. Most existing treasury deployments leak through
the contribution channel and call it a day.

A community that wants both metadata-hiding and a treasury has to
build both pieces, and that is real engineering work. The
engineering work is, however, well-defined: the relations exist,
the SNARK toolchains can prove them, and the missing piece is the
deployment patience to ship two more circuits than the
pay-per-action or relayer-only versions of the system require.

---

### Three models, four axes of constraint

Each of these three models lives differently on each of the
deployment axes that determine what is actually buildable.

The chain you are anchored on matters first. Pay-per-action is
viable on a flat-fee chain where individual transactions cost
fractions of a cent; on a chain whose fee market spikes, it becomes
unaffordable for a member who wants to act during a busy hour. The
relayer model is more chain-portable but inherits the chain's
volatility through the relayer's fee budget. The treasury model
needs a chain whose smart-contract layer can express the treasury
itself, which excludes a few of the more exotic anchor options.

The proving system you choose matters less for the basic models and
more for the privacy-preserving treasury. A pairing-based SNARK
with a per-circuit setup ceremony adds a fresh ceremony for the
contribution circuit; a universal-updatable scheme amortizes the
ceremony over all relations a community might want; a transparent
setup avoids ceremonies entirely. For a community building toward
a privacy-preserving treasury, the setup-ceremony cost is real and
recurring.

The post-quantum stance interacts with the incentive choice only
through the back door. A community whose threat model includes a
fault-tolerant quantum adversary in twenty years probably should
not pick a configuration whose verification is broken under Shor;
but the *incentive* model is not what makes that decision. The
SNARK family does. The incentive choice rides on top.

The verification host — whether the chain has a native pairing
precompile, or runs the verifier in pure bytecode — sets the floor
on per-action fees. Communities anchored on chains with native host
functions can afford pay-per-action at smaller scales than
communities anchored on chains without them. The host axis is
where the cryptographic decisions and the economic decisions
intersect most directly.

---

### What Garden does next

Garden's founder writes a proposal and posts it to the channel.
Three options are on the table. Continue sponsorship, with a
declared end date and a community decision to find before that
date arrives. Move to a paid public relayer, with a per-action
fee, and accept that some members may not be able to participate
during fee spikes. Build a treasury with privacy-preserving dues,
spend three months getting the contribution circuit right, and
then run on community contributions indefinitely.

The collective discusses for a week. Most of the discussion is not
about the cryptography. It is about what kind of community Garden
wants to be: a sponsored project that ends if its sponsor leaves,
a tenant of a paid service whose continuation depends on a vendor's
business decisions, or a self-governing entity that has done the
real work of being self-governing. The cryptographic choices follow
from the political choices, not the other way around.

Garden votes for the third option. The vote itself is a
state-transition the registry has to process, paid for — for the
last time — by the founder. The vote is unanimous. The first
treasury contribution is on chain a month later. Garden's wall is
now built of fees the community itself pools, and the question of
who pays for it has become the question of who is in it, which is
the question Garden was asking from the start.

---

### Reading on

The technical scaffolding for this piece — the six axes of
deployment constraint, the seven concrete configurations, the
migration paths between them — lives in the SoK that this article
companions. Readers wanting benchmark numbers, the full
configuration catalogue, or the migration matrix should follow
that pointer. Readers whose interest is the *shape* of the choice
will find that this article was the technical paper's appendix
once, before its tone was rejected.

The open question, restated: a community that wants to be
metadata-private *and* economically self-sustaining is asking for
two cryptographic constructions to be deployed competently in the
same system. Each construction exists. The two-circuits-deployed
gap is engineering, not research, and the engineering is being
done at the present moment by the small number of teams who have
encountered it firsthand. A future article should report on what
they have learned. This one is happy to have asked the question.
