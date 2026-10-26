package io.github.smallgearlab.sortition;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.nio.charset.StandardCharsets;
import java.util.List;
import java.util.Objects;

/**
 * A session over a fixed set of candidates. Close it when done. Winners are {@link Pick}s; after a restore,
 * {@link Pick#index()} is a position in the list that was passed again.
 */
public final class ItemSession extends SessionBase {
    private static final Fns FNS = new Fns(Native.ITEM_SESSION_REMAINING, Native.ITEM_SESSION_DRAWN_COUNT,
            Native.ITEM_SESSION_REVISION, Native.ITEM_SESSION_ID, Native.ITEM_SESSION_SAVE, Native.ITEM_SESSION_SEAL,
            Native.ITEM_SESSION_FREE);

    private ItemSession(MemorySegment handle) {
        super(FNS, handle);
    }

    private static ItemSession create(Items items, Long seed) {
        try (var arena = Arena.ofConfined()) {
            var packed = items.pack(arena);
            var seedPtr = Native.seed(arena, seed);
            return new ItemSession(newHandle(arena,
                    out -> Native.callInt(Native.ITEM_SESSION_NEW, packed.data(), packed.lens(), packed.count(), seedPtr, out), false));
        }
    }

    /**
     * Creates a session over a fixed set of candidates, with the operating system's randomness.
     *
     * @param items the candidates
     * @return the session
     */
    public static ItemSession create(Items items) {
        return create(items, null);
    }

    /**
     * Creates a repeatable session over a fixed set of candidates.
     *
     * @param items the candidates
     * @param seed  the seed
     * @return the session
     */
    public static ItemSession create(Items items, long seed) {
        return create(items, Long.valueOf(seed));
    }

    /**
     * Restores a session from {@link #save()} and the same set of candidates (the order may differ; another
     * set gives {@link SortitionException#STATE}).
     *
     * @param state the saved state
     * @param items the candidates
     * @return the session
     */
    public static ItemSession load(String state, Items items) {
        var data = state.getBytes(StandardCharsets.UTF_8);
        try (var arena = Arena.ofConfined()) {
            var text = Native.bytes(arena, data);
            var packed = items.pack(arena);
            return new ItemSession(newHandle(arena,
                    out -> Native.callInt(Native.ITEM_SESSION_LOAD, text, (long) data.length, packed.data(), packed.lens(), packed.count(), out), true));
        }
    }

    /**
     * Restores a session from a token made by {@code seal} and the same set of candidates.
     *
     * @param token the sealed token
     * @param key   the secret key, 32 bytes
     * @param items the candidates
     * @return the session
     */
    public static ItemSession unseal(String token, byte[] key, Items items) {
        checkKey(key);
        var data = token.getBytes(StandardCharsets.UTF_8);
        try (var arena = Arena.ofConfined()) {
            var text = Native.bytes(arena, data);
            var keyPtr = Native.bytes(arena, key);
            var packed = items.pack(arena);
            return new ItemSession(newHandle(arena,
                    out -> Native.callInt(Native.ITEM_SESSION_UNSEAL, text, (long) data.length, keyPtr, packed.data(), packed.lens(), packed.count(), out), true));
        }
    }

    private List<Pick> drawWith(int k, String key) {
        if (k < 0) {
            throw new SortitionException(SortitionException.INVALID, "k must not be negative");
        }
        try (var arena = Arena.ofConfined()) {
            var indexes = Native.ints(arena, k);
            var ids = arena.allocate(Math.max(1, 32L * k));
            Native.check(Native.callInt(Native.ITEM_SESSION_DRAW, handle(), k, key(arena, key), indexes, ids, MemorySegment.NULL), false);
            return Lottery.picks(indexes, ids, k);
        }
    }

    /**
     * Draws the next {@code k} candidates. More than {@link #remaining()} gives
     * {@link SortitionException#INVALID} and leaves the state unchanged.
     *
     * @param k how many to draw
     * @return the winners, in the order drawn
     */
    public List<Pick> draw(int k) {
        return drawWith(k, null);
    }

    /**
     * Draws with an idempotency key, like {@link Session#draw(int, String)}.
     *
     * @param k   how many to draw
     * @param key an idempotency key, 1 to 128 bytes
     * @return the winners, in the order drawn
     */
    public List<Pick> draw(int k, String key) {
        return drawWith(k, Objects.requireNonNull(key, "key"));
    }
}
