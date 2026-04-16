package chat.onym.android

import androidx.test.ext.junit.runners.AndroidJUnit4
import com.stellarmls.mls.RustBackedNostrSigner
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Mirrors swift-mls' ephemeralNostrSignersAreUniqueAndProduceValidKeys and
 * rustBackedNostrSignerProducesValidSizedOutputs. Must run instrumented because
 * the signer bottoms out in the Rust JNI (`nostrDerivePublicKey` /
 * `nostrSignEventId`) shipped as .so files.
 */
@RunWith(AndroidJUnit4::class)
class EphemeralNostrSignerTests {

    @Test
    fun ephemeralSignersProduceDistinctPublicKeys() {
        val a = RustBackedNostrSigner.ephemeral()
        val b = RustBackedNostrSigner.ephemeral()

        val pubA = a.publicKey()
        val pubB = b.publicKey()

        assertEquals(32, pubA.size)
        assertEquals(32, pubB.size)
        assertFalse(
            "Two ephemeral signers must not produce the same pubkey",
            pubA.contentEquals(pubB)
        )
    }

    @Test
    fun ephemeralSignerSignsEventIdWithSchnorrSignature() {
        val signer = RustBackedNostrSigner.ephemeral()
        val eventId = ByteArray(32) { i -> i.toByte() }

        val signature = signer.signEventId(eventId)

        assertEquals(64, signature.size)
    }
}
