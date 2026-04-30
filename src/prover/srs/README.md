# Universal SRS — Ethereum Foundation 2023 KZG ceremony

The `ef-kzg-2023.bin` binary in this directory is the n=4096 powers-of-tau
subset of the Ethereum Foundation 2023 KZG ceremony, finalised 2023-11-14
with ≈141k contributors. It's the universal SRS the PLONK prover (under
`feature = "plonk"`) consumes via `src/prover/srs.rs::load_ef_kzg_srs`.

## Format

Layout (little-endian header + arkworks-uncompressed BLS12-381 affine points):

```
[ 0..  4]  magic "EFKZ"
[ 4..  8]  u32 version = 1
[ 8.. 12]  u32 g1_count = 32768
[12.. 16]  u32 g2_count = 65
[16..]     g1_count × 96 B G1Affine, then g2_count × 192 B G2Affine
```

Total: 16 + 32768·96 + 65·192 = 3,158,224 bytes ≈ 3.0 MB.

## Provenance

Source endpoint: `https://seq.ceremony.ethereum.org/info/current_state`.
By 2026 the ceremony is closed; the endpoint name is misleading but the
response is the canonical, finalised `BatchTranscript` JSON (≈253 MB)
containing four PoT sets — n=4096, 8192, 16384, 32768 — plus participant
signatures and identity attestations.

We extract the **n=32768** set (transcript index 3). The n=16384 set
turned out to be just below the threshold jf-plonk's `preprocess`
requires for our depth=11 tier: padded `eval_domain_size = 16384` plus
the hiding-degree-2 blinding overhead means `srs_size = 16384 + ε`,
which fails against an n=16384 SRS with `IndexTooLarge`. n=32768 covers
all three tiers (small/medium/large) with comfortable headroom; future
per-type circuits are expected to come in well below this envelope.

Each of the four PoT sets has its own independently-contributed τ from
the same ~141k ceremony participants — picking transcript index 3
(n=32768) does **not** change the trust model: identical contributors,
identical ceremony chain, just a different τ.

## Reproduction

```bash
# 1. Fetch the published transcript (~253 MB)
./scripts/build/fetch-ef-kzg.sh /tmp/ef-kzg-2023-transcript.json

# 2. Extract the n=4096 PoT subset; rewrites both ef-kzg-2023.bin and
#    expected-hash.in atomically.
cargo run --bin extract-ef-kzg --features extract-tool --release -- \
    /tmp/ef-kzg-2023-transcript.json \
    src/prover/srs/ef-kzg-2023.bin

# 3. Verify integrity
cargo test --features plonk --lib prover::srs::tests
```

## Hash chain

Every layer below is independently verifiable.

- **Upstream JSON transcript SHA-256** — recorded each time the operator
  re-fetches; logged by `scripts/build/fetch-ef-kzg.sh`. Will change if EF
  re-publishes (unlikely for a finalised ceremony, but always worth
  confirming against the values in this README before merging changes).
  Last verified: `8ed1c73857e77ae98ea23e36cdcf828ccbf32b423fddc7480de658f9d116c848` (2026-04-29).
- **Extracted binary SHA-256** — pinned in `expected-hash.in` next to
  this README. `build.rs` enforces it on every compile under
  `feature = "plonk"`.
  Current value (n=32768 extraction):
  `84f3fcee13ffd40437b3628daf6ef40d8fd4a5dfeb0895bb55e6cc5512532295`.
- **Powers-of-tau pairing identity** — checked at test time via
  `e([τ]_1, [1]_2) == e([1]_1, [τ]_2)` (`prover::srs::tests::srs_satisfies_pairing_identity`).

## Bootstrap

When `expected-hash.in` contains the all-zero placeholder
(`[0x00, 0x00, …]`), `build.rs` skips the hash check with a warning. This
lets a fresh checkout build the `extract-ef-kzg` binary in the first
place. Run the extractor; subsequent builds enforce the hash.

## Switching SRS source

The SRS source is a single ~400 KB binary plus its hash. To switch to
e.g. Aztec Ignition (n=2^21, BLS12-381):

1. Replace the input transcript with Aztec's published bytes.
2. Update the URL in `scripts/build/fetch-ef-kzg.sh` to point at
   Aztec's transcript endpoint.
3. Adjust the JSON-key path in `src/bin/extract_ef_kzg.rs` if Aztec's
   transcript schema differs from EF's `BatchTranscript` shape.
4. Re-run the extractor with the new `g1_count` / `g2_count`
   constants in `src/prover/srs.rs` and `src/bin/extract_ef_kzg.rs`.
5. The extractor writes both `ef-kzg-2023.bin` and
   `expected-hash.in`; commit both.

No other code changes required.
