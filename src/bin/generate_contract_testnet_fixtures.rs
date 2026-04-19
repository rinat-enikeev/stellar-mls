use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use ark_bls12_381::{Bls12_381, Fr, G1Affine, G2Affine};
use ark_groth16::{PreparedVerifyingKey, Proof, ProvingKey, VerifyingKey};
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use sha2::{Digest, Sha256};

use sep_xxxx_circuits::commitment::field_to_bytes_be;
use sep_xxxx_circuits::merkle::{CanonicalMember, compressed_public_key_bytes};
use sep_xxxx_circuits::prover::{
    self, ProverInput, SetupResult, UpdateProverInput, compute_leaf_hash,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args(env::args().skip(1))?;
    fs::create_dir_all(&args.out_dir)?;

    let (small_setup, medium_setup, large_setup,
         small_update_setup, medium_update_setup, large_update_setup) =
        if let Some(keyset_dir) = &args.keyset_dir {
            (
                load_setup(&keyset_dir.join("small/proving_key.bin"),
                           &keyset_dir.join("small/verifying_key.bin"))?,
                load_setup(&keyset_dir.join("medium/proving_key.bin"),
                           &keyset_dir.join("medium/verifying_key.bin"))?,
                load_setup(&keyset_dir.join("large/proving_key.bin"),
                           &keyset_dir.join("large/verifying_key.bin"))?,
                load_setup(&keyset_dir.join("small/update_proving_key.bin"),
                           &keyset_dir.join("small/update_verifying_key.bin"))?,
                load_setup(&keyset_dir.join("medium/update_proving_key.bin"),
                           &keyset_dir.join("medium/update_verifying_key.bin"))?,
                load_setup(&keyset_dir.join("large/update_proving_key.bin"),
                           &keyset_dir.join("large/update_verifying_key.bin"))?,
            )
        } else {
            (
                deterministic_setup(5, 1001)?,
                deterministic_setup(8, 1002)?,
                deterministic_setup(11, 1003)?,
                deterministic_update_setup(5, 2001)?,
                deterministic_update_setup(8, 2002)?,
                deterministic_update_setup(11, 2003)?,
            )
        };

    let out_dir = args.out_dir.clone();

    let member_secret_keys = vec![Fr::from(100u64), Fr::from(200u64)];
    let members = member_secret_keys
        .iter()
        .map(|secret_key| CanonicalMember {
            public_key_bytes: compressed_public_key_bytes(secret_key),
            leaf_hash: compute_leaf_hash(secret_key),
        })
        .collect::<Vec<_>>();

    let group_id = Sha256::digest(b"sep-xxxx-testnet-integration-group");
    let salt_epoch_0 = [0x11; 32];
    let salt_epoch_1 = [0x22; 32];

    let mut prove_rng = ChaCha20Rng::seed_from_u64(2026);
    let input_epoch_0 = ProverInput {
        members: members.clone(),
        secret_key: member_secret_keys[0],
        epoch: 0,
        salt: salt_epoch_0,
        depth: 5,
    };
    let (proof_epoch_0_create, public_inputs_epoch_0) =
        prover::prove(&small_setup.proving_key, &input_epoch_0, &mut prove_rng)?;

    let input_epoch_1 = ProverInput {
        members: members.clone(),
        secret_key: member_secret_keys[0],
        epoch: 1,
        salt: salt_epoch_1,
        depth: 5,
    };
    let (proof_epoch_1, public_inputs_epoch_1) =
        prover::prove(&small_setup.proving_key, &input_epoch_1, &mut prove_rng)?;

    // UpdateCircuit fixture: prove the 0 → 1 transition.
    // Members_old == members_new (no roster change), so root_old == root_new.
    // Salts differ (per-epoch salt rotation), and the circuit binds the epoch
    // increment to 1 via the in-circuit `epoch_new = epoch_old + 1` constraint.
    let update_input = UpdateProverInput {
        members_old: members.clone(),
        members_new: members,
        secret_key: member_secret_keys[0],
        epoch_old: 0,
        salt_old: salt_epoch_0,
        salt_new: salt_epoch_1,
        depth: 5,
    };
    let (proof_update, update_public_inputs) = prover::prove_update(
        &small_update_setup.proving_key,
        &update_input,
        &mut prove_rng,
    )?;

    // Sanity: the UpdateCircuit's c_old / c_new must coincide with the
    // membership commitments at epochs 0 and 1, otherwise the contract's
    // on-chain state cross-check at update_commitment would fail.
    assert_eq!(
        field_to_bytes_be(&update_public_inputs.c_old),
        field_to_bytes_be(&public_inputs_epoch_0.commitment),
        "UpdateCircuit c_old must match membership commitment at epoch 0",
    );
    assert_eq!(
        field_to_bytes_be(&update_public_inputs.c_new),
        field_to_bytes_be(&public_inputs_epoch_1.commitment),
        "UpdateCircuit c_new must match membership commitment at epoch 1",
    );

    write_string(
        &out_dir.join("group-id.hex"),
        &format!("{}\n", hex_encode(group_id.as_slice())),
    )?;
    write_string(
        &out_dir.join("tier.txt"),
        "0\n",
    )?;
    write_string(
        &out_dir.join("commitment-epoch-0.hex"),
        &format!("{}\n", hex_encode(&field_to_bytes_be(&public_inputs_epoch_0.commitment))),
    )?;
    write_string(
        &out_dir.join("commitment-epoch-1.hex"),
        &format!("{}\n", hex_encode(&field_to_bytes_be(&public_inputs_epoch_1.commitment))),
    )?;

    write_string(
        &out_dir.join("vk-small.json"),
        &verification_key_json(&small_setup.verifying_key),
    )?;
    write_string(
        &out_dir.join("vk-medium.json"),
        &verification_key_json(&medium_setup.verifying_key),
    )?;
    write_string(
        &out_dir.join("vk-large.json"),
        &verification_key_json(&large_setup.verifying_key),
    )?;
    write_string(
        &out_dir.join("vk-update-small.json"),
        &verification_key_json(&small_update_setup.verifying_key),
    )?;
    write_string(
        &out_dir.join("vk-update-medium.json"),
        &verification_key_json(&medium_update_setup.verifying_key),
    )?;
    write_string(
        &out_dir.join("vk-update-large.json"),
        &verification_key_json(&large_update_setup.verifying_key),
    )?;
    write_string(
        &out_dir.join("proof-epoch-0-create.json"),
        &proof_json(&proof_epoch_0_create),
    )?;
    write_string(
        &out_dir.join("proof-epoch-1.json"),
        &proof_json(&proof_epoch_1),
    )?;
    write_string(
        &out_dir.join("proof-update-0-to-1.json"),
        &proof_json(&proof_update),
    )?;
    write_string(
        &out_dir.join("update-public-inputs-0-to-1.json"),
        &update_public_inputs_json(
            &field_to_bytes_be(&update_public_inputs.c_old),
            0,
            &field_to_bytes_be(&update_public_inputs.c_new),
        ),
    )?;

    write_string(
        &out_dir.join("public-inputs-epoch-0.json"),
        &public_inputs_json(
            &field_to_bytes_be(&public_inputs_epoch_0.commitment),
            0,
        ),
    )?;
    write_string(
        &out_dir.join("public-inputs-epoch-1.json"),
        &public_inputs_json(
            &field_to_bytes_be(&public_inputs_epoch_1.commitment),
            1,
        ),
    )?;

    let summary = format!(
        concat!(
            "{{\n",
            "  \"group_id\": \"{}\",\n",
            "  \"tier\": 0,\n",
            "  \"initial_commitment\": \"{}\",\n",
            "  \"next_commitment\": \"{}\",\n",
            "  \"files\": {{\n",
            "    \"vk_small\": \"vk-small.json\",\n",
            "    \"vk_medium\": \"vk-medium.json\",\n",
            "    \"vk_large\": \"vk-large.json\",\n",
            "    \"vk_update_small\": \"vk-update-small.json\",\n",
            "    \"vk_update_medium\": \"vk-update-medium.json\",\n",
            "    \"vk_update_large\": \"vk-update-large.json\",\n",
            "    \"proof_epoch_0_create\": \"proof-epoch-0-create.json\",\n",
            "    \"proof_epoch_1\": \"proof-epoch-1.json\",\n",
            "    \"proof_update_0_to_1\": \"proof-update-0-to-1.json\",\n",
            "    \"public_inputs_epoch_0\": \"public-inputs-epoch-0.json\",\n",
            "    \"public_inputs_epoch_1\": \"public-inputs-epoch-1.json\",\n",
            "    \"update_public_inputs_0_to_1\": \"update-public-inputs-0-to-1.json\"\n",
            "  }}\n",
            "}}\n"
        ),
        hex_encode(group_id.as_slice()),
        hex_encode(&field_to_bytes_be(&public_inputs_epoch_0.commitment)),
        hex_encode(&field_to_bytes_be(&public_inputs_epoch_1.commitment)),
    );
    write_string(&out_dir.join("summary.json"), &summary)?;

    Ok(())
}

struct Args {
    out_dir: PathBuf,
    /// When set, the 6 Groth16 setups (3 membership + 3 update) are loaded
    /// from `<keyset-dir>/<tier>/{,update_}{proving,verifying}_key.bin`
    /// instead of being regenerated from hardcoded RNG seeds. This makes
    /// the installed contract VKs match the keys clients bundle, so
    /// client-generated proofs verify on-chain.
    keyset_dir: Option<PathBuf>,
}

fn parse_args(
    mut args: impl Iterator<Item = String>,
) -> Result<Args, Box<dyn std::error::Error>> {
    let mut out_dir: Option<PathBuf> = None;
    let mut keyset_dir: Option<PathBuf> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out-dir" => {
                let value = args.next().ok_or("--out-dir requires a value")?;
                out_dir = Some(PathBuf::from(value));
            }
            "--keyset-dir" => {
                let value = args.next().ok_or("--keyset-dir requires a value")?;
                keyset_dir = Some(PathBuf::from(value));
            }
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }

    Ok(Args {
        out_dir: out_dir.ok_or("--out-dir is required")?,
        keyset_dir,
    })
}

fn load_setup(
    pk_path: &Path,
    vk_path: &Path,
) -> Result<SetupResult, Box<dyn std::error::Error>> {
    let pk_bytes = fs::read(pk_path)
        .map_err(|e| format!("reading {}: {}", pk_path.display(), e))?;
    let proving_key =
        ProvingKey::<Bls12_381>::deserialize_compressed(&pk_bytes[..])
            .map_err(|e| format!("deserializing {}: {}", pk_path.display(), e))?;

    let vk_bytes = fs::read(vk_path)
        .map_err(|e| format!("reading {}: {}", vk_path.display(), e))?;
    let verifying_key =
        VerifyingKey::<Bls12_381>::deserialize_compressed(&vk_bytes[..])
            .map_err(|e| format!("deserializing {}: {}", vk_path.display(), e))?;

    let prepared_vk = PreparedVerifyingKey::from(verifying_key.clone());

    Ok(SetupResult {
        proving_key,
        verifying_key,
        prepared_vk,
    })
}

fn deterministic_setup(
    depth: usize,
    seed: u64,
) -> Result<SetupResult, Box<dyn std::error::Error>> {
    let mut rng = ChaCha20Rng::seed_from_u64(seed);
    prover::setup(depth, &mut rng)
}

fn deterministic_update_setup(
    depth: usize,
    seed: u64,
) -> Result<SetupResult, Box<dyn std::error::Error>> {
    let mut rng = ChaCha20Rng::seed_from_u64(seed);
    prover::setup_update(depth, &mut rng)
}

fn verification_key_json(vk: &VerifyingKey<Bls12_381>) -> String {
    let ic = vk
        .gamma_abc_g1
        .iter()
        .map(|point| format!("\"{}\"", hex_encode(&serialize_g1(point))))
        .collect::<Vec<_>>()
        .join(",");

    format!(
        concat!(
            "{{",
            "\"alpha_g1\":\"{}\",",
            "\"beta_g2\":\"{}\",",
            "\"gamma_g2\":\"{}\",",
            "\"delta_g2\":\"{}\",",
            "\"ic\":[{}]",
            "}}\n"
        ),
        hex_encode(&serialize_g1(&vk.alpha_g1)),
        hex_encode(&serialize_g2(&vk.beta_g2)),
        hex_encode(&serialize_g2(&vk.gamma_g2)),
        hex_encode(&serialize_g2(&vk.delta_g2)),
        ic,
    )
}

fn proof_json(proof: &Proof<Bls12_381>) -> String {
    format!(
        concat!(
            "{{",
            "\"a\":\"{}\",",
            "\"b\":\"{}\",",
            "\"c\":\"{}\"",
            "}}\n"
        ),
        hex_encode(&serialize_g1(&proof.a)),
        hex_encode(&serialize_g2(&proof.b)),
        hex_encode(&serialize_g1(&proof.c)),
    )
}

fn public_inputs_json(commitment_bytes: &[u8], epoch: u64) -> String {
    format!(
        concat!(
            "{{",
            "\"commitment\":\"{}\",",
            "\"epoch\":{}",
            "}}\n"
        ),
        hex_encode(commitment_bytes),
        epoch,
    )
}

fn update_public_inputs_json(c_old_bytes: &[u8], epoch_old: u64, c_new_bytes: &[u8]) -> String {
    format!(
        concat!(
            "{{",
            "\"c_old\":\"{}\",",
            "\"epoch_old\":{},",
            "\"c_new\":\"{}\"",
            "}}\n"
        ),
        hex_encode(c_old_bytes),
        epoch_old,
        hex_encode(c_new_bytes),
    )
}

fn serialize_g1(point: &G1Affine) -> Vec<u8> {
    let mut bytes = Vec::new();
    point
        .serialize_uncompressed(&mut bytes)
        .expect("G1 serialization should succeed");
    bytes
}

fn serialize_g2(point: &G2Affine) -> Vec<u8> {
    let mut bytes = Vec::new();
    point
        .serialize_uncompressed(&mut bytes)
        .expect("G2 serialization should succeed");
    bytes
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(nibble_to_hex(byte >> 4));
        out.push(nibble_to_hex(byte & 0x0f));
    }
    out
}

fn nibble_to_hex(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'a' + (nibble - 10)) as char,
        _ => unreachable!("nibble out of range"),
    }
}

fn write_string(path: &Path, contents: &str) -> Result<(), Box<dyn std::error::Error>> {
    fs::write(path, contents)?;
    Ok(())
}
