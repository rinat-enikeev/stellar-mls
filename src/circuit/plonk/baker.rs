//! VK-baking helpers — produce deterministic verifying-key bytes for
//! the on-chain Soroban verifier.
//!
//! Phase C.2 contracts embed VKs as `static` byte slices:
//!
//! ```rust,ignore
//! pub const VK_BYTES: &[u8] = include_bytes!("membership-d11.vk.bin");
//! ```
//!
//! Those bytes are produced here via `bake_membership_vk(depth)`,
//! which mirrors the canonical-witness preprocessing path also
//! exercised by `test_vectors::verify_plonk_membership_vk_fingerprints`.
//! The cross-platform-anchor SHA-256 constants are the source of truth;
//! `bake_membership_vk` is verified against them in the same test.
//!
//! The CLI wrapper at `src/bin/bake_vk.rs` (under `feature =
//! "bake-vk-tool"`) calls `bake_membership_vk` and writes the result
//! to disk. Contracts then `include_bytes!` that file — no jf-plonk
//! types cross the contract boundary.

#![cfg(feature = "plonk")]

use ark_bls12_381_v05::Fr;
use ark_ff_v05::PrimeField;
use ark_serialize_v05::CanonicalSerialize;
use jf_relation::PlonkCircuit;
use sha2::{Digest, Sha256};

use crate::circuit::plonk::democracy::{
    synthesize_democracy_update, DemocracyUpdateWitness,
};
use crate::circuit::plonk::membership::{synthesize_membership, MembershipWitness};
use crate::circuit::plonk::oligarchy::{
    synthesize_oligarchy_create, synthesize_oligarchy_update_quorum, OligarchyAdminSigner,
    OligarchyCreateWitness, OligarchyUpdateQuorumWitness, OLIGARCHY_ADMIN_DEPTH,
    OLIGARCHY_K_MAX,
};
use crate::circuit::plonk::oneonone_create::{
    synthesize_oneonone_create, OneOnOneCreateWitness, DEPTH as ONEONONE_DEPTH,
};
use crate::circuit::plonk::poseidon::{poseidon_hash_one_v05, poseidon_hash_two_v05};
use crate::circuit::plonk::tyranny::{
    synthesize_tyranny_create, synthesize_tyranny_update, TyrannyCreateWitness,
    TyrannyUpdateWitness,
};
use crate::circuit::plonk::update::{synthesize_update, UpdateWitness};
use crate::prover::plonk;

// ---------------------------------------------------------------------------
// Pinned VK fingerprints — the cross-platform invariant.
//
// If any of these change, either:
// - the circuit shape changed (gate order, public-input order, gadget
//   internals) — review the diff carefully and update;
// - the SRS changed (build.rs would also have caught the hash mismatch
//   first);
// - jf-plonk's `preprocess` output format changed at the byte level —
//   in which case all consumers (Soroban verifier, mobile clients)
//   need a coordinated update too.
//
// Mirror these into `docs/cross-platform-test-vectors.json` under
// `plonk_membership_vk_fingerprints`. Non-Rust platforms compute the
// same SHA-256 over their `VerifyingKey::serialize_uncompressed`
// output and assert byte-equality.
// ---------------------------------------------------------------------------

/// SHA-256 of the canonical small-tier (depth=5) VK, hex-encoded.
pub const VK_SHA256_HEX_SMALL: &str =
    "a552b41c2e40167b74ccbec36d83cc931279d278e364cacb99a3a5ce9c26e5ab";

/// SHA-256 of the canonical medium-tier (depth=8) VK, hex-encoded.
pub const VK_SHA256_HEX_MEDIUM: &str =
    "b4f98a146dad1de3447dde3b686ac2ddf8b4cdb153ad69f407684558e749b3d6";

/// SHA-256 of the canonical large-tier (depth=11) VK, hex-encoded.
pub const VK_SHA256_HEX_LARGE: &str =
    "36e96f9bf3b834f81c73a2d402b33ef8c32bc01fe47cf6ee66978e29ab0d5849";

/// SHA-256 of the canonical small-tier (depth=5) **update** VK,
/// hex-encoded. Anchors the update circuit's preprocessing output the
/// same way `VK_SHA256_HEX_SMALL` anchors the membership circuit's.
pub const UPDATE_VK_SHA256_HEX_SMALL: &str =
    "61dbe507fe10fafa94f060d7f24675091c109b6229242051d887a4b27f9a634a";

/// SHA-256 of the canonical medium-tier (depth=8) **update** VK.
pub const UPDATE_VK_SHA256_HEX_MEDIUM: &str =
    "0e15eb665d2b978e41b5f555cdb765789cb7dc6046cf0236d0d0bdf30a2419ee";

/// SHA-256 of the canonical large-tier (depth=11) **update** VK.
pub const UPDATE_VK_SHA256_HEX_LARGE: &str =
    "73ae78e1d42d161ffdd44f27977f27c084c88d460f926eea5f6ca1d06d851245";

/// Look up the pinned SHA-256 hex digest for a given **membership** tier.
/// Returns `None` for any depth other than 5/8/11.
pub fn pinned_vk_sha256_hex(depth: usize) -> Option<&'static str> {
    match depth {
        5 => Some(VK_SHA256_HEX_SMALL),
        8 => Some(VK_SHA256_HEX_MEDIUM),
        11 => Some(VK_SHA256_HEX_LARGE),
        _ => None,
    }
}

/// Look up the pinned SHA-256 hex digest for a given **update** tier.
/// Returns `None` for any depth other than 5/8/11.
pub fn pinned_update_vk_sha256_hex(depth: usize) -> Option<&'static str> {
    match depth {
        5 => Some(UPDATE_VK_SHA256_HEX_SMALL),
        8 => Some(UPDATE_VK_SHA256_HEX_MEDIUM),
        11 => Some(UPDATE_VK_SHA256_HEX_LARGE),
        _ => None,
    }
}

/// Pinned SHA-256 anchors for tyranny-create per-tier VKs.
pub const TYRANNY_CREATE_VK_SHA256_HEX_SMALL: &str =
    "94591dda611688c17cc7733fb8adeeeea0a2f65d74ae2a901f3fbd56ba16a976";
pub const TYRANNY_CREATE_VK_SHA256_HEX_MEDIUM: &str =
    "e87e5ab7927c5a08175534030d4540eee09fee1a4dae9d8c0afcfc6a4f93e6b0";
pub const TYRANNY_CREATE_VK_SHA256_HEX_LARGE: &str =
    "17c97f3beea08eb686283ac1b9f44eabb6e4f8cf7d40bd6965da023ec76e1ff3";

pub const TYRANNY_UPDATE_VK_SHA256_HEX_SMALL: &str =
    "fac8ade73d201d209a819e39f70b51cce715448c7b9d0bd75492fc8a797e9df2";
pub const TYRANNY_UPDATE_VK_SHA256_HEX_MEDIUM: &str =
    "80cf443718dd94159aeb17d0274f13717beb9d1539fea5be14a97e7986e6f8e8";
pub const TYRANNY_UPDATE_VK_SHA256_HEX_LARGE: &str =
    "04b729f34b67c726cf34ba568970cb6df5357fef923736fb4f6a6dab42edd798";

pub fn pinned_tyranny_create_vk_sha256_hex(depth: usize) -> Option<&'static str> {
    match depth {
        5 => Some(TYRANNY_CREATE_VK_SHA256_HEX_SMALL),
        8 => Some(TYRANNY_CREATE_VK_SHA256_HEX_MEDIUM),
        11 => Some(TYRANNY_CREATE_VK_SHA256_HEX_LARGE),
        _ => None,
    }
}

pub fn pinned_tyranny_update_vk_sha256_hex(depth: usize) -> Option<&'static str> {
    match depth {
        5 => Some(TYRANNY_UPDATE_VK_SHA256_HEX_SMALL),
        8 => Some(TYRANNY_UPDATE_VK_SHA256_HEX_MEDIUM),
        11 => Some(TYRANNY_UPDATE_VK_SHA256_HEX_LARGE),
        _ => None,
    }
}

/// SHA-256 of the canonical **OneOnOne create** VK (depth=5 only —
/// 1v1 has no per-tier dimension). Anchors the create-circuit shape
/// across builds.
pub const ONEONONE_CREATE_VK_SHA256_HEX: &str =
    "1be7b883e1d6a62f204239439bcaf5a2ad437eb6013bf75b43c9eea0a08c207d";

/// Pinned anchors for democracy-update VK shape per tier.
pub const DEMOCRACY_UPDATE_VK_SHA256_HEX_SMALL: &str =
    "9772ebfa43f87d005628fe610d29d11d8dfeeafdc350724a21d41c43becafe70";
pub const DEMOCRACY_UPDATE_VK_SHA256_HEX_MEDIUM: &str =
    "cd3aaaeb26926d89c494fbf03506b8558e53dc055c5bff041b7e10cacc70f3e5";
pub const DEMOCRACY_UPDATE_VK_SHA256_HEX_LARGE: &str =
    "73d120375c3edd0d793bb089bd42a2a9955d0ddb460b0d07cc67246ab579760e";

pub fn pinned_democracy_update_vk_sha256_hex(depth: usize) -> Option<&'static str> {
    match depth {
        5 => Some(DEMOCRACY_UPDATE_VK_SHA256_HEX_SMALL),
        8 => Some(DEMOCRACY_UPDATE_VK_SHA256_HEX_MEDIUM),
        11 => Some(DEMOCRACY_UPDATE_VK_SHA256_HEX_LARGE),
        _ => None,
    }
}

pub fn build_canonical_democracy_update_witness(depth: usize) -> DemocracyUpdateWitness {
    let secret_keys: Vec<Fr> = (1u64..=8).map(Fr::from).collect();
    let prover_index = 3usize;
    let epoch_old: u64 = 1234;
    let salt_old: [u8; 32] = [0xEEu8; 32];
    let salt_new: [u8; 32] = [0xFFu8; 32];
    let occ_old = Fr::from(0xA110u64);
    let occ_new = Fr::from(0xA111u64);
    let threshold = 5u64;

    let leaves: Vec<Fr> = secret_keys.iter().map(poseidon_hash_one_v05).collect();
    let num_leaves = 1usize << depth;
    let mut nodes = vec![Fr::from(0u64); 2 * num_leaves];
    for (i, leaf) in leaves.iter().enumerate() {
        nodes[num_leaves + i] = *leaf;
    }
    for i in (1..num_leaves).rev() {
        nodes[i] = poseidon_hash_two_v05(&nodes[2 * i], &nodes[2 * i + 1]);
    }
    let root = nodes[1];

    let mut path = Vec::with_capacity(depth);
    let mut cur = num_leaves + prover_index;
    for _ in 0..depth {
        let sib = if cur % 2 == 0 { cur + 1 } else { cur - 1 };
        path.push(nodes[sib]);
        cur /= 2;
    }

    let salt_old_fr = Fr::from_le_bytes_mod_order(&salt_old);
    let salt_new_fr = Fr::from_le_bytes_mod_order(&salt_new);
    let inner_old = poseidon_hash_two_v05(&root, &Fr::from(epoch_old));
    let mid_old = poseidon_hash_two_v05(&inner_old, &salt_old_fr);
    let c_old = poseidon_hash_two_v05(&mid_old, &occ_old);
    let inner_new = poseidon_hash_two_v05(&root, &Fr::from(epoch_old + 1));
    let mid_new = poseidon_hash_two_v05(&inner_new, &salt_new_fr);
    let c_new = poseidon_hash_two_v05(&mid_new, &occ_new);

    DemocracyUpdateWitness {
        c_old,
        epoch_old,
        c_new,
        occupancy_commitment_old: occ_old,
        occupancy_commitment_new: occ_new,
        threshold_numerator: threshold,
        secret_key: secret_keys[prover_index],
        member_root_old: root,
        member_root_new: root,
        salt_old,
        salt_new,
        merkle_path_old: path,
        leaf_index_old: prover_index,
        depth,
    }
}

pub fn bake_democracy_update_vk(depth: usize) -> Result<Vec<u8>, BakeError> {
    if pinned_democracy_update_vk_sha256_hex(depth).is_none() {
        return Err(BakeError::UnsupportedDepth(depth));
    }
    let witness = build_canonical_democracy_update_witness(depth);
    let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
    synthesize_democracy_update(&mut circuit, &witness).map_err(BakeError::Synthesize)?;
    circuit
        .finalize_for_arithmetization()
        .map_err(BakeError::Synthesize)?;
    let keys = plonk::preprocess(&circuit).map_err(BakeError::Preprocess)?;
    let mut vk_bytes = Vec::new();
    keys.vk
        .serialize_uncompressed(&mut vk_bytes)
        .map_err(BakeError::Serialize)?;
    Ok(vk_bytes)
}

/// Pinned SHA-256 anchors for oligarchy create + update VK shapes.
///
/// **Single-tier across all 3 oligarchy member tiers.** The admin
/// tree depth is fixed at 5 per design §4.6 (admin tier always
/// Small, regardless of member tier), and the create / update
/// circuits don't open Merkle paths against the member tree — only
/// against the admin tree. So the circuit shape (and resulting VK)
/// is independent of the member tier.
///
/// `OLIGARCHY_UPDATE_VK_SHA256_HEX` anchors the K-of-N quorum +
/// count-delta + admin-tree-membership circuit
/// (`synthesize_oligarchy_update_quorum`, PR #205). The simplified
/// `synthesize_oligarchy_update` is no longer baked — it remains in
/// `oligarchy.rs` as a migration reference.
pub const OLIGARCHY_CREATE_VK_SHA256_HEX: &str =
    "07ca92baabded97d241d11e378203bb2d45c955a1641c02a581e2f2df42a1a25";
pub const OLIGARCHY_UPDATE_VK_SHA256_HEX: &str =
    "f89a3e30c48b6d81e9276dacd7f5b41b497ca954369426d5fa995f5343592cf4";

pub fn build_canonical_oligarchy_create_witness() -> OligarchyCreateWitness {
    let occ = Fr::from(0xA110u64);
    let member_root = Fr::from(0xCAFEu64);
    let admin_root = Fr::from(0xADADu64);
    let salt = Fr::from(0xEEEEu64);
    let inner = poseidon_hash_two_v05(&member_root, &Fr::from(0u64));
    let mid = poseidon_hash_two_v05(&inner, &salt);
    let admin_mix = poseidon_hash_two_v05(&occ, &admin_root);
    let commitment = poseidon_hash_two_v05(&mid, &admin_mix);
    OligarchyCreateWitness {
        commitment,
        occupancy_commitment: occ,
        member_root,
        admin_root,
        salt_initial: salt,
    }
}

/// Canonical witness for the K-of-N quorum oligarchy update circuit.
///
/// **Non-boundary configuration on purpose** (lessons from PR #203's
/// review). Both signers active (`K = K_MAX = 2`), `threshold = 1`
/// → `slack = 1`; `count_new = count_old + 1`. Exercises the slack
/// + threshold range gates and the count-delta product gate
/// non-trivially through the production VK fixture, not just the
/// trivial `K = threshold`, `diff = 0` boundary.
///
/// Single-tier — admin tree depth is fixed at 5 across all oligarchy
/// member tiers, so the circuit (and resulting VK) is identical
/// regardless of member tier.
pub fn build_canonical_oligarchy_update_quorum_witness() -> OligarchyUpdateQuorumWitness {
    let admin_secret_keys: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
    let epoch_old = 1234u64;
    let salt_old = Fr::from(0xEEEEu64);
    let salt_new = Fr::from(0xFFFFu64);
    let salt_oc_old = Fr::from(0x55u64);
    let salt_oc_new = Fr::from(0x66u64);
    let count_old = 5u64;
    let count_new = 6u64;
    let threshold = 1u64;
    let member_root = Fr::from(0xCAFEu64);

    // Build admin tree at fixed OLIGARCHY_ADMIN_DEPTH.
    let admin_leaves: Vec<Fr> = admin_secret_keys.iter().map(poseidon_hash_one_v05).collect();
    let num_admin_leaves = 1usize << OLIGARCHY_ADMIN_DEPTH;
    let mut admin_nodes = vec![Fr::from(0u64); 2 * num_admin_leaves];
    for (i, leaf) in admin_leaves.iter().enumerate() {
        admin_nodes[num_admin_leaves + i] = *leaf;
    }
    for i in (1..num_admin_leaves).rev() {
        admin_nodes[i] =
            poseidon_hash_two_v05(&admin_nodes[2 * i], &admin_nodes[2 * i + 1]);
    }
    let admin_root = admin_nodes[1];

    // Merkle paths for the K_MAX active admin signers.
    let mut paths: Vec<Vec<Fr>> = Vec::with_capacity(OLIGARCHY_K_MAX);
    for prover_index in 0..OLIGARCHY_K_MAX {
        let mut path = Vec::with_capacity(OLIGARCHY_ADMIN_DEPTH);
        let mut cur = num_admin_leaves + prover_index;
        for _ in 0..OLIGARCHY_ADMIN_DEPTH {
            let sib = if cur % 2 == 0 { cur + 1 } else { cur - 1 };
            path.push(admin_nodes[sib]);
            cur /= 2;
        }
        paths.push(path);
    }

    // Occupancy commitments (Poseidon binding for count + salt_oc).
    let occ_old = poseidon_hash_two_v05(&Fr::from(count_old), &salt_oc_old);
    let occ_new = poseidon_hash_two_v05(&Fr::from(count_new), &salt_oc_new);

    // c_old / c_new — same chain as synthesize_oligarchy_create.
    let inner_old = poseidon_hash_two_v05(&member_root, &Fr::from(epoch_old));
    let mid_old = poseidon_hash_two_v05(&inner_old, &salt_old);
    let admin_mix_old = poseidon_hash_two_v05(&occ_old, &admin_root);
    let c_old = poseidon_hash_two_v05(&mid_old, &admin_mix_old);
    let inner_new = poseidon_hash_two_v05(&member_root, &Fr::from(epoch_old + 1));
    let mid_new = poseidon_hash_two_v05(&inner_new, &salt_new);
    let admin_mix_new = poseidon_hash_two_v05(&occ_new, &admin_root);
    let c_new = poseidon_hash_two_v05(&mid_new, &admin_mix_new);

    let admin_signers: [OligarchyAdminSigner; OLIGARCHY_K_MAX] =
        core::array::from_fn(|i| OligarchyAdminSigner {
            secret_key: admin_secret_keys[i],
            merkle_path: paths[i].clone(),
            leaf_index: i,
            active: true,
        });

    OligarchyUpdateQuorumWitness {
        c_old,
        epoch_old,
        c_new,
        occupancy_commitment_old: occ_old,
        occupancy_commitment_new: occ_new,
        admin_threshold_numerator: threshold,
        admin_signers,
        member_root_old: member_root,
        member_root_new: member_root,
        admin_root_old: admin_root,
        admin_root_new: admin_root,
        member_count_old: count_old,
        member_count_new: count_new,
        salt_oc_old,
        salt_oc_new,
        salt_old,
        salt_new,
    }
}

pub fn bake_oligarchy_create_vk() -> Result<Vec<u8>, BakeError> {
    let witness = build_canonical_oligarchy_create_witness();
    let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
    synthesize_oligarchy_create(&mut circuit, &witness).map_err(BakeError::Synthesize)?;
    circuit
        .finalize_for_arithmetization()
        .map_err(BakeError::Synthesize)?;
    let keys = plonk::preprocess(&circuit).map_err(BakeError::Preprocess)?;
    let mut vk_bytes = Vec::new();
    keys.vk
        .serialize_uncompressed(&mut vk_bytes)
        .map_err(BakeError::Serialize)?;
    Ok(vk_bytes)
}

/// Bake the oligarchy update VK against the K-of-N quorum circuit
/// (`synthesize_oligarchy_update_quorum`). Single-tier — admin tree
/// depth is fixed across all oligarchy member tiers.
pub fn bake_oligarchy_update_vk() -> Result<Vec<u8>, BakeError> {
    let witness = build_canonical_oligarchy_update_quorum_witness();
    let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
    synthesize_oligarchy_update_quorum(&mut circuit, &witness)
        .map_err(BakeError::Synthesize)?;
    circuit
        .finalize_for_arithmetization()
        .map_err(BakeError::Synthesize)?;
    let keys = plonk::preprocess(&circuit).map_err(BakeError::Preprocess)?;
    let mut vk_bytes = Vec::new();
    keys.vk
        .serialize_uncompressed(&mut vk_bytes)
        .map_err(BakeError::Serialize)?;
    Ok(vk_bytes)
}

/// Deterministic canonical witness for tier `(depth)`. The hash inputs
/// depend only on `depth`, so the circuit shape and resulting VK
/// depend only on `depth`.
///
/// Used as the canonical fixture for VK preprocessing and as the
/// cross-platform fingerprint anchor; non-Rust platforms reproducing
/// the canonical witness must match the same `(secret_keys,
/// prover_index, epoch, salt)` quadruple shown below.
pub fn build_canonical_membership_witness(depth: usize) -> MembershipWitness {
    // Deterministic secret-key set: 1, 2, ..., 8.
    let secret_keys: Vec<Fr> = (1u64..=8).map(Fr::from).collect();
    let prover_index = 3usize;
    let epoch: u64 = 1234;
    let salt: [u8; 32] = [0xEE; 32];

    // Native v0.5 leaf + tree build (matches what the gadget computes
    // in-circuit).
    let leaves: Vec<Fr> = secret_keys.iter().map(poseidon_hash_one_v05).collect();
    let num_leaves = 1usize << depth;
    let mut nodes = vec![Fr::from(0u64); 2 * num_leaves];
    for (i, leaf) in leaves.iter().enumerate() {
        nodes[num_leaves + i] = *leaf;
    }
    for i in (1..num_leaves).rev() {
        nodes[i] = poseidon_hash_two_v05(&nodes[2 * i], &nodes[2 * i + 1]);
    }
    let root = nodes[1];

    let mut path = Vec::with_capacity(depth);
    let mut cur = num_leaves + prover_index;
    for _ in 0..depth {
        let sib = if cur % 2 == 0 { cur + 1 } else { cur - 1 };
        path.push(nodes[sib]);
        cur /= 2;
    }

    let salt_fr = Fr::from_le_bytes_mod_order(&salt);
    let inner = poseidon_hash_two_v05(&root, &Fr::from(epoch));
    let commitment = poseidon_hash_two_v05(&inner, &salt_fr);

    MembershipWitness {
        commitment,
        epoch,
        secret_key: secret_keys[prover_index],
        poseidon_root: root,
        salt,
        merkle_path: path,
        leaf_index: prover_index,
        depth,
    }
}

/// Deterministic canonical witness for the **update** circuit at tier
/// `(depth)`. Reuses the membership canonical's secret keys, prover
/// index, old epoch, and old salt, and adds a fresh `salt_new` so the
/// transition `(salt_old → salt_new)` is well-formed without a roster
/// change. The new tree equals the old tree by construction; the
/// circuit doesn't constrain new-tree membership, so this is a valid
/// canonical witness.
///
/// **Reusing `root` for both old and new is a property of the canonical
/// fixture, not a property of the circuit.** The update circuit binds
/// `c_new` to `(poseidon_root_new, epoch_new, salt_new)` but never
/// proves the prover knows a leaf in `poseidon_root_new` — see the
/// "Security model" section in `circuit::plonk::update` for full
/// detail. Production callers can supply any `poseidon_root_new`;
/// downstream consumers must not interpret it as an authenticated
/// roster.
///
/// Hash inputs depend only on `depth`, so the resulting VK depends
/// only on `depth`.
pub fn build_canonical_update_witness(depth: usize) -> UpdateWitness {
    // Match `build_canonical_membership_witness` exactly so canonical
    // proofs at the two circuits share the same `(secret_key, root,
    // epoch_old, salt_old)` quadruple.
    let secret_keys: Vec<Fr> = (1u64..=8).map(Fr::from).collect();
    let prover_index = 3usize;
    let epoch_old: u64 = 1234;
    let salt_old: [u8; 32] = [0xEE; 32];
    let salt_new: [u8; 32] = [0xFF; 32];

    let leaves: Vec<Fr> = secret_keys.iter().map(poseidon_hash_one_v05).collect();
    let num_leaves = 1usize << depth;
    let mut nodes = vec![Fr::from(0u64); 2 * num_leaves];
    for (i, leaf) in leaves.iter().enumerate() {
        nodes[num_leaves + i] = *leaf;
    }
    for i in (1..num_leaves).rev() {
        nodes[i] = poseidon_hash_two_v05(&nodes[2 * i], &nodes[2 * i + 1]);
    }
    let root = nodes[1];

    let mut path = Vec::with_capacity(depth);
    let mut cur = num_leaves + prover_index;
    for _ in 0..depth {
        let sib = if cur % 2 == 0 { cur + 1 } else { cur - 1 };
        path.push(nodes[sib]);
        cur /= 2;
    }

    // Compute c_old, c_new natively.
    let salt_old_fr = Fr::from_le_bytes_mod_order(&salt_old);
    let salt_new_fr = Fr::from_le_bytes_mod_order(&salt_new);
    let c_old = poseidon_hash_two_v05(
        &poseidon_hash_two_v05(&root, &Fr::from(epoch_old)),
        &salt_old_fr,
    );
    let c_new = poseidon_hash_two_v05(
        &poseidon_hash_two_v05(&root, &Fr::from(epoch_old + 1)),
        &salt_new_fr,
    );

    UpdateWitness {
        c_old,
        epoch_old,
        c_new,
        secret_key: secret_keys[prover_index],
        poseidon_root_old: root,
        salt_old,
        merkle_path_old: path,
        leaf_index_old: prover_index,
        // No roster change — old root doubles as new root for the
        // canonical witness. Production usage will pass a different
        // tree; the circuit doesn't constrain new-tree membership.
        poseidon_root_new: root,
        salt_new,
        depth,
    }
}

/// Errors raised by the baker. Disjoint from `jf_plonk::PlonkError` so
/// callers can distinguish "invalid input" from "preprocess failed".
#[derive(Debug)]
pub enum BakeError {
    /// `depth` is outside the supported tier set (5, 8, 11).
    UnsupportedDepth(usize),
    /// `synthesize_membership` / `synthesize_update` rejected the
    /// canonical witness — should not happen for the supported
    /// depths; indicates a code change broke the canonical-witness
    /// invariant.
    Synthesize(jf_relation::CircuitError),
    /// `preprocess` failed; usually `IndexTooLarge` if the SRS is too
    /// small for the circuit.
    Preprocess(jf_plonk::errors::PlonkError),
    /// VK serialisation failed; arkworks `CanonicalSerialize` raised.
    Serialize(ark_serialize_v05::SerializationError),
}

impl core::fmt::Display for BakeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnsupportedDepth(d) => write!(f, "unsupported depth {d} (expected 5, 8, or 11)"),
            Self::Synthesize(e) => write!(f, "synthesize_membership failed: {e:?}"),
            Self::Preprocess(e) => write!(f, "preprocess failed: {e:?}"),
            Self::Serialize(e) => write!(f, "VK serialise failed: {e:?}"),
        }
    }
}

impl std::error::Error for BakeError {}

/// Build the canonical membership circuit for `depth`, run jf-plonk's
/// preprocessing against the embedded EF KZG SRS, and return the
/// arkworks-uncompressed verifying-key bytes.
///
/// Output is deterministic: the canonical witness is bit-deterministic,
/// the SRS is content-pinned, and `preprocess` is `(circuit, srs)`-
/// deterministic (proven by `prover::plonk::tests::preprocess_is_deterministic`).
/// Two invocations with the same `depth` therefore produce byte-identical
/// output.
///
/// Cross-check the SHA-256 of the returned bytes against
/// `pinned_vk_sha256_hex(depth)` — that comparison is the single anchor
/// shared by the cross-platform tests, the on-chain VK pin, and any
/// non-Rust prover.
pub fn bake_membership_vk(depth: usize) -> Result<Vec<u8>, BakeError> {
    if pinned_vk_sha256_hex(depth).is_none() {
        return Err(BakeError::UnsupportedDepth(depth));
    }

    let witness = build_canonical_membership_witness(depth);
    let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
    synthesize_membership(&mut circuit, &witness).map_err(BakeError::Synthesize)?;
    circuit
        .finalize_for_arithmetization()
        .map_err(BakeError::Synthesize)?;

    let keys = plonk::preprocess(&circuit).map_err(BakeError::Preprocess)?;

    let mut vk_bytes = Vec::new();
    keys.vk
        .serialize_uncompressed(&mut vk_bytes)
        .map_err(BakeError::Serialize)?;
    Ok(vk_bytes)
}

/// Canonical tyranny-create witness at `depth`. Reuses anarchy
/// canonical secret keys + admin at index 0; group_id_fr pinned to
/// `0x7777`.
pub fn build_canonical_tyranny_create_witness(depth: usize) -> TyrannyCreateWitness {
    let secret_keys: Vec<Fr> = (1u64..=8).map(Fr::from).collect();
    let admin_index = 0usize;
    let salt: [u8; 32] = [0xEEu8; 32];
    let group_id_fr = Fr::from(0x7777u64);

    let leaves: Vec<Fr> = secret_keys.iter().map(poseidon_hash_one_v05).collect();
    let num_leaves = 1usize << depth;
    let mut nodes = vec![Fr::from(0u64); 2 * num_leaves];
    for (i, leaf) in leaves.iter().enumerate() {
        nodes[num_leaves + i] = *leaf;
    }
    for i in (1..num_leaves).rev() {
        nodes[i] = poseidon_hash_two_v05(&nodes[2 * i], &nodes[2 * i + 1]);
    }
    let root = nodes[1];

    let mut path = Vec::with_capacity(depth);
    let mut cur = num_leaves + admin_index;
    for _ in 0..depth {
        let sib = if cur % 2 == 0 { cur + 1 } else { cur - 1 };
        path.push(nodes[sib]);
        cur /= 2;
    }

    let admin_leaf = poseidon_hash_one_v05(&secret_keys[admin_index]);
    let admin_comm = poseidon_hash_two_v05(&admin_leaf, &group_id_fr);
    let salt_fr = Fr::from_le_bytes_mod_order(&salt);
    let inner = poseidon_hash_two_v05(&root, &Fr::from(0u64));
    let commitment = poseidon_hash_two_v05(&inner, &salt_fr);

    TyrannyCreateWitness {
        commitment,
        admin_pubkey_commitment: admin_comm,
        group_id_fr,
        admin_secret_key: secret_keys[admin_index],
        member_root: root,
        salt,
        merkle_path: path,
        leaf_index: admin_index,
        depth,
    }
}

/// Canonical tyranny-update witness at `depth`. epoch_old=1234, same
/// admin (index 0), salt_old=0xEE, salt_new=0xFF, no roster change.
pub fn build_canonical_tyranny_update_witness(depth: usize) -> TyrannyUpdateWitness {
    let secret_keys: Vec<Fr> = (1u64..=8).map(Fr::from).collect();
    let admin_index = 0usize;
    let epoch_old: u64 = 1234;
    let salt_old: [u8; 32] = [0xEEu8; 32];
    let salt_new: [u8; 32] = [0xFFu8; 32];
    let group_id_fr = Fr::from(0x7777u64);

    let leaves: Vec<Fr> = secret_keys.iter().map(poseidon_hash_one_v05).collect();
    let num_leaves = 1usize << depth;
    let mut nodes = vec![Fr::from(0u64); 2 * num_leaves];
    for (i, leaf) in leaves.iter().enumerate() {
        nodes[num_leaves + i] = *leaf;
    }
    for i in (1..num_leaves).rev() {
        nodes[i] = poseidon_hash_two_v05(&nodes[2 * i], &nodes[2 * i + 1]);
    }
    let root = nodes[1];

    let mut path = Vec::with_capacity(depth);
    let mut cur = num_leaves + admin_index;
    for _ in 0..depth {
        let sib = if cur % 2 == 0 { cur + 1 } else { cur - 1 };
        path.push(nodes[sib]);
        cur /= 2;
    }

    let admin_leaf = poseidon_hash_one_v05(&secret_keys[admin_index]);
    let admin_comm = poseidon_hash_two_v05(&admin_leaf, &group_id_fr);
    let salt_old_fr = Fr::from_le_bytes_mod_order(&salt_old);
    let salt_new_fr = Fr::from_le_bytes_mod_order(&salt_new);
    let c_old = poseidon_hash_two_v05(
        &poseidon_hash_two_v05(&root, &Fr::from(epoch_old)),
        &salt_old_fr,
    );
    let c_new = poseidon_hash_two_v05(
        &poseidon_hash_two_v05(&root, &Fr::from(epoch_old + 1)),
        &salt_new_fr,
    );

    TyrannyUpdateWitness {
        c_old,
        epoch_old,
        c_new,
        admin_pubkey_commitment: admin_comm,
        group_id_fr,
        admin_secret_key: secret_keys[admin_index],
        member_root_old: root,
        member_root_new: root,
        salt_old,
        salt_new,
        merkle_path_old: path,
        leaf_index_old: admin_index,
        depth,
    }
}

pub fn bake_tyranny_create_vk(depth: usize) -> Result<Vec<u8>, BakeError> {
    if pinned_tyranny_create_vk_sha256_hex(depth).is_none() {
        return Err(BakeError::UnsupportedDepth(depth));
    }
    let witness = build_canonical_tyranny_create_witness(depth);
    let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
    synthesize_tyranny_create(&mut circuit, &witness).map_err(BakeError::Synthesize)?;
    circuit
        .finalize_for_arithmetization()
        .map_err(BakeError::Synthesize)?;
    let keys = plonk::preprocess(&circuit).map_err(BakeError::Preprocess)?;
    let mut vk_bytes = Vec::new();
    keys.vk
        .serialize_uncompressed(&mut vk_bytes)
        .map_err(BakeError::Serialize)?;
    Ok(vk_bytes)
}

pub fn bake_tyranny_update_vk(depth: usize) -> Result<Vec<u8>, BakeError> {
    if pinned_tyranny_update_vk_sha256_hex(depth).is_none() {
        return Err(BakeError::UnsupportedDepth(depth));
    }
    let witness = build_canonical_tyranny_update_witness(depth);
    let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
    synthesize_tyranny_update(&mut circuit, &witness).map_err(BakeError::Synthesize)?;
    circuit
        .finalize_for_arithmetization()
        .map_err(BakeError::Synthesize)?;
    let keys = plonk::preprocess(&circuit).map_err(BakeError::Preprocess)?;
    let mut vk_bytes = Vec::new();
    keys.vk
        .serialize_uncompressed(&mut vk_bytes)
        .map_err(BakeError::Serialize)?;
    Ok(vk_bytes)
}

/// Deterministic canonical witness for the **1v1 create** circuit.
/// Single tier (depth=5 only); two founding members at positions 0/1.
pub fn build_canonical_oneonone_create_witness() -> OneOnOneCreateWitness {
    let _ = ONEONONE_DEPTH; // compile-time pin: depth=5 in oneonone_create.rs
    OneOnOneCreateWitness {
        commitment: {
            // Compute natively to match the in-circuit derivation.
            let leaf_0 = poseidon_hash_one_v05(&Fr::from(1u64));
            let leaf_1 = poseidon_hash_one_v05(&Fr::from(2u64));
            // Walk the active spine: position 0 at every level; right
            // sibling is the zero-subtree hash.
            let zero_subtrees: [Fr; 5] = {
                let mut z = [Fr::from(0u64); 5];
                z[0] = poseidon_hash_two_v05(&Fr::from(0u64), &Fr::from(0u64));
                for i in 1..5 {
                    z[i] = poseidon_hash_two_v05(&z[i - 1], &z[i - 1]);
                }
                z
            };
            let mut current = poseidon_hash_two_v05(&leaf_0, &leaf_1);
            for i in 1..5 {
                current = poseidon_hash_two_v05(&current, &zero_subtrees[i - 1]);
            }
            let root = current;
            let salt = [0xEEu8; 32];
            let salt_fr = Fr::from_le_bytes_mod_order(&salt);
            let inner = poseidon_hash_two_v05(&root, &Fr::from(0u64));
            poseidon_hash_two_v05(&inner, &salt_fr)
        },
        secret_key_0: Fr::from(1u64),
        secret_key_1: Fr::from(2u64),
        salt: [0xEEu8; 32],
    }
}

/// Bake the OneOnOne-create VK. Single tier (depth=5).
pub fn bake_oneonone_create_vk() -> Result<Vec<u8>, BakeError> {
    let witness = build_canonical_oneonone_create_witness();
    let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
    synthesize_oneonone_create(&mut circuit, &witness).map_err(BakeError::Synthesize)?;
    circuit
        .finalize_for_arithmetization()
        .map_err(BakeError::Synthesize)?;

    let keys = plonk::preprocess(&circuit).map_err(BakeError::Preprocess)?;

    let mut vk_bytes = Vec::new();
    keys.vk
        .serialize_uncompressed(&mut vk_bytes)
        .map_err(BakeError::Serialize)?;
    Ok(vk_bytes)
}

/// Build the canonical update circuit for `depth`, run jf-plonk's
/// preprocessing against the embedded EF KZG SRS, and return the
/// arkworks-uncompressed verifying-key bytes.
///
/// Same determinism guarantees as `bake_membership_vk`. Cross-check the
/// SHA-256 against `pinned_update_vk_sha256_hex(depth)`.
pub fn bake_update_vk(depth: usize) -> Result<Vec<u8>, BakeError> {
    if pinned_update_vk_sha256_hex(depth).is_none() {
        return Err(BakeError::UnsupportedDepth(depth));
    }

    let witness = build_canonical_update_witness(depth);
    let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
    synthesize_update(&mut circuit, &witness).map_err(BakeError::Synthesize)?;
    circuit
        .finalize_for_arithmetization()
        .map_err(BakeError::Synthesize)?;

    let keys = plonk::preprocess(&circuit).map_err(BakeError::Preprocess)?;

    let mut vk_bytes = Vec::new();
    keys.vk
        .serialize_uncompressed(&mut vk_bytes)
        .map_err(BakeError::Serialize)?;
    Ok(vk_bytes)
}

/// SHA-256 of `bytes`, hex-encoded (lowercase). Format-compatible with
/// `pinned_vk_sha256_hex` / `pinned_update_vk_sha256_hex`.
pub fn vk_sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{:02x}", b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each tier's bake produces the pinned SHA-256. This is the load-
    /// bearing test for the cross-platform anchor: if it fails, either
    /// (a) `bake_membership_vk` drifted, (b) the canonical witness
    /// changed, or (c) jf-plonk / arkworks shifted byte layout.
    #[test]
    fn bake_membership_vk_matches_pinned_for_all_tiers() {
        for &depth in &[5usize, 8, 11] {
            let bytes = bake_membership_vk(depth)
                .unwrap_or_else(|e| panic!("bake_membership_vk(depth={depth}) failed: {e}"));
            let computed = vk_sha256_hex(&bytes);
            let pinned = pinned_vk_sha256_hex(depth).unwrap();
            assert_eq!(
                computed, pinned,
                "bake_membership_vk(depth={depth}) drifted from pinned SHA-256. \
                 Either the circuit shape changed (audit the diff) or the canonical \
                 witness changed (update both VK_SHA256_HEX_* in baker.rs and \
                 docs/cross-platform-test-vectors.json)."
            );
        }
    }

    /// Determinism: re-baking the same tier produces byte-identical
    /// output. Cheap regression check that nothing in the preprocessing
    /// path picked up RNG or wall-clock state.
    #[test]
    fn bake_membership_vk_is_deterministic() {
        let a = bake_membership_vk(5).expect("first bake");
        let b = bake_membership_vk(5).expect("second bake");
        assert_eq!(a, b, "bake output is non-deterministic");
    }

    /// Unsupported depths are rejected up-front, before doing any
    /// preprocessing work.
    #[test]
    fn bake_membership_vk_rejects_unsupported_depth() {
        match bake_membership_vk(7) {
            Err(BakeError::UnsupportedDepth(7)) => {}
            other => panic!("expected UnsupportedDepth(7), got {other:?}"),
        }
    }

    /// Update-circuit equivalent of
    /// `bake_membership_vk_matches_pinned_for_all_tiers`. Anchors the
    /// update VK shape across builds and platforms — when the pinned
    /// hashes need to be regenerated (legitimate circuit change), this
    /// test fails first with the actual SHA-256s for all tiers, which
    /// can then be pasted into `UPDATE_VK_SHA256_HEX_*`.
    #[test]
    fn bake_update_vk_matches_pinned_for_all_tiers() {
        // Compute all tiers first so a single test run produces every
        // SHA-256 (rather than panicking on the first mismatch and
        // hiding the rest).
        let mut mismatches = Vec::new();
        for &depth in &[5usize, 8, 11] {
            let bytes = bake_update_vk(depth)
                .unwrap_or_else(|e| panic!("bake_update_vk(depth={depth}) failed: {e}"));
            let computed = vk_sha256_hex(&bytes);
            let pinned = pinned_update_vk_sha256_hex(depth).unwrap();
            if computed != pinned {
                mismatches.push(format!(
                    "depth={depth}: computed={computed}, pinned={pinned}"
                ));
            }
        }
        assert!(
            mismatches.is_empty(),
            "bake_update_vk drifted from pinned SHA-256s. Either the \
             update circuit shape changed (audit the diff) or the \
             canonical update witness changed (update both \
             UPDATE_VK_SHA256_HEX_* in baker.rs and any cross-platform \
             anchor):\n  {}",
            mismatches.join("\n  ")
        );
    }

    /// Update bake is deterministic across calls.
    #[test]
    fn bake_update_vk_is_deterministic() {
        let a = bake_update_vk(5).expect("first bake");
        let b = bake_update_vk(5).expect("second bake");
        assert_eq!(a, b, "update bake output is non-deterministic");
    }

    /// Unsupported depth is rejected up-front.
    #[test]
    fn bake_update_vk_rejects_unsupported_depth() {
        match bake_update_vk(7) {
            Err(BakeError::UnsupportedDepth(7)) => {}
            other => panic!("expected UnsupportedDepth(7), got {other:?}"),
        }
    }

    /// Anchor for tyranny-create + tyranny-update VK shapes across all
    /// three tiers. Mismatches print computed=… so the failure surface
    /// can be pasted into the `TYRANNY_*_VK_SHA256_HEX_*` constants.
    #[test]
    fn bake_tyranny_vks_match_pinned_for_all_tiers() {
        let mut mismatches = Vec::new();
        for &depth in &[5usize, 8, 11] {
            for (label, computed, pinned) in [
                (
                    "tyranny-create",
                    vk_sha256_hex(&bake_tyranny_create_vk(depth).unwrap()),
                    pinned_tyranny_create_vk_sha256_hex(depth).unwrap(),
                ),
                (
                    "tyranny-update",
                    vk_sha256_hex(&bake_tyranny_update_vk(depth).unwrap()),
                    pinned_tyranny_update_vk_sha256_hex(depth).unwrap(),
                ),
            ] {
                if computed != pinned {
                    mismatches.push(format!(
                        "{label} depth={depth}: computed={computed}, pinned={pinned}"
                    ));
                }
            }
        }
        assert!(mismatches.is_empty(), "tyranny VK pin drift:\n  {}", mismatches.join("\n  "));
    }

    /// Anchor for the OneOnOne-create VK shape.
    #[test]
    fn bake_oneonone_create_vk_matches_pinned() {
        let bytes = bake_oneonone_create_vk().expect("bake");
        let computed = vk_sha256_hex(&bytes);
        assert_eq!(
            computed, ONEONONE_CREATE_VK_SHA256_HEX,
            "bake_oneonone_create_vk drifted from pinned SHA-256: \
             computed={computed}, pinned={ONEONONE_CREATE_VK_SHA256_HEX}",
        );
    }

    /// OneOnOne-create bake is deterministic.
    #[test]
    fn bake_oneonone_create_vk_is_deterministic() {
        let a = bake_oneonone_create_vk().expect("first bake");
        let b = bake_oneonone_create_vk().expect("second bake");
        assert_eq!(a, b, "oneonone-create bake is non-deterministic");
    }

    /// Anchor for democracy-update VK shapes across all 3 tiers.
    #[test]
    fn bake_democracy_update_vk_matches_pinned_for_all_tiers() {
        let mut mismatches = Vec::new();
        for &depth in &[5usize, 8, 11] {
            let computed = vk_sha256_hex(&bake_democracy_update_vk(depth).unwrap());
            let pinned = pinned_democracy_update_vk_sha256_hex(depth).unwrap();
            if computed != pinned {
                mismatches.push(format!(
                    "democracy-update depth={depth}: computed={computed}, pinned={pinned}"
                ));
            }
        }
        assert!(mismatches.is_empty(), "democracy-update VK pin drift:\n  {}", mismatches.join("\n  "));
    }

    /// Anchor for oligarchy create + update VK shapes.
    #[test]
    fn bake_oligarchy_vks_match_pinned() {
        let mut mismatches = Vec::new();
        let create_computed = vk_sha256_hex(&bake_oligarchy_create_vk().unwrap());
        if create_computed != OLIGARCHY_CREATE_VK_SHA256_HEX {
            mismatches.push(format!(
                "oligarchy-create: computed={create_computed}, pinned={OLIGARCHY_CREATE_VK_SHA256_HEX}"
            ));
        }
        let update_computed = vk_sha256_hex(&bake_oligarchy_update_vk().unwrap());
        if update_computed != OLIGARCHY_UPDATE_VK_SHA256_HEX {
            mismatches.push(format!(
                "oligarchy-update: computed={update_computed}, pinned={OLIGARCHY_UPDATE_VK_SHA256_HEX}"
            ));
        }
        assert!(mismatches.is_empty(), "oligarchy VK pin drift:\n  {}", mismatches.join("\n  "));
    }
}
