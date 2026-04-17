#ifndef SEP_FFI_H
#define SEP_FFI_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct sep_byte_buffer_t {
    uint8_t *ptr;
    size_t len;
} sep_byte_buffer_t;

void sep_byte_buffer_free(sep_byte_buffer_t buffer);
void sep_string_free(char *ptr);

bool sep_compute_leaf_hash(
    const uint8_t *secret_key_ptr,
    size_t secret_key_len,
    sep_byte_buffer_t *out_leaf_hash,
    char **out_error
);

bool sep_compute_public_key(
    const uint8_t *secret_key_ptr,
    size_t secret_key_len,
    sep_byte_buffer_t *out_public_key,
    char **out_error
);

bool sep_compute_merkle_root(
    const uint8_t *member_public_keys_ptr,
    size_t member_public_keys_len,
    const uint8_t *leaf_hashes_ptr,
    size_t leaf_hashes_len,
    size_t depth,
    sep_byte_buffer_t *out_root,
    char **out_error
);

bool sep_nostr_derive_public_key(
    const uint8_t *secret_key_ptr,
    size_t secret_key_len,
    sep_byte_buffer_t *out_public_key,
    char **out_error
);

bool sep_nostr_sign_event_id(
    const uint8_t *secret_key_ptr,
    size_t secret_key_len,
    const uint8_t *event_id_ptr,
    size_t event_id_len,
    sep_byte_buffer_t *out_signature,
    char **out_error
);

bool sep_nostr_verify_event_signature(
    const uint8_t *public_key_ptr,
    size_t public_key_len,
    const uint8_t *event_id_ptr,
    size_t event_id_len,
    const uint8_t *signature_ptr,
    size_t signature_len,
    char **out_error
);

bool sep_proof_to_contract_format(
    const uint8_t *compressed_proof_ptr,
    size_t compressed_proof_len,
    sep_byte_buffer_t *out_proof_a,
    sep_byte_buffer_t *out_proof_b,
    sep_byte_buffer_t *out_proof_c,
    char **out_error
);

bool sep_compute_sha256_commitment(
    const uint8_t *poseidon_root_ptr,
    size_t poseidon_root_len,
    uint64_t epoch,
    const uint8_t *salt_ptr,
    size_t salt_len,
    sep_byte_buffer_t *out_commitment,
    char **out_error
);

bool sep_compute_poseidon_commitment(
    const uint8_t *poseidon_root_ptr,
    size_t poseidon_root_len,
    uint64_t epoch,
    const uint8_t *salt_ptr,
    size_t salt_len,
    sep_byte_buffer_t *out_commitment,
    char **out_error
);

bool sep_generate_testing_proving_key(
    size_t depth,
    uint64_t seed,
    sep_byte_buffer_t *out_proving_key,
    char **out_error
);

bool sep_generate_membership_proof(
    const uint8_t *proving_key_ptr,
    size_t proving_key_len,
    const uint8_t *member_public_keys_ptr,
    size_t member_public_keys_len,
    const uint8_t *leaf_hashes_ptr,
    size_t leaf_hashes_len,
    const uint8_t *secret_key_ptr,
    size_t secret_key_len,
    uint64_t epoch,
    const uint8_t *salt_ptr,
    size_t salt_len,
    size_t depth,
    sep_byte_buffer_t *out_proof,
    sep_byte_buffer_t *out_commitment,
    char **out_error
);

bool sep_generate_testing_update_proving_key(
    size_t depth,
    uint64_t seed,
    sep_byte_buffer_t *out_proving_key,
    char **out_error
);

bool sep_generate_update_proof(
    const uint8_t *proving_key_ptr,
    size_t proving_key_len,
    const uint8_t *old_public_keys_ptr,
    size_t old_public_keys_len,
    const uint8_t *old_leaf_hashes_ptr,
    size_t old_leaf_hashes_len,
    const uint8_t *new_public_keys_ptr,
    size_t new_public_keys_len,
    const uint8_t *new_leaf_hashes_ptr,
    size_t new_leaf_hashes_len,
    const uint8_t *secret_key_ptr,
    size_t secret_key_len,
    uint64_t epoch_old,
    const uint8_t *salt_old_ptr,
    size_t salt_old_len,
    const uint8_t *salt_new_ptr,
    size_t salt_new_len,
    size_t depth,
    sep_byte_buffer_t *out_proof,
    sep_byte_buffer_t *out_public_inputs,
    char **out_error
);

bool sep_parse_update_public_inputs(
    const uint8_t *public_inputs_ptr,
    size_t public_inputs_len,
    sep_byte_buffer_t *out_c_old,
    sep_byte_buffer_t *out_epoch_old,
    sep_byte_buffer_t *out_c_new,
    char **out_error
);

#endif
