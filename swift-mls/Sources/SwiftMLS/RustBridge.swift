import CSEPMLSFFI
import Foundation

enum RustBridge {
    static func deriveNostrPublicKey(secretKey: Data) throws -> Data {
        try withSingleOutputBuffer { buffer, errorPointer in
            secretKey.withUnsafeBytes { rawBytes in
                sep_nostr_derive_public_key(
                    rawBytes.bindMemory(to: UInt8.self).baseAddress,
                    secretKey.count,
                    buffer,
                    errorPointer
                )
            }
        }
    }

    static func signNostrEventID(secretKey: Data, eventID: Data) throws -> Data {
        try withSingleOutputBuffer { buffer, errorPointer in
            secretKey.withUnsafeBytes { secretKeyBytes in
                eventID.withUnsafeBytes { eventIDBytes in
                    sep_nostr_sign_event_id(
                        secretKeyBytes.bindMemory(to: UInt8.self).baseAddress,
                        secretKey.count,
                        eventIDBytes.bindMemory(to: UInt8.self).baseAddress,
                        eventID.count,
                        buffer,
                        errorPointer
                    )
                }
            }
        }
    }

    static func verifyNostrEventSignature(publicKey: Data, eventID: Data, signature: Data) throws -> Bool {
        var rawError: UnsafeMutablePointer<CChar>?
        let success = publicKey.withUnsafeBytes { pubKeyBytes in
            eventID.withUnsafeBytes { eventIDBytes in
                signature.withUnsafeBytes { sigBytes in
                    sep_nostr_verify_event_signature(
                        pubKeyBytes.bindMemory(to: UInt8.self).baseAddress,
                        publicKey.count,
                        eventIDBytes.bindMemory(to: UInt8.self).baseAddress,
                        eventID.count,
                        sigBytes.bindMemory(to: UInt8.self).baseAddress,
                        signature.count,
                        &rawError
                    )
                }
            }
        }
        if !success {
            if let rawError {
                let message = String(cString: rawError)
                sep_string_free(rawError)
                throw SEPError.ffiFailure(message)
            }
            return false
        }
        return true
    }

    static func computeLeafHash(secretKey: Data) throws -> Data {
        try withSingleOutputBuffer { buffer, errorPointer in
            secretKey.withUnsafeBytes { rawBytes in
                sep_compute_leaf_hash(
                    rawBytes.bindMemory(to: UInt8.self).baseAddress,
                    secretKey.count,
                    buffer,
                    errorPointer
                )
            }
        }
    }

    static func computePublicKey(secretKey: Data) throws -> Data {
        try withSingleOutputBuffer { buffer, errorPointer in
            secretKey.withUnsafeBytes { rawBytes in
                sep_compute_public_key(
                    rawBytes.bindMemory(to: UInt8.self).baseAddress,
                    secretKey.count,
                    buffer,
                    errorPointer
                )
            }
        }
    }

    static func computeMerkleRoot(members: [SEPGroupMemberLeaf], depth: Int) throws -> Data {
        let flattenedMembers = try flattenMembers(members)
        return try withSingleOutputBuffer { buffer, errorPointer in
            flattenedMembers.publicKeys.withUnsafeBytes { publicKeyBytes in
                flattenedMembers.leafHashes.withUnsafeBytes { leafHashBytes in
                    sep_compute_merkle_root(
                        publicKeyBytes.bindMemory(to: UInt8.self).baseAddress,
                        flattenedMembers.publicKeys.count,
                        leafHashBytes.bindMemory(to: UInt8.self).baseAddress,
                        flattenedMembers.leafHashes.count,
                        depth,
                        buffer,
                        errorPointer
                    )
                }
            }
        }
    }

    static func proofToContractFormat(compressedProof: Data) throws -> SEPContractProofComponents {
        var proofABuffer = sep_byte_buffer_t(ptr: nil, len: 0)
        var proofBBuffer = sep_byte_buffer_t(ptr: nil, len: 0)
        var proofCBuffer = sep_byte_buffer_t(ptr: nil, len: 0)
        var rawError: UnsafeMutablePointer<CChar>?

        let success = compressedProof.withUnsafeBytes { proofBytes in
            sep_proof_to_contract_format(
                proofBytes.bindMemory(to: UInt8.self).baseAddress,
                compressedProof.count,
                &proofABuffer,
                &proofBBuffer,
                &proofCBuffer,
                &rawError
            )
        }

        if !success {
            if proofABuffer.ptr != nil { sep_byte_buffer_free(proofABuffer) }
            if proofBBuffer.ptr != nil { sep_byte_buffer_free(proofBBuffer) }
            if proofCBuffer.ptr != nil { sep_byte_buffer_free(proofCBuffer) }
            throw consumeRustError(rawError)
        }

        return SEPContractProofComponents(
            proofA: consumeBuffer(proofABuffer),
            proofB: consumeBuffer(proofBBuffer),
            proofC: consumeBuffer(proofCBuffer)
        )
    }

    static func computeSHA256Commitment(poseidonRoot: Data, epoch: UInt64, salt: Data) throws -> Data {
        try validateFieldElement(poseidonRoot)
        try validateSalt(salt)

        return try withSingleOutputBuffer { buffer, errorPointer in
            poseidonRoot.withUnsafeBytes { rootBytes in
                salt.withUnsafeBytes { saltBytes in
                    sep_compute_sha256_commitment(
                        rootBytes.bindMemory(to: UInt8.self).baseAddress,
                        poseidonRoot.count,
                        epoch,
                        saltBytes.bindMemory(to: UInt8.self).baseAddress,
                        salt.count,
                        buffer,
                        errorPointer
                    )
                }
            }
        }
    }

    static func computePoseidonCommitment(poseidonRoot: Data, epoch: UInt64, salt: Data) throws -> Data {
        try validateFieldElement(poseidonRoot)
        try validateSalt(salt)

        return try withSingleOutputBuffer { buffer, errorPointer in
            poseidonRoot.withUnsafeBytes { rootBytes in
                salt.withUnsafeBytes { saltBytes in
                    sep_compute_poseidon_commitment(
                        rootBytes.bindMemory(to: UInt8.self).baseAddress,
                        poseidonRoot.count,
                        epoch,
                        saltBytes.bindMemory(to: UInt8.self).baseAddress,
                        salt.count,
                        buffer,
                        errorPointer
                    )
                }
            }
        }
    }

    static func generateTestingProvingKey(depth: Int, seed: UInt64) throws -> Data {
        try withSingleOutputBuffer { buffer, errorPointer in
            sep_generate_testing_proving_key(depth, seed, buffer, errorPointer)
        }
    }

    static func generateMembershipProof(
        provingKey: Data,
        members: [SEPGroupMemberLeaf],
        secretKey: Data,
        epoch: UInt64,
        salt: Data,
        depth: Int
    ) throws -> SEPMembershipProofBundle {
        let flattenedMembers = try flattenMembers(members)
        try validateFieldElement(secretKey)
        try validateSalt(salt)

        var proofBuffer = sep_byte_buffer_t(ptr: nil, len: 0)
        var commitmentBuffer = sep_byte_buffer_t(ptr: nil, len: 0)
        var rawError: UnsafeMutablePointer<CChar>?

        let success = provingKey.withUnsafeBytes { provingKeyBytes in
            flattenedMembers.publicKeys.withUnsafeBytes { publicKeyBytes in
                flattenedMembers.leafHashes.withUnsafeBytes { leafBytes in
                    secretKey.withUnsafeBytes { secretKeyBytes in
                        salt.withUnsafeBytes { saltBytes in
                            sep_generate_membership_proof(
                                provingKeyBytes.bindMemory(to: UInt8.self).baseAddress,
                                provingKey.count,
                                publicKeyBytes.bindMemory(to: UInt8.self).baseAddress,
                                flattenedMembers.publicKeys.count,
                                leafBytes.bindMemory(to: UInt8.self).baseAddress,
                                flattenedMembers.leafHashes.count,
                                secretKeyBytes.bindMemory(to: UInt8.self).baseAddress,
                                secretKey.count,
                                epoch,
                                saltBytes.bindMemory(to: UInt8.self).baseAddress,
                                salt.count,
                                depth,
                                &proofBuffer,
                                &commitmentBuffer,
                                &rawError
                            )
                        }
                    }
                }
            }
        }

        if !success {
            if proofBuffer.ptr != nil {
                sep_byte_buffer_free(proofBuffer)
            }
            if commitmentBuffer.ptr != nil {
                sep_byte_buffer_free(commitmentBuffer)
            }
            throw consumeRustError(rawError)
        }

        let proof = consumeBuffer(proofBuffer)
        let commitment = consumeBuffer(commitmentBuffer)
        return SEPMembershipProofBundle(
            proof: proof,
            publicInputs: SEPPublicInputs(commitment: commitment, epoch: epoch)
        )
    }

    static func generateTestingUpdateProvingKey(depth: Int, seed: UInt64) throws -> Data {
        try withSingleOutputBuffer { buffer, errorPointer in
            sep_generate_testing_update_proving_key(depth, seed, buffer, errorPointer)
        }
    }

    static func generateUpdateProof(
        provingKey: Data,
        oldMembers: [SEPGroupMemberLeaf],
        newMembers: [SEPGroupMemberLeaf],
        secretKey: Data,
        epochOld: UInt64,
        saltOld: Data,
        saltNew: Data,
        depth: Int
    ) throws -> SEPUpdateProofBundle {
        let flattenedOld = try flattenMembers(oldMembers)
        let flattenedNew = try flattenMembers(newMembers)
        try validateFieldElement(secretKey)
        try validateSalt(saltOld)
        try validateSalt(saltNew)

        let provingKeyArray = [UInt8](provingKey)
        let oldPublicKeysArray = [UInt8](flattenedOld.publicKeys)
        let oldLeafHashesArray = [UInt8](flattenedOld.leafHashes)
        let newPublicKeysArray = [UInt8](flattenedNew.publicKeys)
        let newLeafHashesArray = [UInt8](flattenedNew.leafHashes)
        let secretKeyArray = [UInt8](secretKey)
        let saltOldArray = [UInt8](saltOld)
        let saltNewArray = [UInt8](saltNew)

        var proofBuffer = sep_byte_buffer_t(ptr: nil, len: 0)
        var publicInputsBuffer = sep_byte_buffer_t(ptr: nil, len: 0)
        var rawError: UnsafeMutablePointer<CChar>?

        let success = sep_generate_update_proof(
            provingKeyArray,
            provingKeyArray.count,
            oldPublicKeysArray,
            oldPublicKeysArray.count,
            oldLeafHashesArray,
            oldLeafHashesArray.count,
            newPublicKeysArray,
            newPublicKeysArray.count,
            newLeafHashesArray,
            newLeafHashesArray.count,
            secretKeyArray,
            secretKeyArray.count,
            epochOld,
            saltOldArray,
            saltOldArray.count,
            saltNewArray,
            saltNewArray.count,
            depth,
            &proofBuffer,
            &publicInputsBuffer,
            &rawError
        )

        if !success {
            if proofBuffer.ptr != nil { sep_byte_buffer_free(proofBuffer) }
            if publicInputsBuffer.ptr != nil { sep_byte_buffer_free(publicInputsBuffer) }
            throw consumeRustError(rawError)
        }

        let proof = consumeBuffer(proofBuffer)
        let publicInputsWire = consumeBuffer(publicInputsBuffer)
        let parsed = try parseUpdatePublicInputs(wireFormat: publicInputsWire)
        return SEPUpdateProofBundle(proof: proof, publicInputs: parsed)
    }

    static func parseUpdatePublicInputs(wireFormat: Data) throws -> SEPUpdatePublicInputs {
        var cOldBuffer = sep_byte_buffer_t(ptr: nil, len: 0)
        var epochOldBuffer = sep_byte_buffer_t(ptr: nil, len: 0)
        var cNewBuffer = sep_byte_buffer_t(ptr: nil, len: 0)
        var rawError: UnsafeMutablePointer<CChar>?

        let success = wireFormat.withUnsafeBytes { wireBytes in
            sep_parse_update_public_inputs(
                wireBytes.bindMemory(to: UInt8.self).baseAddress,
                wireFormat.count,
                &cOldBuffer,
                &epochOldBuffer,
                &cNewBuffer,
                &rawError
            )
        }

        if !success {
            if cOldBuffer.ptr != nil { sep_byte_buffer_free(cOldBuffer) }
            if epochOldBuffer.ptr != nil { sep_byte_buffer_free(epochOldBuffer) }
            if cNewBuffer.ptr != nil { sep_byte_buffer_free(cNewBuffer) }
            throw consumeRustError(rawError)
        }

        let cOld = consumeBuffer(cOldBuffer)
        let epochOldBytes = consumeBuffer(epochOldBuffer)
        let cNew = consumeBuffer(cNewBuffer)

        guard epochOldBytes.count == 8 else {
            throw SEPError.ffiFailure("sep_parse_update_public_inputs returned \(epochOldBytes.count)-byte epoch_old (expected 8)")
        }
        let epochOld = epochOldBytes.reduce(UInt64(0)) { ($0 << 8) | UInt64($1) }

        return SEPUpdatePublicInputs(cOld: cOld, epochOld: epochOld, cNew: cNew)
    }

    private static func withSingleOutputBuffer(
        _ body: (_ buffer: UnsafeMutablePointer<sep_byte_buffer_t>, _ error: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>) -> Bool
    ) throws -> Data {
        var buffer = sep_byte_buffer_t(ptr: nil, len: 0)
        var rawError: UnsafeMutablePointer<CChar>?

        let success = body(&buffer, &rawError)
        if !success {
            if buffer.ptr != nil {
                sep_byte_buffer_free(buffer)
            }
            throw consumeRustError(rawError)
        }

        return consumeBuffer(buffer)
    }

    private static func consumeBuffer(_ buffer: sep_byte_buffer_t) -> Data {
        guard let pointer = buffer.ptr else {
            return Data()
        }

        let data = Data(bytes: pointer, count: buffer.len)
        sep_byte_buffer_free(buffer)
        return data
    }

    private static func consumeRustError(_ rawError: UnsafeMutablePointer<CChar>?) -> SEPError {
        guard let rawError else {
            return .ffiFailure("Rust FFI call failed without an error message")
        }

        let message = String(cString: rawError)
        sep_string_free(rawError)
        return .ffiFailure(message)
    }

    private static func validateFieldElement(_ data: Data) throws {
        if data.count != 32 {
            throw SEPError.invalidFieldByteLength(expected: 32, actual: data.count)
        }
    }

    private static func validateSalt(_ salt: Data) throws {
        if salt.count != 32 {
            throw SEPError.invalidSaltLength(actual: salt.count)
        }
    }

    private static func flattenMembers(_ members: [SEPGroupMemberLeaf]) throws -> (publicKeys: Data, leafHashes: Data) {
        var flattenedPublicKeys = Data(capacity: members.count * 48)
        var flattenedLeafHashes = Data(capacity: members.count * 32)

        for (index, member) in members.enumerated() {
            if member.publicKeyCompressed.count != 48 {
                throw SEPError.invalidPublicKeyLength(index: index, actual: member.publicKeyCompressed.count)
            }
            if member.leafHash.count != 32 {
                throw SEPError.invalidLeafHashLength(index: index, actual: member.leafHash.count)
            }
            flattenedPublicKeys.append(member.publicKeyCompressed)
            flattenedLeafHashes.append(member.leafHash)
        }

        return (publicKeys: flattenedPublicKeys, leafHashes: flattenedLeafHashes)
    }
}
