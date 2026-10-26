package io.github.smallgearlab.sortition;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.HexFormat;
import java.util.List;

import static java.lang.foreign.ValueLayout.ADDRESS;
import static java.lang.foreign.ValueLayout.JAVA_BYTE;
import static java.lang.foreign.ValueLayout.JAVA_INT;
import static java.lang.foreign.ValueLayout.JAVA_LONG;

/**
 * Fair, repeatable and verifiable random lotteries. Use the candidate functions
 * ({@link #drawItems}, {@link #drawStages}, {@link ItemSession}) when the winners must not depend on
 * the order of your list: each candidate gets an ID from its bytes, and the draw works on the IDs in
 * sorted order. {@link #draw}, {@link #drawText} and {@link #drawData} return positions in the list as given.
 *
 * <p>Pass a seed ({@link #seedFromText}) for a repeatable result; without one the draw uses the
 * operating system's randomness. Every method throws {@link SortitionException} for invalid input.
 */
public final class Lottery {
    private Lottery() {}

    /**
     * The version of the native library.
     *
     * @return the version string
     */
    public static String version() {
        return Native.readCString((MemorySegment) Native.call(Native.VERSION));
    }

    private static int count(int value, String name) {
        if (value < 0) {
            throw new SortitionException(SortitionException.INVALID, name + " must not be negative");
        }
        return value;
    }

    private static MemorySegment seedInfo(Arena arena, byte[] text, MemorySegment seed) {
        var digest = arena.allocate(32);
        Native.check(Native.callInt(Native.SEED_FROM_TEXT, Native.bytes(arena, text), (long) text.length, seed, digest), false);
        return digest;
    }

    /**
     * The seed of a seed string: the first 8 bytes of its SHA-256, read as a little-endian number.
     *
     * @param text the seed string
     * @return the seed
     */
    public static long seedFromText(String text) {
        try (var arena = Arena.ofConfined()) {
            var seed = arena.allocate(JAVA_LONG);
            seedInfo(arena, text.getBytes(StandardCharsets.UTF_8), seed);
            return seed.get(JAVA_LONG, 0);
        }
    }

    /**
     * The SHA-256 of a seed string (64 lowercase hex characters): the commitment to publish before the draw.
     *
     * @param text the seed string
     * @return the commitment
     */
    public static String seedCommit(String text) {
        try (var arena = Arena.ofConfined()) {
            var digest = seedInfo(arena, text.getBytes(StandardCharsets.UTF_8), arena.allocate(JAVA_LONG));
            return HexFormat.of().formatHex(digest.toArray(JAVA_BYTE));
        }
    }

    /** A count of 0..2^32-1 (the C {@code uint32_t}), as the int that carries its bits. */
    static int u32(long n, String name) {
        if (n < 0 || n > 0xFFFFFFFFL) {
            throw new SortitionException(SortitionException.INVALID, name + " must be in 0..4294967295");
        }
        return (int) n;
    }

    private static int[] draw(long n, int k, Long seed) {
        int count = u32(n, "n");
        count(k, "k");
        try (var arena = Arena.ofConfined()) {
            var out = Native.ints(arena, k);
            Native.check(Native.callInt(Native.DRAW, count, k, Native.seed(arena, seed), out), false);
            return Native.readInts(out, k);
        }
    }

    /**
     * Draws {@code k} distinct numbers from 0..n-1, in the order drawn, with the operating system's randomness.
     *
     * @param n how many numbers there are
     * @param k how many to draw
     * @return the numbers drawn
     */
    public static int[] draw(long n, int k) {
        return draw(n, k, null);
    }

    /**
     * Draws {@code k} distinct numbers from 0..n-1, in the order drawn. The same seed gives the same result.
     *
     * @param n    how many numbers there are
     * @param k    how many to draw
     * @param seed the seed
     * @return the numbers drawn
     */
    public static int[] draw(long n, int k, long seed) {
        return draw(n, k, Long.valueOf(seed));
    }

    private static List<String> drawText(String text, int k, Long seed) {
        count(k, "k");
        var data = text.getBytes(StandardCharsets.UTF_8);
        try (var arena = Arena.ofConfined()) {
            long capacity = data.length + 1L;
            var out = arena.allocate(capacity);
            long n = (long) Native.call(Native.DRAW_TEXT, Native.bytes(arena, data), (long) data.length, k, Native.seed(arena, seed), out, capacity);
            if (n < 0) {
                throw new SortitionException((int) n, null);
            }
            if (n == 0) {
                return List.of();
            }
            return List.of(new String(out.asSlice(0, n).toArray(JAVA_BYTE), StandardCharsets.UTF_8).split("\n", -1));
        }
    }

    /**
     * Draws {@code k} lines from text (one candidate per line; lines are trimmed and empty lines are not candidates).
     *
     * @param text the lines
     * @param k    how many to draw
     * @return the chosen lines, in the order drawn
     */
    public static List<String> drawText(String text, int k) {
        return drawText(text, k, null);
    }

    /**
     * Draws {@code k} lines from text, repeatably.
     *
     * @param text the lines
     * @param k    how many to draw
     * @param seed the seed
     * @return the chosen lines, in the order drawn
     */
    public static List<String> drawText(String text, int k, long seed) {
        return drawText(text, k, Long.valueOf(seed));
    }

    private static String drawData(DataFormat format, String text, int k, String path, Long seed) {
        count(k, "k");
        if (path.indexOf('\0') >= 0) {
            throw new SortitionException(SortitionException.INVALID, "path must not contain NUL");
        }
        var data = text.getBytes(StandardCharsets.UTF_8);
        try (var arena = Arena.ofConfined()) {
            var out = arena.allocate(ADDRESS);
            Native.check(Native.callInt(Native.DRAW_DATA, format.value, Native.bytes(arena, data), (long) data.length,
                    Native.cString(arena, path), k, Native.seed(arena, seed), out), true);
            return Native.takeString(out);
        }
    }

    /**
     * Draws {@code k} elements of an array inside JSON, YAML or TOML data.
     *
     * @param format the format of {@code text}
     * @param text   the data
     * @param k      how many to draw
     * @param path   where the array is ({@code "a.b.0"}); empty means the root
     * @return the chosen elements as a JSON array string
     */
    public static String drawData(DataFormat format, String text, int k, String path) {
        return drawData(format, text, k, path, null);
    }

    /**
     * Draws {@code k} elements of an array inside JSON, YAML or TOML data, repeatably.
     *
     * @param format the format of {@code text}
     * @param text   the data
     * @param k      how many to draw
     * @param path   where the array is ({@code "a.b.0"}); empty means the root
     * @param seed   the seed
     * @return the chosen elements as a JSON array string
     */
    public static String drawData(DataFormat format, String text, int k, String path, long seed) {
        return drawData(format, text, k, path, Long.valueOf(seed));
    }

    /**
     * Reads the key ID of a sealed token (to choose the key before opening it).
     *
     * @param token the sealed token
     * @return the key ID, 0 to 255
     */
    public static int sealKeyId(String token) {
        var data = token.getBytes(StandardCharsets.UTF_8);
        try (var arena = Arena.ofConfined()) {
            var out = arena.allocate(JAVA_BYTE);
            Native.check(Native.callInt(Native.SEAL_KEY_ID, Native.bytes(arena, data), (long) data.length, out), true);
            return out.get(JAVA_BYTE, 0) & 0xFF;
        }
    }

    /**
     * The ID of each candidate, in the order of {@code items}.
     *
     * @param items the candidates
     * @return the IDs: 64 lowercase hex characters each
     */
    public static List<String> itemIds(Items items) {
        try (var arena = Arena.ofConfined()) {
            var packed = items.pack(arena);
            var out = arena.allocate(Math.max(1, 32L * packed.count()));
            Native.check(Native.callInt(Native.ITEM_IDS, packed.data(), packed.lens(), packed.count(), out), false);
            var ids = new ArrayList<String>();
            for (int i = 0; i < packed.count(); i++) {
                ids.add(hex(out, i));
            }
            return ids;
        }
    }

    /**
     * The fingerprint of the set of candidates. It does not depend on the order of {@code items}:
     * publish it with the commitment, before the draw.
     *
     * @param items the candidates
     * @return 64 lowercase hex characters
     */
    public static String itemFingerprint(Items items) {
        try (var arena = Arena.ofConfined()) {
            var packed = items.pack(arena);
            var out = arena.allocate(32);
            Native.check(Native.callInt(Native.ITEM_FINGERPRINT, packed.data(), packed.lens(), packed.count(), out), false);
            return hex(out, 0);
        }
    }

    private static List<Pick> drawItems(Items items, int k, Long seed) {
        count(k, "k");
        try (var arena = Arena.ofConfined()) {
            var packed = items.pack(arena);
            var indexes = Native.ints(arena, k);
            var ids = arena.allocate(Math.max(1, 32L * k));
            Native.check(Native.callInt(Native.ITEMS_DRAW, packed.data(), packed.lens(), packed.count(), k,
                    Native.seed(arena, seed), indexes, ids), false);
            return picks(indexes, ids, k);
        }
    }

    /**
     * Draws {@code k} candidates without replacement. The same set of candidates and the same seed give the
     * same winners whatever the order of {@code items}.
     *
     * @param items the candidates
     * @param k     how many to draw
     * @return the winners, in the order drawn
     */
    public static List<Pick> drawItems(Items items, int k) {
        return drawItems(items, k, null);
    }

    /**
     * Draws {@code k} candidates without replacement, repeatably.
     *
     * @param items the candidates
     * @param k     how many to draw
     * @param seed  the seed
     * @return the winners, in the order drawn
     */
    public static List<Pick> drawItems(Items items, int k, long seed) {
        return drawItems(items, k, Long.valueOf(seed));
    }

    private static List<StageResult> drawStages(Items items, List<Stage> stages, Long seed) {
        long total = 0;
        for (var s : stages) {
            total += count(s.count(), "stage count");
        }
        if (total > Integer.MAX_VALUE) {
            throw new SortitionException(SortitionException.INVALID, "the stages add up to too many winners");
        }
        var picks = drawItems(items, (int) total, seed);
        var out = new ArrayList<StageResult>();
        int offset = 0;
        for (var s : stages) {
            out.add(new StageResult(s.name(), s.count(), List.copyOf(picks.subList(offset, offset + s.count()))));
            offset += s.count();
        }
        return out;
    }

    /**
     * Draws prize stages in order without giving anyone two prizes. It draws the total once and cuts it up in
     * the order of {@code stages}, which has the same probabilities as drawing stage by stage.
     *
     * @param items  the candidates
     * @param stages the plan, in the order to draw
     * @return the winners of each stage
     */
    public static List<StageResult> drawStages(Items items, List<Stage> stages) {
        return drawStages(items, stages, null);
    }

    /**
     * Draws prize stages in order, repeatably.
     *
     * @param items  the candidates
     * @param stages the plan, in the order to draw
     * @param seed   the seed
     * @return the winners of each stage
     */
    public static List<StageResult> drawStages(Items items, List<Stage> stages, long seed) {
        return drawStages(items, stages, Long.valueOf(seed));
    }

    // ---- shared with the sessions ----

    static String hex(MemorySegment ids, int i) {
        return HexFormat.of().formatHex(ids.asSlice(32L * i, 32).toArray(JAVA_BYTE));
    }

    static List<Pick> picks(MemorySegment indexes, MemorySegment ids, int k) {
        var out = new ArrayList<Pick>(k);
        for (int i = 0; i < k; i++) {
            out.add(new Pick(indexes.getAtIndex(JAVA_INT, i), hex(ids, i)));
        }
        return List.copyOf(out);
    }
}
