import Foundation
import Testing

@testable import Sortition

private let names = ["alice", "bob", "carol", "dave", "erin", "frank", "grace", "heidi"]

private func code(_ body: () throws -> Void) -> Int? {
    do { try body() } catch let e as SortitionError { return e.code } catch { return nil }
    return nil
}

@Suite struct BindingTests {
    @Test func quickstartPrintsTheSameWinnersAsEveryOtherBinding() throws {
        let people = ["alice", "bob", "carol", "dave"]
        let picks = try Lottery.drawItems(people, k: 2, seed: Lottery.seedFromText("my seed"))
        #expect(picks.map { people[$0.index] } == ["dave", "bob"])
        #expect(Lottery.seedCommit("my seed") == "7efa9b064a86e6a99f9cff6c838e61c005ad9aac30e74efc50a0b9ec8270e7ad")
    }

    @Test func winnersDoNotDependOnTheOrderOfTheList() throws {
        let seed = Lottery.seedFromText("order")
        let reversed = Array(names.reversed())
        let a = try Lottery.drawItems(names, k: 5, seed: seed)
        let b = try Lottery.drawItems(reversed, k: 5, seed: seed)
        #expect(a.map(\.id) == b.map(\.id))
        #expect(a.map { names[$0.index] } == b.map { reversed[$0.index] })
        #expect(try Lottery.itemFingerprint(names) == Lottery.itemFingerprint(reversed))
    }

    @Test func bytesAndStringsAreTheSameCandidates() throws {
        #expect(try Lottery.itemIDs([[0x61, 0x62], [0x63], []] as [[UInt8]]) == Lottery.itemIDs(["ab", "c", ""]))
        #expect(Set(try Lottery.itemIDs(["x", "x", "x"])).count == 3)
        #expect(try Lottery.itemIDs([String]()).isEmpty)
    }

    @Test func errorsCarryTheCode() {
        #expect(code { _ = try Lottery.draw(n: 3, k: 4) } == SortitionError.invalid)
        #expect(code { _ = try Lottery.draw(n: 3, k: -1) } == SortitionError.invalid)
        #expect(code { _ = try Lottery.drawItems(["a"], k: 2) } == SortitionError.invalid)
        #expect(code { _ = try Session(state: "{}") } == SortitionError.state)
        #expect(code { _ = try Lottery.sealKeyID("junk") } == SortitionError.state)
        #expect("\(SortitionError(code: -1, message: nil))" == "sortition: error code -1")
    }

    @Test func textAndData() throws {
        #expect(try Lottery.drawText("  a \n\nb\nc\n", k: 3, seed: 1).sorted() == ["a", "b", "c"])
        #expect(try Lottery.drawText("", k: 0).isEmpty)
        let json = try Lottery.drawData(.json, #"{"members": ["a", "b", "c"]}"#, k: 2, path: "members", seed: 7)
        #expect((try JSONSerialization.jsonObject(with: Data(json.utf8)) as! [Any]).count == 2)
        do {
            _ = try Lottery.drawData(.json, "{}", k: 1, path: "nope")
            Issue.record("a missing path must fail")
        } catch let e as SortitionError {
            #expect(e.code == -5)
            #expect(e.message?.isEmpty == false)
        }
    }

    @Test func sessionSplitDrawsEqualOneDrawAndSurviveSaveAndLoad() throws {
        let a = try Session(count: 200, seed: 9)
        var got = try a.draw(20)
        let b = try Session(state: a.save())
        got += try b.draw(30)
        #expect(got == (try Lottery.draw(n: 200, k: 50, seed: 9)))
        #expect(b.remaining == 150 && b.drawnCount == 50)
        #expect(b.id.count == 32)
        #expect(b.revision == 2)  // one draw before the save, one after the restore
        #expect(code { _ = try b.draw(151) } == SortitionError.invalid)
    }

    @Test func idempotencyKeysReplayAndConflict() throws {
        let s = try Session(count: 50)
        let first = try s.draw(3, key: "req-1")
        let state = try s.save()
        #expect(try s.draw(3, key: "req-1") == first)
        #expect(try s.save() == state)
        #expect(code { _ = try s.draw(4, key: "req-1") } == SortitionError.conflict)
        #expect(code { _ = try s.draw(1, key: "a\0b") } == SortitionError.invalid)
        #expect(code { _ = try s.draw(1, key: "") } == SortitionError.invalid)
    }

    @Test func anOldStateSentAgainGivesTheSameResult() throws {
        let a = try Session(count: 100)
        _ = try a.draw(4, key: "a")
        let stale = try a.save()
        let want = try a.draw(6, key: "b")
        let old = try Session(state: stale)
        #expect(try old.draw(6, key: "b") == want)
        #expect(try a.save() == old.save())
    }

    @Test func sealingHidesTheStateAndDetectsChanges() throws {
        let key = [UInt8](repeating: 7, count: 32)
        let a = try Session(count: 50)
        _ = try a.draw(3, key: "r")
        let token = try a.seal(key: key, keyID: 9)
        #expect(token.hasPrefix("sortition1."))
        #expect(try Lottery.sealKeyID(token) == 9)
        #expect(try Session(sealed: token, key: key).save() == a.save())
        let tampered = String(token.dropLast()) + (token.last == "A" ? "B" : "A")
        #expect(code { _ = try Session(sealed: tampered, key: key) } == SortitionError.state)
        #expect(code { _ = try Session(sealed: token, key: [UInt8](repeating: 0, count: 32)) } == SortitionError.state)
        #expect(code { _ = try a.seal(key: [UInt8](repeating: 0, count: 31)) } == SortitionError.invalid)
    }

    @Test func itemSessionsResumeWithTheSameSetInAnyOrder() throws {
        let lot = try ItemSession(items: names, seed: 11)
        let got = try lot.draw(3, key: "k1")
        #expect(try lot.draw(3, key: "k1") == got)
        #expect(lot.revision == 1)
        let state = try lot.save()
        let reversed = Array(names.reversed())
        let again = try ItemSession(state: state, items: reversed)
        let rest = try again.draw(5)
        #expect((got + rest).map(\.id) == (try Lottery.drawItems(names, k: 8, seed: 11)).map(\.id))
        let reversedIDs = try Lottery.itemIDs(reversed)
        #expect(rest.allSatisfy { reversedIDs[$0.index] == $0.id })
        #expect(code { _ = try ItemSession(state: state, items: Array(names.dropFirst())) } == SortitionError.state)

        let token = try lot.seal(key: [UInt8](repeating: 0, count: 32), keyID: 1)
        let rotated = Array(names[2...] + names[..<2])
        let unsealed = try ItemSession(sealed: token, key: [UInt8](repeating: 0, count: 32), items: rotated)
        #expect(try lot.draw(5).map(\.id) == unsealed.draw(5).map(\.id))
    }

    @Test func stagesCutOneDrawInOrderAndCheckTheirInput() throws {
        let seed = Lottery.seedFromText("stages")
        let got = try Lottery.drawStages(names, [Stage("副賞", 3), Stage("大賞", 1)], seed: seed)
        #expect(got.map(\.name) == ["副賞", "大賞"])
        #expect(got.flatMap(\.winners) == (try Lottery.drawItems(names, k: 4, seed: seed)))
        #expect(got[0].winners.map { names[$0.index] } == ["carol", "heidi", "bob"])
        #expect(try Lottery.drawStages(names, []).isEmpty)
        #expect(try Lottery.drawStages(names, [Stage("none", 0)])[0].winners.isEmpty)
        #expect(code { _ = try Lottery.drawStages(names, [Stage("too many", 99)]) } == SortitionError.invalid)
    }

    @Test func withoutASeedTheResultsDiffer() throws {
        #expect(try Lottery.draw(n: 1_000_000, k: 8) != Lottery.draw(n: 1_000_000, k: 8))
        #expect(try Session(count: 10).id != Session(count: 10).id)
    }

    @Test func theNativeVersionIsReported() {
        #expect(Lottery.version == "0.1.1")
    }
}
