package com.stellarmls.chat.model

sealed class ChatError(message: String) : Exception(message) {
    object InvalidInviteCode : ChatError("Invalid invite code")
    object EncryptionFailed : ChatError("Failed to encrypt message")
    object DecryptionFailed : ChatError("Failed to decrypt message")
    object NoKey : ChatError("No signing key available")
    class VerificationFailed(reason: String) : ChatError("Verification failed: $reason")
    object RelayPublishFailed : ChatError("Failed to publish to any relay")
    object ContractNotConfigured : ChatError("Stellar contract not configured")
    class OnChainPublishFailed(reason: String) : ChatError("On-chain publish failed: $reason")
}
