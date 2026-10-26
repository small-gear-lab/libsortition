package io.github.smallgearlab.sortition;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.nio.charset.StandardCharsets;

/**
 * A session draws without replacement from 0..n-1 and continues where it stopped. Drawing in several
 * steps, with a save and a restore in between, gives the same order as one draw. Close it when done.
 */
public final class Session extends SessionBase {
    private static final Fns FNS = new Fns(Native.SESSION_REMAINING, Native.SESSION_DRAWN_COUNT, Native.SESSION_REVISION,
            Native.SESSION_ID, Native.SESSION_SAVE, Native.SESSION_SEAL, Native.SESSION_FREE);

    private Session(MemorySegment handle) {
        super(FNS, handle);
    }

    private static Session create(long n, Long seed) {
        int count = Lottery.u32(n, "n");
        try (var arena = Arena.ofConfined()) {
            var seedPtr = Native.seed(arena, seed);
            return new Session(newHandle(arena, out -> Native.callInt(Native.SESSION_NEW, count, seedPtr, out), false));
        }
    }

    /**
     * Creates a session over 0..n-1 that runs a cryptographic stream seeded from the operating system.
     *
     * @param n how many numbers there are
     * @return the session
     */
    public static Session create(long n) {
        return create(n, null);
    }

    /**
     * Creates a repeatable session over 0..n-1.
     *
     * @param n    how many numbers there are
     * @param seed the seed
     * @return the session
     */
    public static Session create(long n, long seed) {
        return create(n, Long.valueOf(seed));
    }

    /**
     * Restores a session from {@link #save()}.
     *
     * @param state the saved state
     * @return the session
     */
    public static Session load(String state) {
        var data = state.getBytes(StandardCharsets.UTF_8);
        try (var arena = Arena.ofConfined()) {
            var text = Native.bytes(arena, data);
            return new Session(newHandle(arena, out -> Native.callInt(Native.SESSION_LOAD, text, (long) data.length, out), true));
        }
    }

    /**
     * Restores a session from a token made by {@code seal}. A malformed token, a wrong key and a changed token
     * all give {@link SortitionException#STATE}.
     *
     * @param token the sealed token
     * @param key   the secret key, 32 bytes
     * @return the session
     */
    public static Session unseal(String token, byte[] key) {
        checkKey(key);
        var data = token.getBytes(StandardCharsets.UTF_8);
        try (var arena = Arena.ofConfined()) {
            var text = Native.bytes(arena, data);
            var keyPtr = Native.bytes(arena, key);
            return new Session(newHandle(arena, out -> Native.callInt(Native.SESSION_UNSEAL, text, (long) data.length, keyPtr, out), true));
        }
    }

    private int[] drawWith(int k, String key) {
        if (k < 0) {
            throw new SortitionException(SortitionException.INVALID, "k must not be negative");
        }
        try (var arena = Arena.ofConfined()) {
            var out = Native.ints(arena, k);
            Native.check(Native.callInt(Native.SESSION_DRAW, handle(), k, key(arena, key), out, MemorySegment.NULL), false);
            return Native.readInts(out, k);
        }
    }

    /**
     * Draws the next {@code k} numbers. More than {@link #remaining()} gives {@link SortitionException#INVALID}
     * and leaves the state unchanged.
     *
     * @param k how many to draw
     * @return the numbers, in the order drawn
     */
    public int[] draw(int k) {
        return drawWith(k, null);
    }

    /**
     * Draws with an idempotency key. The same key and the same {@code k} again does not change the state and
     * returns the earlier result; the same key with another {@code k} gives {@link SortitionException#CONFLICT}.
     *
     * @param k   how many to draw
     * @param key an idempotency key, 1 to 128 bytes
     * @return the numbers, in the order drawn
     */
    public int[] draw(int k, String key) {
        return drawWith(k, java.util.Objects.requireNonNull(key, "key"));
    }
}
