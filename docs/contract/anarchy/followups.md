# sep-anarchy: post-merge follow-ups

Tracking list for should-fix items identified during PR #151 review. None block
merge — they are test-suite reinforcement, not correctness gaps. The underlying
code paths exist and are reachable; the gap is that the in-suite mocks can't
drive them. Testnet integration closes the mock-Groth16 limitation.

## Items

1. **Epoch-overflow test.** Exercise the `epoch + 1` saturation/overflow path
   in `update_membership` so the guard is covered, not just present.

2. **IC-arity coverage at medium/large tier.** The `test_vectors_consistency`
   pin covers the small tier; add coverage that the medium and large tiers
   reject malformed IC-count proofs through the same path.

3. **GroupCount-decrement test.** Add a test that the group count decrements
   correctly on the documented removal path (currently only the increment path
   is exercised inline).

4. **`ic_layout` pin.** Pin the IC-layout constants alongside the existing
   error-code / IC-count / tier-capacity pins in `test-vectors.json` so layout
   drift is caught at build time rather than at deploy time.

## Disposition

Ship as a follow-up PR after #151 merges. None are correctness gaps; all four
are reachable via the existing public API once the testnet integration replaces
the mock Groth16 verifier.
