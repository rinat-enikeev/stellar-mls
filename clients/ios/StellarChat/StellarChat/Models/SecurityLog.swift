import Foundation
import os.log

/// M-19: Structured security event logging for audit-relevant events.
/// Logs to the unified OS log system under the "Security" category.
/// Never logs sensitive data (keys, plaintexts, proofs).
enum SecurityLog {
    private static let logger = Logger(subsystem: "com.stellarmls.chat", category: "Security")

    /// A decryption attempt failed (wrong key, corrupted data, etc.).
    static func decryptionFailed(context: String) {
        logger.warning("Decryption failed: \(context, privacy: .public)")
    }

    /// A message was rejected because the sender BLS pubkey is not in the member list.
    static func nonMemberMessageRejected(groupID: String) {
        logger.warning("Non-member message rejected for group \(groupID.prefix(8), privacy: .public)")
    }

    /// An invalid key attestation was received (bad structure or signature).
    static func invalidAttestation(reason: String) {
        logger.warning("Invalid attestation: \(reason, privacy: .public)")
    }

    /// A proof verification failed on-chain or locally.
    static func proofVerificationFailed(groupID: String, reason: String) {
        logger.error("Proof verification failed for group \(groupID.prefix(8), privacy: .public): \(reason, privacy: .public)")
    }

    /// An on-chain operation failed (contract call error).
    static func onChainOperationFailed(operation: String, reason: String) {
        logger.error("On-chain \(operation, privacy: .public) failed: \(reason, privacy: .public)")
    }

    /// A state update was rejected (epoch mismatch, invalid attestation, etc.).
    static func stateUpdateRejected(groupID: String, reason: String) {
        logger.warning("State update rejected for group \(groupID.prefix(8), privacy: .public): \(reason, privacy: .public)")
    }

    /// A relay connection event (connect, disconnect, failure).
    static func relayEvent(_ event: String, url: String) {
        logger.info("Relay \(event, privacy: .public): \(url, privacy: .public)")
    }

    /// A duplicate protocol message was detected and dropped.
    static func duplicateProtocolMessage(eventID: String) {
        logger.info("Duplicate protocol message dropped: \(eventID.prefix(8), privacy: .public)")
    }

    /// A salt request was rate-limited.
    static func saltRequestRateLimited(groupID: String, epoch: UInt64) {
        logger.info("Salt request rate-limited for group \(groupID.prefix(8), privacy: .public) epoch \(epoch, privacy: .public)")
    }
}
