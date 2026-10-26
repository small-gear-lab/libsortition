package io.github.smallgearlab.sortition;

import com.google.gson.JsonParser;
import org.junit.jupiter.api.Test;

import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.List;
import java.util.TreeSet;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertNotEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

class BindingTest {
    private static final List<String> NAMES = List.of("alice", "bob", "carol", "dave", "erin", "frank", "grace", "heidi");

    private static int code(Runnable r) {
        return assertThrows(SortitionException.class, r::run).code();
    }

    private static List<String> ids(List<Pick> picks) {
        return picks.stream().map(Pick::id).toList();
    }

    private static int[] concat(int[] a, int[] b) {
        var out = Arrays.copyOf(a, a.length + b.length);
        System.arraycopy(b, 0, out, a.length, b.length);
        return out;
    }

    @Test
    void quickstartPrintsTheSameWinnersAsEveryOtherBinding() {
        var people = List.of("alice", "bob", "carol", "dave");
        var picks = Lottery.drawItems(Items.of(people), 2, Lottery.seedFromText("my seed"));
        assertEquals(List.of("dave", "bob"), picks.stream().map(p -> people.get(p.index())).toList());
        assertEquals("7efa9b064a86e6a99f9cff6c838e61c005ad9aac30e74efc50a0b9ec8270e7ad", Lottery.seedCommit("my seed"));
    }

    @Test
    void winnersDoNotDependOnTheOrderOfTheList() {
        long seed = Lottery.seedFromText("order");
        var reversed = new ArrayList<>(NAMES);
        Collections.reverse(reversed);
        var a = Lottery.drawItems(Items.of(NAMES), 5, seed);
        var b = Lottery.drawItems(Items.of(reversed), 5, seed);
        assertEquals(ids(a), ids(b));
        for (int i = 0; i < a.size(); i++) {
            assertEquals(NAMES.get(a.get(i).index()), reversed.get(b.get(i).index()));
        }
        assertEquals(Lottery.itemFingerprint(Items.of(NAMES)), Lottery.itemFingerprint(Items.of(reversed)));
    }

    @Test
    void bytesAndStringsAreTheSameCandidates() {
        var bytes = Items.ofBytes(List.of(new byte[]{0x61, 0x62}, new byte[]{0x63}, new byte[0]));
        assertEquals(Lottery.itemIds(Items.of("ab", "c", "")), Lottery.itemIds(bytes));
        assertEquals(3, new TreeSet<>(Lottery.itemIds(Items.of("x", "x", "x"))).size());
        assertEquals(0, Lottery.itemIds(Items.of()).size());
    }

    @Test
    void errorsCarryTheCode() {
        var e = assertThrows(SortitionException.class, () -> Lottery.draw(3, 4));
        assertEquals(SortitionException.INVALID, e.code());
        assertInstanceOf(IllegalArgumentException.class, e);
        assertTrue(e.getMessage().contains("-1"));
        assertEquals(SortitionException.INVALID, code(() -> Lottery.drawItems(Items.of("a"), 2)));
        assertEquals(SortitionException.INVALID, code(() -> Lottery.draw(3, -1)));
        assertEquals(SortitionException.STATE, code(() -> Session.load("{}")));
        assertEquals(SortitionException.STATE, code(() -> Lottery.sealKeyId("junk")));
    }

    @Test
    void textAndData() {
        var lines = new ArrayList<>(Lottery.drawText("  a \n\nb\nc\n", 3, 1));
        Collections.sort(lines);
        assertEquals(List.of("a", "b", "c"), lines);
        assertEquals(List.of(), Lottery.drawText("", 0));
        var json = Lottery.drawData(DataFormat.JSON, "{\"members\": [\"a\", \"b\", \"c\"]}", 2, "members", 7);
        assertEquals(2, JsonParser.parseString(json).getAsJsonArray().size());
        var e = assertThrows(SortitionException.class, () -> Lottery.drawData(DataFormat.JSON, "{}", 1, ""));
        assertEquals(-5, e.code());
        assertTrue(e.getMessage().length() > "sortition error code -5".length(), "the message has details");
    }

    @Test
    void sessionSplitDrawsEqualOneDrawAndSurviveSaveAndLoad() {
        try (var a = Session.create(200, 9)) {
            var got = a.draw(20);
            try (var b = Session.load(a.save())) {
                got = concat(got, b.draw(30));
                assertArrayEquals(Lottery.draw(200, 50, 9), got);
                assertEquals(150, b.remaining());
                assertEquals(50, b.drawnCount());
                assertTrue(b.id().matches("[0-9a-f]{32}"));
                assertEquals(2L, b.revision()); // one draw before the save, one after the restore
                assertEquals(SortitionException.INVALID, code(() -> b.draw(151)));
            }
        }
    }

    @Test
    void idempotencyKeysReplayAndConflict() {
        try (var s = Session.create(50)) {
            var first = s.draw(3, "req-1");
            var state = s.save();
            assertArrayEquals(first, s.draw(3, "req-1"));
            assertEquals(state, s.save());
            assertEquals(SortitionException.CONFLICT, code(() -> s.draw(4, "req-1")));
            assertEquals(SortitionException.INVALID, code(() -> s.draw(1, "a\0b")));
            assertEquals(SortitionException.INVALID, code(() -> s.draw(1, "")));
        }
    }

    @Test
    void anOldStateSentAgainGivesTheSameResult() {
        try (var a = Session.create(100)) {
            a.draw(4, "a");
            var stale = a.save();
            var want = a.draw(6, "b");
            try (var old = Session.load(stale)) {
                assertArrayEquals(want, old.draw(6, "b"));
                assertEquals(a.save(), old.save());
            }
        }
    }

    @Test
    void sealingHidesTheStateAndDetectsChanges() {
        var key = new byte[32];
        Arrays.fill(key, (byte) 7);
        try (var a = Session.create(50)) {
            a.draw(3, "r");
            var token = a.seal(key, 9);
            assertTrue(token.startsWith("sortition1."));
            assertEquals(9, Lottery.sealKeyId(token));
            assertEquals(200, Lottery.sealKeyId(a.seal(key, 200)));
            try (var b = Session.unseal(token, key)) {
                assertEquals(a.save(), b.save());
            }
            var last = token.charAt(token.length() - 1);
            var tampered = token.substring(0, token.length() - 1) + (last == 'A' ? 'B' : 'A');
            assertEquals(SortitionException.STATE, code(() -> Session.unseal(tampered, key)));
            assertEquals(SortitionException.STATE, code(() -> Session.unseal(token, new byte[32])));
            assertEquals(SortitionException.INVALID, code(() -> a.seal(new byte[31])));
            assertEquals(SortitionException.INVALID, code(() -> a.seal(key, 256)));
        }
    }

    @Test
    void itemSessionsResumeWithTheSameSetInAnyOrder() {
        var names = Items.of(NAMES);
        var reversed = new ArrayList<>(NAMES);
        Collections.reverse(reversed);
        try (var lot = ItemSession.create(names, 11)) {
            var got = lot.draw(3, "k1");
            assertEquals(got, lot.draw(3, "k1"));
            assertEquals(1L, lot.revision());
            var state = lot.save();
            try (var again = ItemSession.load(state, Items.of(reversed))) {
                var rest = again.draw(5);
                var all = new ArrayList<>(ids(got));
                all.addAll(ids(rest));
                assertEquals(ids(Lottery.drawItems(names, 8, 11)), all);
                var reversedIds = Lottery.itemIds(Items.of(reversed));
                for (var p : rest) {
                    assertEquals(reversedIds.get(p.index()), p.id());
                }
            }
            assertEquals(SortitionException.STATE, code(() -> ItemSession.load(state, Items.of(NAMES.subList(1, 8)))));

            var key = new byte[32];
            var token = lot.seal(key, 1);
            var rotated = new ArrayList<>(NAMES.subList(2, 8));
            rotated.addAll(NAMES.subList(0, 2));
            try (var unsealed = ItemSession.unseal(token, key, Items.of(rotated))) {
                assertEquals(ids(lot.draw(5)), ids(unsealed.draw(5)));
            }
        }
    }

    @Test
    void stagesCutOneDrawInOrderAndCheckTheirInput() {
        var names = Items.of(NAMES);
        long seed = Lottery.seedFromText("stages");
        var got = Lottery.drawStages(names, List.of(new Stage("副賞", 3), new Stage("大賞", 1)), seed);
        assertEquals(List.of("副賞", "大賞"), got.stream().map(StageResult::name).toList());
        var all = got.stream().flatMap(r -> r.winners().stream()).toList();
        assertEquals(Lottery.drawItems(names, 4, seed), all);
        assertEquals(List.of("carol", "heidi", "bob"), got.get(0).winners().stream().map(p -> NAMES.get(p.index())).toList());
        assertEquals(List.of(), Lottery.drawStages(names, List.of()));
        assertEquals(List.of(), Lottery.drawStages(names, List.of(new Stage("none", 0))).get(0).winners());
        assertEquals(SortitionException.INVALID, code(() -> Lottery.drawStages(names, List.of(new Stage("too many", 99)))));
    }

    @Test
    void aClosedSessionCannotBeUsed() {
        var s = Session.create(5);
        s.close();
        s.close();
        assertThrows(IllegalStateException.class, () -> s.draw(1));
        assertThrows(IllegalStateException.class, s::remaining);
    }

    @Test
    void withoutASeedTheResultsDiffer() {
        assertFalse(Arrays.equals(Lottery.draw(1_000_000, 8), Lottery.draw(1_000_000, 8)));
        assertNotEquals(Session.create(10).id(), Session.create(10).id());
    }

    @Test
    void theNativeVersionIsReported() {
        assertEquals("0.1.0", Lottery.version());
    }
}
