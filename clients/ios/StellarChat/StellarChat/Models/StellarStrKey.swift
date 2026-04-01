import Foundation

/// Stellar StrKey encoding (SEP-0023).
/// Encodes Ed25519 public keys as G..., secret keys as S...
enum StellarStrKey {
    /// Version byte for Ed25519 public key (account ID): 6 << 3 = 48.
    private static let versionAccountID: UInt8 = 6 << 3

    /// Encode a 32-byte Ed25519 public key as a Stellar account ID (G...).
    static func encodeAccountID(_ publicKey: Data) -> String {
        precondition(publicKey.count == 32, "Ed25519 public key must be 32 bytes")
        var payload = Data([versionAccountID])
        payload.append(publicKey)
        let checksum = crc16XModem(payload)
        payload.append(checksum.littleEndianBytes)
        return base32Encode(payload)
    }

    // MARK: - CRC16-XModem

    private static func crc16XModem(_ data: Data) -> UInt16 {
        var crc: UInt16 = 0x0000
        for byte in data {
            crc ^= UInt16(byte) << 8
            for _ in 0..<8 {
                if crc & 0x8000 != 0 {
                    crc = (crc << 1) ^ 0x1021
                } else {
                    crc <<= 1
                }
            }
        }
        return crc & 0xFFFF
    }

    // MARK: - Base32 (RFC 4648, no padding)

    private static let base32Alphabet = Array("ABCDEFGHIJKLMNOPQRSTUVWXYZ234567")

    private static func base32Encode(_ data: Data) -> String {
        var result = ""
        result.reserveCapacity((data.count * 8 + 4) / 5)

        var buffer: UInt64 = 0
        var bitsLeft = 0

        for byte in data {
            buffer = (buffer << 8) | UInt64(byte)
            bitsLeft += 8
            while bitsLeft >= 5 {
                bitsLeft -= 5
                let index = Int((buffer >> bitsLeft) & 0x1F)
                result.append(base32Alphabet[index])
            }
        }

        if bitsLeft > 0 {
            let index = Int((buffer << (5 - bitsLeft)) & 0x1F)
            result.append(base32Alphabet[index])
        }

        return result
    }
}

private extension UInt16 {
    var littleEndianBytes: Data {
        var value = self.littleEndian
        return withUnsafeBytes(of: &value) { Data($0) }
    }
}
