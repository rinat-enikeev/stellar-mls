import Foundation

/// Minimal BIP-173 bech32 decoder, sufficient for Nostr NIP-19 `npub1…`
/// strings. Only the decode direction is needed — the GUI accepts either
/// npub1 or hex and normalises to hex before handing off to ceremony_tool.
enum Bech32 {
    struct DecodeError: LocalizedError {
        let message: String
        var errorDescription: String? { message }
    }

    private static let charset = Array("qpzry9x8gf2tvdw0s3jn54khce6mua7l")

    private static func charsetIndex(_ c: Character) -> Int? {
        charset.firstIndex(of: c)
    }

    private static func polymod(_ values: [UInt32]) -> UInt32 {
        let gen: [UInt32] = [0x3b6a57b2, 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3]
        var chk: UInt32 = 1
        for v in values {
            let top = chk >> 25
            chk = ((chk & 0x1ffffff) << 5) ^ v
            for i in 0..<5 where (top >> i) & 1 == 1 {
                chk ^= gen[i]
            }
        }
        return chk
    }

    private static func hrpExpand(_ hrp: String) -> [UInt32] {
        let bytes = Array(hrp.unicodeScalars).map { UInt32($0.value) }
        var out: [UInt32] = bytes.map { $0 >> 5 }
        out.append(0)
        out.append(contentsOf: bytes.map { $0 & 31 })
        return out
    }

    private static func verifyChecksum(hrp: String, data: [UInt32]) -> Bool {
        polymod(hrpExpand(hrp) + data) == 1
    }

    /// Decode a bech32 string into `(hrp, 5-bit data)`.
    /// Throws on invalid characters, missing separator, bad checksum, or
    /// mixed-case input.
    static func decode(_ input: String) throws -> (hrp: String, data: [UInt32]) {
        let lower = input.lowercased()
        let upper = input.uppercased()
        if lower != input && upper != input {
            throw DecodeError(message: "bech32 string has mixed case")
        }
        let s = lower
        guard let sepIdx = s.lastIndex(of: "1") else {
            throw DecodeError(message: "bech32 separator '1' not found")
        }
        let hrp = String(s[..<sepIdx])
        let dataPart = s[s.index(after: sepIdx)...]
        if hrp.isEmpty { throw DecodeError(message: "bech32 human-readable part is empty") }
        if dataPart.count < 6 { throw DecodeError(message: "bech32 payload too short for checksum") }

        var data: [UInt32] = []
        data.reserveCapacity(dataPart.count)
        for c in dataPart {
            guard let idx = charsetIndex(c) else {
                throw DecodeError(message: "invalid bech32 character '\(c)'")
            }
            data.append(UInt32(idx))
        }
        guard verifyChecksum(hrp: hrp, data: data) else {
            throw DecodeError(message: "bech32 checksum mismatch")
        }
        // Strip the 6-symbol checksum before returning the payload.
        return (hrp, Array(data.dropLast(6)))
    }

    /// Repack 5-bit groups into 8-bit bytes. Rejects payloads whose
    /// leftover high bits aren't zero.
    static func convertBits(_ data: [UInt32], from fromBits: Int, to toBits: Int, pad: Bool)
        throws -> [UInt8]
    {
        var acc: UInt32 = 0
        var bits: Int = 0
        var out: [UInt8] = []
        let maxv: UInt32 = (1 << toBits) - 1
        for v in data {
            if v >> fromBits != 0 { throw DecodeError(message: "bech32 value out of range") }
            acc = (acc << fromBits) | v
            bits += fromBits
            while bits >= toBits {
                bits -= toBits
                out.append(UInt8((acc >> bits) & maxv))
            }
        }
        if pad {
            if bits > 0 { out.append(UInt8((acc << (toBits - bits)) & maxv)) }
        } else if bits >= fromBits || ((acc << (toBits - bits)) & maxv) != 0 {
            throw DecodeError(message: "invalid bech32 padding")
        }
        return out
    }
}

/// Parse either a 64-char lowercase/uppercase hex pubkey or an
/// `npub1…` bech32 string and return a 64-char lowercase hex string.
/// Trims surrounding whitespace. Throws `Bech32.DecodeError` with a
/// human-readable message on failure.
func normalizePubkey(_ raw: String) throws -> String {
    let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)

    // Already hex?
    if trimmed.count == 64, trimmed.allSatisfy({ $0.isHexDigit }) {
        return trimmed.lowercased()
    }

    // Try npub1.
    let lower = trimmed.lowercased()
    if lower.hasPrefix("npub1") {
        let (hrp, data) = try Bech32.decode(trimmed)
        guard hrp == "npub" else {
            throw Bech32.DecodeError(message: "expected npub1 prefix, got \(hrp)1…")
        }
        let bytes = try Bech32.convertBits(data, from: 5, to: 8, pad: false)
        guard bytes.count == 32 else {
            throw Bech32.DecodeError(message: "npub1 payload must be 32 bytes, got \(bytes.count)")
        }
        return bytes.map { String(format: "%02x", $0) }.joined()
    }

    throw Bech32.DecodeError(
        message: "paste a 64-char hex pubkey or an npub1… string from your Nostr client"
    )
}

private extension Character {
    var isHexDigit: Bool {
        switch self {
        case "0"..."9", "a"..."f", "A"..."F": return true
        default: return false
        }
    }
}
