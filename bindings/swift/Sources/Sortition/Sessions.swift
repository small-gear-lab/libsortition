import CSortition

/// What both kinds of session share. The native session is freed when the object goes away.
/// A session is not thread-safe: use one from one thread at a time.
public class SessionBase {
    struct Fns: Sendable {
        let remaining: @Sendable (OpaquePointer?) -> UInt32
        let drawnCount: @Sendable (OpaquePointer?) -> UInt32
        let revision: @Sendable (OpaquePointer?) -> UInt64
        let id: @Sendable (OpaquePointer?, UnsafeMutablePointer<UInt8>?) -> Int32
        let save: @Sendable (OpaquePointer?, UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?) -> Int32
        let seal: @Sendable (OpaquePointer?, UnsafePointer<UInt8>?, UInt8, UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?) -> Int32
        let free: @Sendable (OpaquePointer?) -> Void
    }

    let fns: Fns
    let handle: OpaquePointer

    init(_ fns: Fns, _ handle: OpaquePointer) {
        self.fns = fns
        self.handle = handle
    }

    deinit { fns.free(handle) }

    /// How many are not drawn yet.
    public var remaining: Int { Int(fns.remaining(handle)) }

    /// How many were drawn.
    public var drawnCount: Int { Int(fns.drawnCount(handle)) }

    /// The number of draws that changed the state. A store can update a record only when the revision is the
    /// one it read, so that two updates made from the same state do not overwrite each other.
    public var revision: UInt64 { fns.revision(handle) }

    /// The session's identifier (32 lowercase hex characters), taken from the operating system when it was created.
    public var id: String {
        var out = [UInt8](repeating: 0, count: 33)
        _ = fns.id(handle, &out)
        return String(decoding: out.prefix(32), as: UTF8.self)
    }

    /// The state as JSON. It contains the internal state of the random generator: whoever has it can predict
    /// the next winners, so do not give it to a party you do not trust. ``seal(key:keyID:)`` it instead.
    public func save() throws -> String {
        var out: UnsafeMutablePointer<CChar>?
        try check(fns.save(handle, &out))
        return take(out)
    }

    /// The state as a token that only the holder of `key` can open, and that cannot be changed without notice
    /// (XChaCha20-Poly1305). `key` is a secret of exactly 32 bytes (make it with a cryptographic random
    /// generator); `keyID` (0 to 255) is a label for rotating keys, see ``Lottery/sealKeyID(_:)``.
    public func seal(key: [UInt8], keyID: UInt8 = 0) throws -> String {
        try SessionBase.checkKey(key)
        var out: UnsafeMutablePointer<CChar>?
        try withBytes(key) { k, _ in try check(fns.seal(handle, k, keyID, &out)) }
        return take(out)
    }

    static func checkKey(_ key: [UInt8]) throws {
        if key.count != 32 { throw invalid("key must be exactly 32 bytes") }
    }

    static func checkKeyString(_ key: String?) throws {
        if let key, key.utf8.contains(0) { throw invalid("key must not contain NUL") }
    }
}

private let sessionFns = SessionBase.Fns(
    remaining: sortition_session_remaining, drawnCount: sortition_session_drawn_count,
    revision: sortition_session_revision, id: sortition_session_id, save: sortition_session_save,
    seal: sortition_session_seal, free: sortition_session_free)

private let itemSessionFns = SessionBase.Fns(
    remaining: sortition_item_session_remaining, drawnCount: sortition_item_session_drawn_count,
    revision: sortition_item_session_revision, id: sortition_item_session_id, save: sortition_item_session_save,
    seal: sortition_item_session_seal, free: sortition_item_session_free)

/// A session draws without replacement from 0..<n and continues where it stopped. Drawing in several steps,
/// with a save and a restore in between, gives the same order as one draw.
public final class Session: SessionBase {
    private init(_ handle: OpaquePointer) { super.init(sessionFns, handle) }

    private static func make(_ create: (UnsafeMutablePointer<OpaquePointer?>) throws -> Void) throws -> Session {
        var out: OpaquePointer?
        try create(&out)
        return Session(out!)
    }

    /// Creates a session over 0..<n. Without a seed it runs a cryptographic stream seeded from the operating system.
    public convenience init(count n: Int, seed: UInt64? = nil) throws {
        let n = try u32(n, "n")
        var out: OpaquePointer?
        try withSeed(seed) { s in try check(sortition_session_new(n, s, &out)) }
        self.init(out!)
    }

    /// Restores a session from ``save()``.
    public convenience init(state: String) throws {
        var out: OpaquePointer?
        try withBytes(Array(state.utf8)) { t, n in try check(sortition_session_load(t, n, &out), detailed: true) }
        self.init(out!)
    }

    /// Restores a session from a token made by ``seal(key:keyID:)``. A malformed token, a wrong key and a
    /// changed token all give ``SortitionError/state``.
    public convenience init(sealed token: String, key: [UInt8]) throws {
        try SessionBase.checkKey(key)
        var out: OpaquePointer?
        try withBytes(Array(token.utf8)) { t, n in
            try withBytes(key) { k, _ in try check(sortition_session_unseal(t, n, k, &out), detailed: true) }
        }
        self.init(out!)
    }

    /// Draws the next `k` numbers. More than ``remaining`` throws ``SortitionError/invalid`` and leaves the state
    /// unchanged. With an idempotency `key` (1 to 128 bytes), the same key and the same `k` again does not change
    /// the state and returns the earlier result; the same key with another `k` throws ``SortitionError/conflict``.
    public func draw(_ k: Int, key: String? = nil) throws -> [Int] {
        let k = try u32(k, "k")
        try SessionBase.checkKeyString(key)
        var out = [UInt32](repeating: 0, count: max(Int(k), 1))
        try check(sortition_session_draw_keyed(handle, k, key, &out, nil))
        return out.prefix(Int(k)).map(Int.init)
    }
}

/// A session over a fixed set of candidates. Winners are ``Pick``s; after a restore, ``Pick/index`` is a
/// position in the list that was passed again.
public final class ItemSession: SessionBase {
    private init(_ handle: OpaquePointer) { super.init(itemSessionFns, handle) }

    /// Creates a session over a fixed set of candidates.
    public convenience init<C: Candidate>(items: [C], seed: UInt64? = nil) throws {
        var out: OpaquePointer?
        try withItems(items) { d, l, n in
            try withSeed(seed) { s in try check(sortition_item_session_new(d, l, n, s, &out)) }
        }
        self.init(out!)
    }

    /// Restores a session from ``save()`` and the same set of candidates (the order may differ; another set
    /// throws ``SortitionError/state``).
    public convenience init<C: Candidate>(state: String, items: [C]) throws {
        var out: OpaquePointer?
        try withBytes(Array(state.utf8)) { t, tn in
            try withItems(items) { d, l, n in
                try check(sortition_item_session_load(t, tn, d, l, n, &out), detailed: true)
            }
        }
        self.init(out!)
    }

    /// Restores a session from a token made by ``seal(key:keyID:)`` and the same set of candidates.
    public convenience init<C: Candidate>(sealed token: String, key: [UInt8], items: [C]) throws {
        try SessionBase.checkKey(key)
        var out: OpaquePointer?
        try withBytes(Array(token.utf8)) { t, tn in
            try withBytes(key) { k, _ in
                try withItems(items) { d, l, n in
                    try check(sortition_item_session_unseal(t, tn, k, d, l, n, &out), detailed: true)
                }
            }
        }
        self.init(out!)
    }

    /// Draws the next `k` candidates, with an optional idempotency `key` as in ``Session/draw(_:key:)``.
    public func draw(_ k: Int, key: String? = nil) throws -> [Pick] {
        let k = try u32(k, "k")
        try SessionBase.checkKeyString(key)
        var indexes = [UInt32](repeating: 0, count: max(Int(k), 1))
        var ids = [UInt8](repeating: 0, count: max(32 * Int(k), 1))
        try check(sortition_item_session_draw_keyed(handle, k, key, &indexes, &ids, nil))
        return picks(Array(indexes.prefix(Int(k))), ids)
    }
}
