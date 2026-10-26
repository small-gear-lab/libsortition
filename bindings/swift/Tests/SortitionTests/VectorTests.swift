import Foundation
import Testing

@testable import Sortition

/// The shared test vectors (docs/SPEC.md; test-vectors/vectors.json): every binding must give the same values.
enum Vectors {
    /// The raw file (`Data` is Sendable); it is parsed on each use, which is cheap enough for a test.
    private static let raw: Data = {
        var dir = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        while true {
            let file = dir.appendingPathComponent("test-vectors/vectors.json")
            if let data = try? Data(contentsOf: file) { return data }
            let parent = dir.deletingLastPathComponent()
            precondition(parent != dir, "test-vectors/vectors.json was not found above the test file")
            dir = parent
        }
    }()

    static var all: [String: Any] { try! JSONSerialization.jsonObject(with: raw) as! [String: Any] }

    static func list(_ key: String) -> [[String: Any]] { all[key] as! [[String: Any]] }
    static func seed(_ v: Any?) -> UInt64 { UInt64(v as! String)! }
    static func ints(_ v: Any?) -> [Int] { (v as! [NSNumber]).map(\.intValue) }

    static func bytes(hex: String) -> [UInt8] {
        var out: [UInt8] = []
        var index = hex.startIndex
        while index < hex.endIndex {
            let next = hex.index(index, offsetBy: 2)
            out.append(UInt8(hex[index..<next], radix: 16)!)
            index = next
        }
        return out
    }

    static func sets() -> [String: [[UInt8]]] {
        Dictionary(uniqueKeysWithValues: list("item_sets").map {
            ($0["name"] as! String, ($0["items_hex"] as! [String]).map(bytes(hex:)))
        })
    }

    static func picks(_ v: Any?) -> [Pick] {
        (v as! [[String: Any]]).map { Pick(index: ($0["index"] as! NSNumber).intValue, id: $0["id"] as! String) }
    }
}

@Suite struct VectorTests {
    @Test func seedFromText() {
        for v in Vectors.list("seed_from_text") {
            let text = v["text"] as! String
            #expect(Lottery.seedFromText(text) == Vectors.seed(v["seed"]), "\(text)")
            #expect(Lottery.seedCommit(text) == v["digest"] as! String, "\(text)")
        }
    }

    @Test func draw() throws {
        for v in Vectors.list("draw") {
            let got = try Lottery.draw(n: Int(truncating: v["n"] as! NSNumber), k: Int(truncating: v["k"] as! NSNumber), seed: Vectors.seed(v["seed"]))
            #expect(got == Vectors.ints(v["result"]))
        }
    }

    @Test func itemIDsAndFingerprints() throws {
        for v in Vectors.list("item_sets") {
            let items = (v["items_hex"] as! [String]).map(Vectors.bytes(hex:))
            #expect(try Lottery.itemIDs(items) == v["ids"] as! [String])
            #expect(try Lottery.itemFingerprint(items) == v["fingerprint"] as! String)
        }
    }

    @Test func drawItems() throws {
        let sets = Vectors.sets()
        for v in Vectors.list("draw_items") {
            let got = try Lottery.drawItems(sets[v["set"] as! String]!, k: (v["k"] as! NSNumber).intValue, seed: Vectors.seed(v["seed"]))
            #expect(got == Vectors.picks(v["winners"]))
        }
    }

    @Test func stages() throws {
        let sets = Vectors.sets()
        for v in Vectors.list("stages") {
            let expected = v["stages"] as! [[String: Any]]
            let plan = expected.map { Stage($0["name"] as! String, ($0["count"] as! NSNumber).intValue) }
            let got = try Lottery.drawStages(sets[v["set"] as! String]!, plan, seed: Lottery.seedFromText(v["seed_text"] as! String))
            #expect(got.count == expected.count)
            for (g, x) in zip(got, expected) {
                #expect(g.name == x["name"] as! String)
                #expect(g.winners == Vectors.picks(x["winners"]))
            }
        }
    }

    private func drawAll(_ session: Session, _ v: [String: Any]) throws {
        let draws = Vectors.ints(v["draws"])
        let expected = (v["expected"] as! [Any]).map(Vectors.ints)
        for (k, want) in zip(draws, expected) { #expect(try session.draw(k) == want) }
    }

    @Test func sessionRestoredFromAChaCha20State() throws {
        let v = Vectors.all["chacha20_session"] as! [String: Any]
        let state = String(decoding: try JSONSerialization.data(withJSONObject: v["state"]!, options: [.sortedKeys]), as: UTF8.self)
        try drawAll(Session(state: state), v)
    }

    @Test func sealedToken() throws {
        let v = Vectors.all["seal"] as! [String: Any]
        let token = v["token"] as! String
        #expect(try Lottery.sealKeyID(token) == UInt8((v["key_id"] as! NSNumber).intValue))
        try drawAll(Session(sealed: token, key: Vectors.bytes(hex: v["key"] as! String)), v)
    }
}
