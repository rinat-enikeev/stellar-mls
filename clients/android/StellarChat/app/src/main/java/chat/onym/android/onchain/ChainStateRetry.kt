package chat.onym.android.onchain

import kotlinx.coroutines.delay

/**
 * Bounded retry for on-chain state reads when the RPC node is expected to
 * trail the network briefly. Used on the receive path: when a peer
 * broadcasts an update for epoch N, our RPC may not yet have indexed the
 * ledger close that committed epoch N to the contract. Polling once and
 * rejecting on mismatch would silently drop a legitimate update; polling
 * forever risks blocking on a genuinely-missing commitment.
 *
 * Returns the most recent entry seen — even when its epoch still trails
 * [expectedEpoch] after exhausting [maxAttempts] — so the caller can still
 * compare the stale entry and make a chain-authority decision. Re-throws
 * the last error only if every fetch attempt threw.
 */
suspend fun fetchOnChainStateAwaitingEpoch(
    fetch: suspend () -> SEPCommitmentEntry,
    expectedEpoch: Long,
    maxAttempts: Int = 4,
    initialDelayMs: Long = 250L
): SEPCommitmentEntry {
    require(maxAttempts >= 1) { "maxAttempts must be >= 1" }
    var delayMs = initialDelayMs
    var lastEntry: SEPCommitmentEntry? = null
    var lastError: Exception? = null
    for (attempt in 0 until maxAttempts) {
        try {
            val entry = fetch()
            lastEntry = entry
            if (entry.epoch >= expectedEpoch) return entry
        } catch (e: Exception) {
            lastError = e
        }
        if (attempt < maxAttempts - 1) {
            delay(delayMs)
            delayMs *= 2
        }
    }
    return lastEntry ?: throw (lastError
        ?: java.io.IOException("fetchOnChainState failed after $maxAttempts attempts"))
}
