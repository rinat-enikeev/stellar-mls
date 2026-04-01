package com.stellarmls.chat.onchain

/** Result of verifying local group state against on-chain commitment. */
sealed class OnChainVerificationResult {
    object NotPublished : OnChainVerificationResult()
    object Verified : OnChainVerificationResult()
    data class EpochMismatch(val local: Long, val onChain: Long) : OnChainVerificationResult()
    object CommitmentMismatch : OnChainVerificationResult()
    object Inactive : OnChainVerificationResult()
    data class Error(val message: String) : OnChainVerificationResult()

    val displayText: String
        get() = when (this) {
            is NotPublished -> "Not published on-chain"
            is Verified -> "Verified on-chain"
            is EpochMismatch -> "Epoch mismatch: local $local, on-chain $onChain"
            is CommitmentMismatch -> "Commitment mismatch"
            is Inactive -> "Group deactivated on-chain"
            is Error -> "Error: $message"
        }
}
