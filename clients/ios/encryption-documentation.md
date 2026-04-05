# App Encryption Documentation — Onym (chat.onym.ios)

## Overview

Onym is an end-to-end encrypted group messaging application. It uses standard, publicly available encryption algorithms for message confidentiality, key agreement, digital signatures, and data-at-rest protection. No proprietary or non-standard encryption algorithms are used.

All cryptographic algorithms used are accepted standards published by international standard bodies (IETF, IEEE, NIST, W3C/IRTF).

## Encryption Algorithms Used

### 1. AES-256-GCM (NIST SP 800-38D)

- **Purpose:** End-to-end encryption of chat messages, media files (images, video, voice messages), push notification payloads, and local data-at-rest storage encryption.
- **Key length:** 256-bit symmetric keys.
- **Standard:** NIST Special Publication 800-38D; IETF RFC 5116.
- **Implementation:** Apple CryptoKit framework (`AES.GCM`).

### 2. HKDF-SHA256 (IETF RFC 5869)

- **Purpose:** Deriving encryption keys from shared secrets (message traffic keys, storage encryption keys, inbox keys).
- **Key length:** 256-bit derived keys.
- **Standard:** IETF RFC 5869 (HMAC-based Extract-and-Expand Key Derivation Function).
- **Implementation:** Apple CryptoKit framework (`HKDF<SHA256>`).

### 3. X25519 ECDH (IETF RFC 7748)

- **Purpose:** Ephemeral key agreement for encrypted group invitations and push notification relay registration.
- **Key length:** 256-bit (Curve25519).
- **Standard:** IETF RFC 7748.
- **Implementation:** Apple CryptoKit framework (`Curve25519.KeyAgreement`).

### 4. secp256k1 Schnorr Signatures (BIP-340)

- **Purpose:** Signing Nostr protocol events (NIP-01) for message authentication and integrity.
- **Key length:** 256-bit (secp256k1 elliptic curve).
- **Standard:** BIP-340 (Bitcoin Improvement Proposal); secp256k1 curve defined by SEC 2 (Standards for Efficient Cryptography).
- **Implementation:** Rust FFI library (SwiftMLS).

### 5. Ed25519 (IETF RFC 8032)

- **Purpose:** Stellar account key signing and key attestation binding.
- **Key length:** 256-bit (Edwards curve).
- **Standard:** IETF RFC 8032 (Edwards-Curve Digital Signature Algorithm).
- **Implementation:** Derived via HKDF from the Nostr key; used for on-chain identity binding.

### 6. BLS12-381 (IETF draft-irtf-cfrg-bls-signature)

- **Purpose:** Group membership commitments and sender authentication. BLS public keys are used as leaves in a Poseidon Merkle tree for zero-knowledge membership proofs.
- **Key length:** 381-bit (BLS12-381 pairing-friendly curve).
- **Standard:** IETF draft-irtf-cfrg-bls-signature; Ethereum 2.0 specification.
- **Implementation:** Rust FFI library (SwiftMLS).

### 7. Groth16 Zero-Knowledge Proofs (Jens Groth, EUROCRYPT 2016)

- **Purpose:** Proving group membership without revealing the member's identity. Proofs are verified on-chain via a Stellar Soroban smart contract.
- **Key length:** Operates over BLS12-381 curve.
- **Standard:** Published academic standard (Groth, "On the Size of Pairing-based Non-interactive Arguments," EUROCRYPT 2016). Widely adopted in Ethereum (Zcash, Tornado Cash, Semaphore).
- **Implementation:** Rust FFI library (SwiftMLS) using arkworks.

### 8. SHA-256 (NIST FIPS 180-4)

- **Purpose:** Event ID computation (NIP-01), group topic derivation, commitment hashing.
- **Standard:** NIST FIPS 180-4; IETF RFC 6234.
- **Implementation:** Apple CryptoKit framework (`SHA256`).

### 9. SRTP (IETF RFC 3711)

- **Purpose:** Encryption of voice and video call media streams via WebRTC.
- **Standard:** IETF RFC 3711 (Secure Real-time Transport Protocol).
- **Implementation:** WebRTC framework (third-party, stasel/WebRTC).

## Encryption Not Provided by Apple's Operating System

The following encryption is implemented via the app's bundled Rust FFI library (SwiftMLS), not through Apple's CryptoKit or OS-level APIs:

| Algorithm | Library | Purpose |
|-----------|---------|---------|
| secp256k1 Schnorr | SwiftMLS (Rust FFI) | Nostr event signing |
| BLS12-381 | SwiftMLS (Rust FFI) | Membership key derivation |
| Poseidon hash | SwiftMLS (Rust FFI) | Merkle tree commitments |
| Groth16 ZK proofs | SwiftMLS (Rust FFI) | Zero-knowledge membership proofs |

All of these are standard, published algorithms with no proprietary modifications.

## Encryption Provided by Apple's Operating System

| Algorithm | Framework | Purpose |
|-----------|-----------|---------|
| AES-256-GCM | CryptoKit | Message, media, storage, notification encryption |
| HKDF-SHA256 | CryptoKit | Key derivation |
| X25519 ECDH | CryptoKit | Key agreement |
| SHA-256 | CryptoKit | Hashing |
| TLS 1.2/1.3 | URLSession | HTTPS connections to relayer and Blossom servers |
| File Protection Complete | Data Protection | At-rest file encryption |
| Keychain | Security framework | Secret key storage |

## Export Classification

- **ECCN:** 5D002 — Information Security software using encryption exceeding mass-market limits.
- **License Exception:** EAR 740.17(b)(1) — Mass market encryption software. The app is publicly available on the App Store, uses only standard encryption algorithms, and is designed for installation by the end user without substantial support.
- **Classification basis:** All algorithms are standard (NIST, IETF, BIP). Maximum symmetric key length is 256 bits (AES-256). Maximum asymmetric key length is 381 bits (BLS12-381). No proprietary algorithms. Available to the general public without restriction.

## BIS Self-Classification Filing

A self-classification report must be filed with BIS (Bureau of Industry and Security) and NSA annually per Supplement No. 5 to Part 742 of the EAR, using the following details:

| Field | Value |
|-------|-------|
| Product name | Onym |
| Model/version | 1.0 |
| Manufacturer | Onym |
| ECCN | 5D002 |
| Authorization type | MMKT (Mass Market) |
| Encryption algorithms | AES-256-GCM, HKDF-SHA256, X25519, secp256k1, Ed25519, BLS12-381, Groth16, SHA-256 |
| Key lengths | 256-bit symmetric (AES), 256-bit asymmetric (secp256k1, X25519, Ed25519), 381-bit (BLS12-381) |
| Open source | Yes (MIT license, https://github.com/rinat-enikeev/stellar-mls) |

### Filing addresses

- **BIS:** crypt@bis.gov
- **NSA:** enc@nsa.gov

Submit as a CSV per the Supplement No. 5 format. Filing is required before the app goes live on the App Store.
