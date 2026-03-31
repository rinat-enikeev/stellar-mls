import CryptoKit
import Foundation

public enum SEPInvitationSender {
    public static func sendInvitation(
        bootstrap: SEPInvitationBootstrap,
        recipientPublicKey: Data,
        relayURLs: [URL],
        cryptoProvider: any SEPInvitationCryptoProvider,
        signer: any SEPNostrEventSigner,
        relayTransport: any SEPNostrRelayTransport = URLSessionSEPNostrRelayTransport(),
        options: SEPInvitationSendOptions = SEPInvitationSendOptions()
    ) async throws -> SEPInvitationSendResult {
        guard !relayURLs.isEmpty else {
            throw SEPError.emptyRelayList
        }

        let plaintext = try JSONEncoder().encode(bootstrap)
        let hiddenInboxTag = try cryptoProvider.hiddenInboxTag(recipientPublicKey: recipientPublicKey)
        let sealedEnvelope = try cryptoProvider.sealInvitation(plaintext, recipientPublicKey: recipientPublicKey)
        let envelopeData = try JSONEncoder().encode(sealedEnvelope)
        let content = envelopeData.base64EncodedString()

        let pubkey = try signer.publicKey()
        guard pubkey.count == 32 else {
            throw SEPError.invalidNostrPublicKeyLength(actual: pubkey.count)
        }

        let createdAt = options.createdAt ?? Int64(Date().timeIntervalSince1970)
        let tags = [["sep_inbox", hiddenInboxTag], ["sep_version", "1"]] + options.additionalTags
        let pubkeyHex = pubkey.hexString()
        let eventID = try eventID(pubkeyHex: pubkeyHex, createdAt: createdAt, kind: options.kind, tags: tags, content: content)

        let signature = try signer.signEventID(eventID)
        guard signature.count == 64 else {
            throw SEPError.invalidNostrSignatureLength(actual: signature.count)
        }

        let event = SEPNostrEvent(
            id: eventID.hexString(),
            pubkey: pubkeyHex,
            createdAt: createdAt,
            kind: options.kind,
            tags: tags,
            content: content,
            sig: signature.hexString()
        )

        let relayResults = await withTaskGroup(of: SEPNostrRelaySendResult.self) { group in
            for relayURL in relayURLs {
                group.addTask {
                    do {
                        return try await relayTransport.publish(event: event, to: relayURL)
                    } catch {
                        return SEPNostrRelaySendResult(
                            relayURL: relayURL,
                            accepted: false,
                            message: error.localizedDescription
                        )
                    }
                }
            }

            var results: [SEPNostrRelaySendResult] = []
            results.reserveCapacity(relayURLs.count)
            for await result in group {
                results.append(result)
            }
            return results.sorted { $0.relayURL.absoluteString < $1.relayURL.absoluteString }
        }

        return SEPInvitationSendResult(event: event, relayResults: relayResults)
    }

    static func eventID(
        pubkeyHex: String,
        createdAt: Int64,
        kind: Int,
        tags: [[String]],
        content: String
    ) throws -> Data {
        let object: [Any] = [0, pubkeyHex, createdAt, kind, tags, content]
        let serialized = try JSONSerialization.data(withJSONObject: object, options: [])
        return Data(SHA256.hash(data: serialized))
    }
}

private extension Data {
    func hexString() -> String {
        map { String(format: "%02x", $0) }.joined()
    }
}
