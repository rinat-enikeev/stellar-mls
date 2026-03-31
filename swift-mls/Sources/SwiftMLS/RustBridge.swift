import CSEPMLSFFI
import Foundation

enum RustBridge {
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

    static func computeMerkleRoot(leafHashes: [Data], depth: Int) throws -> Data {
        let bytes = try flattenFieldElements(leafHashes)
        return try withSingleOutputBuffer { buffer, errorPointer in
            bytes.withUnsafeBytes { rawBytes in
                sep_compute_merkle_root(
                    rawBytes.bindMemory(to: UInt8.self).baseAddress,
                    bytes.count,
                    depth,
                    buffer,
                    errorPointer
                )
            }
        }
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
        leafHashes: [Data],
        secretKey: Data,
        proverIndex: Int,
        epoch: UInt64,
        salt: Data,
        depth: Int
    ) throws -> SEPMembershipProofBundle {
        if proverIndex < 0 {
            throw SEPError.invalidProverIndex(proverIndex)
        }

        let leafHashBytes = try flattenFieldElements(leafHashes)
        try validateFieldElement(secretKey)
        try validateSalt(salt)

        var proofBuffer = sep_byte_buffer_t(ptr: nil, len: 0)
        var commitmentBuffer = sep_byte_buffer_t(ptr: nil, len: 0)
        var rawError: UnsafeMutablePointer<CChar>?

        let success = provingKey.withUnsafeBytes { provingKeyBytes in
            leafHashBytes.withUnsafeBytes { leafBytes in
                secretKey.withUnsafeBytes { secretKeyBytes in
                    salt.withUnsafeBytes { saltBytes in
                        sep_generate_membership_proof(
                            provingKeyBytes.bindMemory(to: UInt8.self).baseAddress,
                            provingKey.count,
                            leafBytes.bindMemory(to: UInt8.self).baseAddress,
                            leafHashBytes.count,
                            secretKeyBytes.bindMemory(to: UInt8.self).baseAddress,
                            secretKey.count,
                            numericCast(proverIndex),
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

    private static func flattenFieldElements(_ elements: [Data]) throws -> Data {
        var flattened = Data(capacity: elements.count * 32)
        for (index, element) in elements.enumerated() {
            if element.count != 32 {
                throw SEPError.invalidLeafHashLength(index: index, actual: element.count)
            }
            flattened.append(element)
        }
        return flattened
    }
}
