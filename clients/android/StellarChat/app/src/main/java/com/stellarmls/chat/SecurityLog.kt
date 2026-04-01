package com.stellarmls.chat

import android.util.Log

/**
 * M-19: Structured security event logging for audit-relevant events.
 * Uses Android's Log framework with the "StellarSecurity" tag.
 * Never logs sensitive data (keys, plaintexts, proofs).
 * All logging is DEBUG-only — zero console output in release builds.
 */
object SecurityLog {
    private const val TAG = "StellarSecurity"

    /** A decryption attempt failed. */
    fun decryptionFailed(context: String) {
        if (BuildConfig.DEBUG) Log.w(TAG, "Decryption failed: $context")
    }

    /** A message was rejected because the sender BLS pubkey is not in the member list. */
    fun nonMemberMessageRejected(groupID: String) {
        if (BuildConfig.DEBUG) Log.w(TAG, "Non-member message rejected for group ${groupID.take(8)}")
    }

    /** An invalid key attestation was received. */
    fun invalidAttestation(reason: String) {
        if (BuildConfig.DEBUG) Log.w(TAG, "Invalid attestation: $reason")
    }

    /** A proof verification failed. */
    fun proofVerificationFailed(groupID: String, reason: String) {
        if (BuildConfig.DEBUG) Log.e(TAG, "Proof verification failed for group ${groupID.take(8)}: $reason")
    }

    /** An on-chain operation failed. */
    fun onChainOperationFailed(operation: String, reason: String) {
        if (BuildConfig.DEBUG) Log.e(TAG, "On-chain $operation failed: $reason")
    }

    /** A state update was rejected. */
    fun stateUpdateRejected(groupID: String, reason: String) {
        if (BuildConfig.DEBUG) Log.w(TAG, "State update rejected for group ${groupID.take(8)}: $reason")
    }

    /** A relay connection event. */
    fun relayEvent(event: String, url: String) {
        if (BuildConfig.DEBUG) Log.i(TAG, "Relay $event: $url")
    }

    /** A duplicate protocol message was detected and dropped. */
    fun duplicateProtocolMessage(eventID: String) {
        if (BuildConfig.DEBUG) Log.i(TAG, "Duplicate protocol message dropped: ${eventID.take(8)}")
    }

    /** A salt request was rate-limited. */
    fun saltRequestRateLimited(groupID: String, epoch: Long) {
        if (BuildConfig.DEBUG) Log.i(TAG, "Salt request rate-limited for group ${groupID.take(8)} epoch $epoch")
    }
}
