DEV-ONLY DEMOCRACY VERIFYING KEYS.

These files were produced by `generate_democracy_vk_dev` using OsRng in a
single process. The Groth16 trapdoor is not securely destroyed and anyone
holding the proving key can forge Democracy update proofs.

DO NOT publish these files to a production contract. DO NOT commit them
to a release branch or bundle them with a mobile binary.

The real Democracy VK must come from the Phase 2 trusted-setup ceremony
documented in docs/democracy-circuit-ceremony.md.
