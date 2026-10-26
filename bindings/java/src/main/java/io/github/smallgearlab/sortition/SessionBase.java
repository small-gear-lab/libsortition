package io.github.smallgearlab.sortition;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.invoke.MethodHandle;
import java.lang.ref.Cleaner;
import java.nio.charset.StandardCharsets;

import static java.lang.foreign.ValueLayout.ADDRESS;

/**
 * What both kinds of session share. A session is not thread-safe: use one from one thread at a time.
 */
abstract class SessionBase implements AutoCloseable {
    /** The C functions of one kind of session. */
    record Fns(MethodHandle remaining, MethodHandle drawnCount, MethodHandle revision, MethodHandle id,
               MethodHandle save, MethodHandle seal, MethodHandle free) {}

    private static final Cleaner CLEANER = Cleaner.create();

    /** The native handle. It is freed by {@link #close()}, or by the cleaner if a session was forgotten. */
    private static final class State implements Runnable {
        private MemorySegment handle;
        private final MethodHandle free;

        State(MemorySegment handle, MethodHandle free) {
            this.handle = handle;
            this.free = free;
        }

        @Override
        public synchronized void run() {
            if (handle != null) {
                var h = handle;
                handle = null;
                Native.call(free, h);
            }
        }

        synchronized MemorySegment handle() {
            if (handle == null) {
                throw new IllegalStateException("session is closed");
            }
            return handle;
        }
    }

    private final Fns fns;
    private final State state;
    private final Cleaner.Cleanable cleanable;

    SessionBase(Fns fns, MemorySegment handle) {
        this.fns = fns;
        this.state = new State(handle, fns.free());
        this.cleanable = CLEANER.register(this, state);
    }

    final MemorySegment handle() {
        return state.handle();
    }

    /** Frees the session. It is safe to call it more than once. */
    @Override
    public final void close() {
        cleanable.clean();
    }

    /**
     * How many are not drawn yet.
     *
     * @return the count
     */
    public final long remaining() {
        return Integer.toUnsignedLong(Native.callInt(fns.remaining(), handle()));
    }

    /**
     * How many were drawn.
     *
     * @return the count
     */
    public final long drawnCount() {
        return Integer.toUnsignedLong(Native.callInt(fns.drawnCount(), handle()));
    }

    /**
     * The number of draws that changed the state. A store can update a record only when the revision is
     * the one it read, so that two updates made from the same state do not overwrite each other.
     *
     * @return the revision
     */
    public final long revision() {
        return (long) Native.call(fns.revision(), handle());
    }

    /**
     * The session's identifier (32 lowercase hex characters), taken from the operating system when it was created.
     *
     * @return the identifier
     */
    public final String id() {
        try (var arena = Arena.ofConfined()) {
            var out = arena.allocate(33);
            Native.check(Native.callInt(fns.id(), handle(), out), false);
            return out.getString(0, StandardCharsets.UTF_8);
        }
    }

    /**
     * The state as JSON. It contains the internal state of the random generator: whoever has it can predict
     * the next winners, so do not give it to a party you do not trust. {@link #seal} it instead.
     *
     * @return the state
     */
    public final String save() {
        try (var arena = Arena.ofConfined()) {
            var out = arena.allocate(ADDRESS);
            Native.check(Native.callInt(fns.save(), handle(), out), false);
            return Native.takeString(out);
        }
    }

    /**
     * The state as a token that only the holder of {@code key} can open, and that cannot be changed without
     * notice (XChaCha20-Poly1305).
     *
     * @param key   a secret key of exactly 32 bytes (make it with a cryptographic random generator)
     * @param keyId a label for rotating keys, 0 to 255 (read it with {@link Lottery#sealKeyId})
     * @return the token
     */
    public final String seal(byte[] key, int keyId) {
        checkKey(key);
        if (keyId < 0 || keyId > 255) {
            throw new SortitionException(SortitionException.INVALID, "keyId must be in 0..255");
        }
        try (var arena = Arena.ofConfined()) {
            var out = arena.allocate(ADDRESS);
            Native.check(Native.callInt(fns.seal(), handle(), Native.bytes(arena, key), (byte) keyId, out), false);
            return Native.takeString(out);
        }
    }

    /**
     * The state as a sealed token with key ID 0.
     *
     * @param key a secret key of exactly 32 bytes
     * @return the token
     */
    public final String seal(byte[] key) {
        return seal(key, 0);
    }

    static void checkKey(byte[] key) {
        if (key.length != 32) {
            throw new SortitionException(SortitionException.INVALID, "key must be exactly 32 bytes");
        }
    }

    /** A NUL-terminated idempotency key ({@code null}: no key). */
    static MemorySegment key(Arena arena, String key) {
        if (key != null && key.indexOf('\0') >= 0) {
            throw new SortitionException(SortitionException.INVALID, "key must not contain NUL");
        }
        return Native.cString(arena, key);
    }

    static MemorySegment newHandle(Arena arena, java.util.function.Function<MemorySegment, Integer> create, boolean detailed) {
        var out = arena.allocate(ADDRESS);
        Native.check(create.apply(out), detailed);
        return out.get(ADDRESS, 0);
    }
}
