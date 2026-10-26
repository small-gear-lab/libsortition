package io.github.smallgearlab.sortition;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import org.junit.jupiter.api.Test;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.HexFormat;
import java.util.List;
import java.util.Map;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;

/** The shared test vectors (docs/SPEC.md; test-vectors/vectors.json): every binding must give the same values. */
class VectorTest {
    private static final JsonObject V = load();

    private static JsonObject load() {
        try {
            for (var dir = Path.of("").toAbsolutePath(); dir != null; dir = dir.getParent()) {
                var file = dir.resolve("test-vectors").resolve("vectors.json");
                if (Files.isRegularFile(file)) {
                    return JsonParser.parseString(Files.readString(file)).getAsJsonObject();
                }
            }
        } catch (IOException e) {
            throw new IllegalStateException(e);
        }
        throw new IllegalStateException("test-vectors/vectors.json was not found above the working directory");
    }

    private static long seed(JsonElement e) {
        return Long.parseUnsignedLong(e.getAsString());
    }

    private static int[] ints(JsonArray a) {
        return a.asList().stream().mapToInt(JsonElement::getAsInt).toArray();
    }

    private static Items items(JsonObject set) {
        var list = new ArrayList<byte[]>();
        for (var h : set.getAsJsonArray("items_hex")) {
            list.add(HexFormat.of().parseHex(h.getAsString()));
        }
        return Items.ofBytes(list);
    }

    private static Map<String, Items> sets() {
        var out = new HashMap<String, Items>();
        for (var s : V.getAsJsonArray("item_sets")) {
            out.put(s.getAsJsonObject().get("name").getAsString(), items(s.getAsJsonObject()));
        }
        return out;
    }

    private static List<Pick> picks(JsonArray a) {
        return a.asList().stream()
                .map(w -> new Pick(w.getAsJsonObject().get("index").getAsInt(), w.getAsJsonObject().get("id").getAsString()))
                .toList();
    }

    private static List<String> strings(JsonArray a) {
        return a.asList().stream().map(JsonElement::getAsString).toList();
    }

    @Test
    void seedFromText() {
        for (var e : V.getAsJsonArray("seed_from_text")) {
            var v = e.getAsJsonObject();
            var text = v.get("text").getAsString();
            assertEquals(seed(v.get("seed")), Lottery.seedFromText(text), text);
            assertEquals(v.get("digest").getAsString(), Lottery.seedCommit(text), text);
        }
    }

    @Test
    void draw() {
        for (var e : V.getAsJsonArray("draw")) {
            var v = e.getAsJsonObject();
            assertArrayEquals(ints(v.getAsJsonArray("result")),
                    Lottery.draw(v.get("n").getAsLong(), v.get("k").getAsInt(), seed(v.get("seed"))));
        }
    }

    @Test
    void itemIdsAndFingerprints() {
        for (var e : V.getAsJsonArray("item_sets")) {
            var v = e.getAsJsonObject();
            assertEquals(strings(v.getAsJsonArray("ids")), Lottery.itemIds(items(v)));
            assertEquals(v.get("fingerprint").getAsString(), Lottery.itemFingerprint(items(v)));
        }
    }

    @Test
    void drawItems() {
        var sets = sets();
        for (var e : V.getAsJsonArray("draw_items")) {
            var v = e.getAsJsonObject();
            assertEquals(picks(v.getAsJsonArray("winners")),
                    Lottery.drawItems(sets.get(v.get("set").getAsString()), v.get("k").getAsInt(), seed(v.get("seed"))));
        }
    }

    @Test
    void stages() {
        var sets = sets();
        for (var e : V.getAsJsonArray("stages")) {
            var v = e.getAsJsonObject();
            var plan = new ArrayList<Stage>();
            for (var s : v.getAsJsonArray("stages")) {
                plan.add(new Stage(s.getAsJsonObject().get("name").getAsString(), s.getAsJsonObject().get("count").getAsInt()));
            }
            var got = Lottery.drawStages(sets.get(v.get("set").getAsString()), plan, Lottery.seedFromText(v.get("seed_text").getAsString()));
            var expected = v.getAsJsonArray("stages");
            assertEquals(expected.size(), got.size());
            for (int i = 0; i < got.size(); i++) {
                var x = expected.get(i).getAsJsonObject();
                assertEquals(x.get("name").getAsString(), got.get(i).name());
                assertEquals(x.get("count").getAsInt(), got.get(i).count());
                assertEquals(picks(x.getAsJsonArray("winners")), got.get(i).winners());
            }
        }
    }

    private static void drawAll(Session session, JsonObject v) {
        var draws = ints(v.getAsJsonArray("draws"));
        var expected = v.getAsJsonArray("expected");
        for (int i = 0; i < draws.length; i++) {
            assertArrayEquals(ints(expected.get(i).getAsJsonArray()), session.draw(draws[i]), "draw " + i);
        }
    }

    @Test
    void sessionRestoredFromAChaCha20State() {
        var v = V.getAsJsonObject("chacha20_session");
        try (var session = Session.load(v.get("state").toString())) {
            drawAll(session, v);
        }
    }

    @Test
    void sealedToken() {
        var v = V.getAsJsonObject("seal");
        var token = v.get("token").getAsString();
        assertEquals(v.get("key_id").getAsInt(), Lottery.sealKeyId(token));
        try (var session = Session.unseal(token, HexFormat.of().parseHex(v.get("key").getAsString()))) {
            drawAll(session, v);
        }
    }
}
