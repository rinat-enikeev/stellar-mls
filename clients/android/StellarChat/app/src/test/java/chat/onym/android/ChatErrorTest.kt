package chat.onym.android

import chat.onym.android.model.ChatError
import org.junit.Assert.*
import org.junit.Test

/**
 * Pure JVM unit tests for ChatError sealed class.
 * Verifies all error types have the expected messages.
 */
class ChatErrorTest {

    @Test
    fun invalidInviteCode_message() {
        val error = ChatError.InvalidInviteCode
        assertTrue(error.message!!.contains("invite code", ignoreCase = true))
    }

    @Test
    fun encryptionFailed_message() {
        val error = ChatError.EncryptionFailed
        assertTrue(error.message!!.contains("encrypt", ignoreCase = true))
    }

    @Test
    fun decryptionFailed_message() {
        val error = ChatError.DecryptionFailed
        assertTrue(error.message!!.contains("decrypt", ignoreCase = true))
    }

    @Test
    fun noKey_message() {
        val error = ChatError.NoKey
        assertTrue(error.message!!.contains("key", ignoreCase = true))
    }

    @Test
    fun verificationFailed_includesReason() {
        val error = ChatError.VerificationFailed("signature invalid")
        assertTrue(error.message!!.contains("signature invalid"))
    }

    @Test
    fun relayPublishFailed_message() {
        val error = ChatError.RelayPublishFailed
        assertTrue(error.message!!.contains("relay", ignoreCase = true))
    }

    @Test
    fun contractNotConfigured_message() {
        val error = ChatError.ContractNotConfigured
        assertTrue(error.message!!.contains("not configured"))
    }

    @Test
    fun onChainPublishFailed_includesReason() {
        val error = ChatError.OnChainPublishFailed("timeout")
        assertTrue(error.message!!.contains("timeout"))
    }

    @Test
    fun allErrorsAreExceptions() {
        val errors: List<ChatError> = listOf(
            ChatError.InvalidInviteCode,
            ChatError.EncryptionFailed,
            ChatError.DecryptionFailed,
            ChatError.NoKey,
            ChatError.VerificationFailed("test"),
            ChatError.RelayPublishFailed,
            ChatError.ContractNotConfigured,
            ChatError.OnChainPublishFailed("test")
        )
        for (error in errors) {
            assertTrue(
                "${error::class.simpleName} must be an Exception",
                error is Exception
            )
            assertNotNull(
                "${error::class.simpleName} must have a non-null message",
                error.message
            )
        }
    }
}
