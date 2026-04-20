package chat.onym.android

import chat.onym.android.onchain.SEPCommitmentEntry
import chat.onym.android.onchain.fetchOnChainStateAwaitingEpoch
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import java.io.IOException

/**
 * Regression tests for the chain-first receive-path retry helper. The
 * receive path rejects an update when the on-chain commitment at the
 * update's epoch disagrees with what the envelope claims. Without bounded
 * retry, an RPC node that trails the network by one ledger close would
 * cause a legitimate update to be silently dropped. These tests lock in
 * the retry semantics so a future refactor can't regress to single-shot
 * polling.
 */
class ChainStateRetryTest {

    private fun entry(epoch: Long, active: Boolean = true): SEPCommitmentEntry =
        SEPCommitmentEntry(
            commitment = ByteArray(32) { it.toByte() },
            epoch = epoch,
            timestamp = 0L,
            tier = 0,
            active = active
        )

    @Test
    fun returnsImmediately_whenChainAlreadyAtExpectedEpoch() = runTest {
        var calls = 0
        val result = fetchOnChainStateAwaitingEpoch(
            fetch = { calls++; entry(epoch = 5) },
            expectedEpoch = 5,
            maxAttempts = 4,
            initialDelayMs = 0L
        )
        assertEquals(5L, result.epoch)
        assertEquals("must not retry when first fetch already satisfies expected epoch", 1, calls)
    }

    @Test
    fun retriesUntilEpochAdvances_simulatingRpcLag() = runTest {
        val epochsSeen = mutableListOf<Long>()
        val epochsToReturn = listOf(3L, 3L, 5L)
        var attempt = 0
        val result = fetchOnChainStateAwaitingEpoch(
            fetch = {
                val e = epochsToReturn[attempt++]
                epochsSeen.add(e)
                entry(epoch = e)
            },
            expectedEpoch = 5,
            maxAttempts = 4,
            initialDelayMs = 0L
        )
        assertEquals(5L, result.epoch)
        assertEquals(listOf(3L, 3L, 5L), epochsSeen)
    }

    @Test
    fun returnsLastStaleEntry_whenChainNeverAdvancesWithinBudget() = runTest {
        var calls = 0
        val result = fetchOnChainStateAwaitingEpoch(
            fetch = { calls++; entry(epoch = 3) },
            expectedEpoch = 5,
            maxAttempts = 3,
            initialDelayMs = 0L
        )
        // Helper returns the last seen entry so the caller can still decide
        // based on commitment match — but the trailing epoch will cause the
        // caller's chain-match check to fail, dropping the update.
        assertEquals(3L, result.epoch)
        assertEquals(3, calls)
    }

    @Test
    fun rethrowsWhenEveryAttemptFails() = runTest {
        var calls = 0
        try {
            fetchOnChainStateAwaitingEpoch(
                fetch = { calls++; throw IOException("rpc down") },
                expectedEpoch = 5,
                maxAttempts = 3,
                initialDelayMs = 0L
            )
            fail("expected IOException after all attempts fail")
        } catch (e: IOException) {
            assertEquals("rpc down", e.message)
        }
        assertEquals(3, calls)
    }

    @Test
    fun returnsSuccessfulEntry_evenAfterTransientErrors() = runTest {
        var attempt = 0
        val result = fetchOnChainStateAwaitingEpoch(
            fetch = {
                attempt++
                if (attempt == 1) throw IOException("transient")
                entry(epoch = 7)
            },
            expectedEpoch = 7,
            maxAttempts = 4,
            initialDelayMs = 0L
        )
        assertEquals(7L, result.epoch)
        assertEquals(2, attempt)
    }

    @Test
    fun rejectsInvalidMaxAttempts() = runTest {
        try {
            fetchOnChainStateAwaitingEpoch(
                fetch = { entry(epoch = 5) },
                expectedEpoch = 5,
                maxAttempts = 0,
                initialDelayMs = 0L
            )
            fail("expected IllegalArgumentException for maxAttempts=0")
        } catch (e: IllegalArgumentException) {
            assertTrue(e.message!!.contains("maxAttempts"))
        }
    }

    /**
     * Production default is 4 attempts with initialDelayMs=250, so the
     * expected sleep schedule between attempts is 250→500→1000 ms. Under
     * [runTest] the `delay` calls advance virtual time deterministically,
     * so we can assert the total elapsed virtual time without wall-clocking
     * 1.75 s. Locks in exponential backoff in case someone tweaks the
     * multiplier.
     */
    @Test
    @kotlinx.coroutines.ExperimentalCoroutinesApi
    fun appliesExponentialBackoff_betweenAttempts() = runTest {
        var attempt = 0
        val startVirtual = testScheduler.currentTime
        fetchOnChainStateAwaitingEpoch(
            // Force three retries: return stale epoch on attempts 1..3, then
            // the expected epoch on attempt 4 so we exercise all three sleeps.
            fetch = {
                attempt++
                entry(epoch = if (attempt == 4) 5L else 3L)
            },
            expectedEpoch = 5,
            maxAttempts = 4,
            initialDelayMs = 250L
        )
        val elapsed = testScheduler.currentTime - startVirtual
        // 250 + 500 + 1000 = 1750 ms of virtual time spent sleeping.
        assertEquals(1750L, elapsed)
        assertEquals(4, attempt)
    }
}
