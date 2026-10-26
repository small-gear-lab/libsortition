import CSortition

/// A failure reported by the core.
public struct SortitionError: Error, CustomStringConvertible, Equatable {
    /// An argument is out of range (k larger than the number of candidates, an empty key, ...).
    public static let invalid = -1
    /// An invalid saved state or sealed token (or a different set of candidates).
    public static let state = -8
    /// An idempotency key was used with a different request.
    public static let conflict = -9

    /// The negative error code of the core (see docs/SPEC.md).
    public let code: Int
    /// Details, when the core has them.
    public let message: String?

    public var description: String {
        if let message, !message.isEmpty { return "sortition: \(message) (code \(code))" }
        return "sortition: error code \(code)"
    }
}

/// Something that can be a candidate: a `String` (its UTF-8 bytes) or `[UInt8]`.
public protocol Candidate {
    var candidateBytes: [UInt8] { get }
}

extension String: Candidate {
    public var candidateBytes: [UInt8] { Array(utf8) }
}

extension Array: Candidate where Element == UInt8 {
    public var candidateBytes: [UInt8] { self }
}

/// The format of structured data for ``Lottery/drawData(_:_:k:path:seed:)``.
public enum DataFormat: Int32, Sendable {
    case json = 0
    /// YAML (aliases, custom tags and multiple documents are rejected).
    case yaml = 1
    /// TOML (it needs a path, because its root is a table).
    case toml = 2
}

/// A winner: its position in the list that was passed, and its ID (SHA-256, 64 lowercase hex characters).
public struct Pick: Equatable, Sendable {
    public let index: Int
    public let id: String
}

/// One prize stage of a plan.
public struct Stage: Equatable, Sendable {
    public let name: String
    public let count: Int

    public init(_ name: String, _ count: Int) {
        self.name = name
        self.count = count
    }
}

/// The winners of one prize stage.
public struct StageResult: Equatable, Sendable {
    public let name: String
    public let count: Int
    public let winners: [Pick]
}

// MARK: - helpers

func lastError() -> String? {
    sortition_last_error().map { String(cString: $0) }
}

func check(_ rc: Int32, detailed: Bool = false) throws {
    if rc != 0 { throw SortitionError(code: Int(rc), message: detailed ? lastError() : nil) }
}

func invalid(_ message: String) -> SortitionError {
    SortitionError(code: SortitionError.invalid, message: message)
}

/// A value for a C `uint32_t`.
func u32(_ value: Int, _ name: String) throws -> UInt32 {
    guard let v = UInt32(exactly: value) else { throw invalid("\(name) must be in 0...4294967295") }
    return v
}

func withSeed<R>(_ seed: UInt64?, _ body: (UnsafePointer<UInt64>?) throws -> R) rethrows -> R {
    guard var s = seed else { return try body(nil) }
    return try withUnsafePointer(to: &s) { try body($0) }
}

/// The candidates as one buffer and an array of lengths (never empty, so the pointers are never nil).
func withItems<C: Candidate, R>(
    _ items: [C], _ body: (UnsafePointer<UInt8>, UnsafePointer<Int>, Int) throws -> R
) rethrows -> R {
    var data: [UInt8] = []
    var lens: [Int] = []
    for item in items {
        let bytes = item.candidateBytes
        data += bytes
        lens.append(bytes.count)
    }
    if data.isEmpty { data.append(0) }
    if lens.isEmpty { lens.append(0) }
    let count = items.count
    return try data.withUnsafeBufferPointer { d in
        try lens.withUnsafeBufferPointer { l in try body(d.baseAddress!, l.baseAddress!, count) }
    }
}

func withBytes<R>(_ bytes: [UInt8], _ body: (UnsafePointer<UInt8>, Int) throws -> R) rethrows -> R {
    let padded = bytes.isEmpty ? [0] : bytes
    return try padded.withUnsafeBufferPointer { try body($0.baseAddress!, bytes.count) }
}

/// Reads a string the core allocated, and frees it.
func take(_ p: UnsafeMutablePointer<CChar>?) -> String {
    guard let p else { return "" }
    defer { sortition_free(p) }
    return String(cString: p)
}

func hex(_ bytes: ArraySlice<UInt8>) -> String {
    let digits = Array("0123456789abcdef")
    var out = ""
    out.reserveCapacity(bytes.count * 2)
    for b in bytes {
        out.append(digits[Int(b >> 4)])
        out.append(digits[Int(b & 15)])
    }
    return out
}

func picks(_ indexes: [UInt32], _ ids: [UInt8]) -> [Pick] {
    indexes.enumerated().map { i, index in
        Pick(index: Int(index), id: hex(ids[(32 * i)..<(32 * i + 32)]))
    }
}
