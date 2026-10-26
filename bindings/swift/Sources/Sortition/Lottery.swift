import CSortition

/// Fair, repeatable and verifiable random lotteries.
///
/// Use the candidate functions (``drawItems(_:k:seed:)``, ``drawStages(_:_:seed:)``, ``ItemSession``)
/// when the winners must not depend on the order of your list: each candidate gets an ID from its bytes,
/// and the draw works on the IDs in sorted order. ``draw(n:k:seed:)``, ``drawText(_:k:seed:)`` and
/// ``drawData(_:_:k:path:seed:)`` return positions in the list as given.
///
/// Pass a seed (``seedFromText(_:)``) for a repeatable result; without one the draw uses the operating
/// system's randomness. Everything throws ``SortitionError`` for invalid input.
public enum Lottery {
    /// The version of the native library.
    public static var version: String { String(cString: sortition_version()) }

    private static func seedInfo(_ text: [UInt8]) throws -> (seed: UInt64, digest: [UInt8]) {
        var seed: UInt64 = 0
        var digest = [UInt8](repeating: 0, count: 32)
        try withBytes(text) { t, n in
            try check(sortition_seed_from_text(t, n, &seed, &digest))
        }
        return (seed, digest)
    }

    /// The seed of a seed string: the first 8 bytes of its SHA-256, read as a little-endian number.
    public static func seedFromText(_ text: String) -> UInt64 {
        // any bytes are valid, so this cannot fail
        try! seedInfo(Array(text.utf8)).seed
    }

    /// The SHA-256 of a seed string (64 lowercase hex characters): the commitment to publish before the draw.
    public static func seedCommit(_ text: String) -> String {
        hex(try! seedInfo(Array(text.utf8)).digest[...])
    }

    /// Draws `k` distinct numbers from 0..<n, in the order drawn.
    public static func draw(n: Int, k: Int, seed: UInt64? = nil) throws -> [Int] {
        let n = try u32(n, "n"), k = try u32(k, "k")
        var out = [UInt32](repeating: 0, count: max(Int(k), 1))
        try withSeed(seed) { s in try check(sortition_draw(n, k, s, &out)) }
        return out.prefix(Int(k)).map(Int.init)
    }

    /// Draws `k` lines from text (one candidate per line; lines are trimmed and empty lines are not candidates).
    public static func drawText(_ text: String, k: Int, seed: UInt64? = nil) throws -> [String] {
        let k = try u32(k, "k")
        let bytes = Array(text.utf8)
        var out = [UInt8](repeating: 0, count: bytes.count + 1)
        let written = withBytes(bytes) { t, n in
            withSeed(seed) { s in sortition_draw_text(t, n, k, s, &out, out.count) }
        }
        if written < 0 { throw SortitionError(code: Int(written), message: nil) }
        if written == 0 { return [] }
        return String(decoding: out[0..<Int(written)], as: UTF8.self)
            .split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
    }

    /// Draws `k` elements of an array inside JSON, YAML or TOML data and returns them as a JSON array string.
    /// `path` says where the array is (`"a.b.0"`); empty means the root.
    public static func drawData(
        _ format: DataFormat, _ text: String, k: Int, path: String = "", seed: UInt64? = nil
    ) throws -> String {
        let k = try u32(k, "k")
        if path.utf8.contains(0) { throw invalid("path must not contain NUL") }
        var out: UnsafeMutablePointer<CChar>?
        try withBytes(Array(text.utf8)) { t, n in
            try withSeed(seed) { s in
                try check(sortition_draw_data(format.rawValue, t, n, path, k, s, &out), detailed: true)
            }
        }
        return take(out)
    }

    /// Reads the key ID of a sealed token (to choose the key before opening it).
    public static func sealKeyID(_ token: String) throws -> UInt8 {
        var id: UInt8 = 0
        try withBytes(Array(token.utf8)) { t, n in
            try check(sortition_seal_key_id(t, n, &id), detailed: true)
        }
        return id
    }

    /// The ID of each candidate (64 lowercase hex characters), in the order of `items`.
    public static func itemIDs<C: Candidate>(_ items: [C]) throws -> [String] {
        var out = [UInt8](repeating: 0, count: max(32 * items.count, 1))
        try withItems(items) { d, l, n in try check(sortition_item_ids(d, l, n, &out)) }
        return (0..<items.count).map { hex(out[(32 * $0)..<(32 * $0 + 32)]) }
    }

    /// The fingerprint of the set of candidates (64 lowercase hex characters). It does not depend on the
    /// order of `items`: publish it with the commitment, before the draw.
    public static func itemFingerprint<C: Candidate>(_ items: [C]) throws -> String {
        var out = [UInt8](repeating: 0, count: 32)
        try withItems(items) { d, l, n in try check(sortition_item_fingerprint(d, l, n, &out)) }
        return hex(out[...])
    }

    /// Draws `k` candidates without replacement. The same set of candidates and the same seed give the same
    /// winners whatever the order of `items`.
    public static func drawItems<C: Candidate>(_ items: [C], k: Int, seed: UInt64? = nil) throws -> [Pick] {
        let k = try u32(k, "k")
        var indexes = [UInt32](repeating: 0, count: max(Int(k), 1))
        var ids = [UInt8](repeating: 0, count: max(32 * Int(k), 1))
        try withItems(items) { d, l, n in
            try withSeed(seed) { s in try check(sortition_items_draw(d, l, n, k, s, &indexes, &ids)) }
        }
        return picks(Array(indexes.prefix(Int(k))), ids)
    }

    /// Draws prize stages in order without giving anyone two prizes. It draws the total once and cuts it up
    /// in the order of `stages`, which has the same probabilities as drawing stage by stage.
    public static func drawStages<C: Candidate>(
        _ items: [C], _ stages: [Stage], seed: UInt64? = nil
    ) throws -> [StageResult] {
        var total = 0
        for stage in stages {
            if stage.count < 0 { throw invalid("stage count must not be negative") }
            let (sum, overflow) = total.addingReportingOverflow(stage.count)
            if overflow { throw invalid("the stages add up to too many winners") }
            total = sum
        }
        let all = try drawItems(items, k: total, seed: seed)
        var offset = 0
        return stages.map { stage in
            defer { offset += stage.count }
            return StageResult(name: stage.name, count: stage.count, winners: Array(all[offset..<(offset + stage.count)]))
        }
    }
}
