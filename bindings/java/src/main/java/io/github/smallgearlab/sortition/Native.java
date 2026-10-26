package io.github.smallgearlab.sortition;

import java.lang.foreign.Arena;
import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.Linker;
import java.lang.foreign.MemoryLayout;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.SymbolLookup;
import java.lang.invoke.MethodHandle;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;

import static java.lang.foreign.ValueLayout.ADDRESS;
import static java.lang.foreign.ValueLayout.JAVA_BYTE;
import static java.lang.foreign.ValueLayout.JAVA_INT;
import static java.lang.foreign.ValueLayout.JAVA_LONG;

/**
 * The C ABI of the core (core/include/sortition.h), called through {@code java.lang.foreign}.
 * A pointer is {@code ADDRESS}, {@code size_t} and {@code uint64_t} are {@code JAVA_LONG}, and
 * {@code uint32_t} and {@code int32_t} are {@code JAVA_INT}.
 */
final class Native {
    private static final Linker LINKER = Linker.nativeLinker();
    private static final SymbolLookup LOOKUP = open();

    /**
     * Finds the shared library: {@code LIBSORTITION_PATH} (a file), then {@code target/release} in
     * the working directory or any directory above it, then the system's library search path.
     */
    private static SymbolLookup open() {
        var arena = Arena.global();
        var env = System.getenv("LIBSORTITION_PATH");
        if (env != null && !env.isEmpty()) {
            return SymbolLookup.libraryLookup(Path.of(env), arena);
        }
        var file = System.mapLibraryName("sortition");
        for (var dir = Path.of("").toAbsolutePath(); dir != null; dir = dir.getParent()) {
            var candidate = dir.resolve("target").resolve("release").resolve(file);
            if (Files.isRegularFile(candidate)) {
                return SymbolLookup.libraryLookup(candidate, arena);
            }
        }
        return SymbolLookup.libraryLookup(file, arena);
    }

    private static MethodHandle fn(String name, MemoryLayout result, MemoryLayout... args) {
        var address = LOOKUP.find(name).orElseThrow(() -> new UnsatisfiedLinkError("sortition: missing symbol " + name));
        return LINKER.downcallHandle(address, result == null ? FunctionDescriptor.ofVoid(args) : FunctionDescriptor.of(result, args));
    }

    // Plain functions
    static final MethodHandle VERSION = fn("sortition_version", ADDRESS);
    static final MethodHandle LAST_ERROR = fn("sortition_last_error", ADDRESS);
    static final MethodHandle FREE = fn("sortition_free", null, ADDRESS);
    static final MethodHandle SEED_FROM_TEXT = fn("sortition_seed_from_text", JAVA_INT, ADDRESS, JAVA_LONG, ADDRESS, ADDRESS);
    static final MethodHandle DRAW = fn("sortition_draw", JAVA_INT, JAVA_INT, JAVA_INT, ADDRESS, ADDRESS);
    static final MethodHandle DRAW_TEXT = fn("sortition_draw_text", JAVA_LONG, ADDRESS, JAVA_LONG, JAVA_INT, ADDRESS, ADDRESS, JAVA_LONG);
    static final MethodHandle DRAW_DATA = fn("sortition_draw_data", JAVA_INT, JAVA_INT, ADDRESS, JAVA_LONG, ADDRESS, JAVA_INT, ADDRESS, ADDRESS);
    static final MethodHandle SEAL_KEY_ID = fn("sortition_seal_key_id", JAVA_INT, ADDRESS, JAVA_LONG, ADDRESS);

    // Candidates
    static final MethodHandle ITEM_IDS = fn("sortition_item_ids", JAVA_INT, ADDRESS, ADDRESS, JAVA_LONG, ADDRESS);
    static final MethodHandle ITEM_FINGERPRINT = fn("sortition_item_fingerprint", JAVA_INT, ADDRESS, ADDRESS, JAVA_LONG, ADDRESS);
    static final MethodHandle ITEMS_DRAW = fn("sortition_items_draw", JAVA_INT, ADDRESS, ADDRESS, JAVA_LONG, JAVA_INT, ADDRESS, ADDRESS, ADDRESS);

    // Number sessions
    static final MethodHandle SESSION_NEW = fn("sortition_session_new", JAVA_INT, JAVA_INT, ADDRESS, ADDRESS);
    static final MethodHandle SESSION_LOAD = fn("sortition_session_load", JAVA_INT, ADDRESS, JAVA_LONG, ADDRESS);
    static final MethodHandle SESSION_UNSEAL = fn("sortition_session_unseal", JAVA_INT, ADDRESS, JAVA_LONG, ADDRESS, ADDRESS);
    static final MethodHandle SESSION_DRAW = fn("sortition_session_draw_keyed", JAVA_INT, ADDRESS, JAVA_INT, ADDRESS, ADDRESS, ADDRESS);
    static final MethodHandle SESSION_REMAINING = fn("sortition_session_remaining", JAVA_INT, ADDRESS);
    static final MethodHandle SESSION_DRAWN_COUNT = fn("sortition_session_drawn_count", JAVA_INT, ADDRESS);
    static final MethodHandle SESSION_REVISION = fn("sortition_session_revision", JAVA_LONG, ADDRESS);
    static final MethodHandle SESSION_ID = fn("sortition_session_id", JAVA_INT, ADDRESS, ADDRESS);
    static final MethodHandle SESSION_SAVE = fn("sortition_session_save", JAVA_INT, ADDRESS, ADDRESS);
    static final MethodHandle SESSION_SEAL = fn("sortition_session_seal", JAVA_INT, ADDRESS, ADDRESS, JAVA_BYTE, ADDRESS);
    static final MethodHandle SESSION_FREE = fn("sortition_session_free", null, ADDRESS);

    // Candidate sessions
    static final MethodHandle ITEM_SESSION_NEW = fn("sortition_item_session_new", JAVA_INT, ADDRESS, ADDRESS, JAVA_LONG, ADDRESS, ADDRESS);
    static final MethodHandle ITEM_SESSION_LOAD = fn("sortition_item_session_load", JAVA_INT, ADDRESS, JAVA_LONG, ADDRESS, ADDRESS, JAVA_LONG, ADDRESS);
    static final MethodHandle ITEM_SESSION_UNSEAL = fn("sortition_item_session_unseal", JAVA_INT, ADDRESS, JAVA_LONG, ADDRESS, ADDRESS, ADDRESS, JAVA_LONG, ADDRESS);
    static final MethodHandle ITEM_SESSION_DRAW = fn("sortition_item_session_draw_keyed", JAVA_INT, ADDRESS, JAVA_INT, ADDRESS, ADDRESS, ADDRESS, ADDRESS);
    static final MethodHandle ITEM_SESSION_REMAINING = fn("sortition_item_session_remaining", JAVA_INT, ADDRESS);
    static final MethodHandle ITEM_SESSION_DRAWN_COUNT = fn("sortition_item_session_drawn_count", JAVA_INT, ADDRESS);
    static final MethodHandle ITEM_SESSION_REVISION = fn("sortition_item_session_revision", JAVA_LONG, ADDRESS);
    static final MethodHandle ITEM_SESSION_ID = fn("sortition_item_session_id", JAVA_INT, ADDRESS, ADDRESS);
    static final MethodHandle ITEM_SESSION_SAVE = fn("sortition_item_session_save", JAVA_INT, ADDRESS, ADDRESS);
    static final MethodHandle ITEM_SESSION_SEAL = fn("sortition_item_session_seal", JAVA_INT, ADDRESS, ADDRESS, JAVA_BYTE, ADDRESS);
    static final MethodHandle ITEM_SESSION_FREE = fn("sortition_item_session_free", null, ADDRESS);

    private Native() {}

    /** Calls a handle; the arguments must already have the exact types of the C declaration. */
    static Object call(MethodHandle handle, Object... args) {
        try {
            return handle.invokeWithArguments(args);
        } catch (RuntimeException | Error e) {
            throw e;
        } catch (Throwable e) {
            throw new IllegalStateException(e);
        }
    }

    static int callInt(MethodHandle handle, Object... args) {
        return (int) call(handle, args);
    }

    /** Throws a {@link SortitionException} for a non-zero code. {@code detailed} reads the thread's last error. */
    static void check(int rc, boolean detailed) {
        if (rc != 0) {
            throw new SortitionException(rc, detailed ? lastError() : null);
        }
    }

    static String lastError() {
        var p = (MemorySegment) call(LAST_ERROR);
        return p.equals(MemorySegment.NULL) ? null : readCString(p);
    }

    static String readCString(MemorySegment p) {
        return p.reinterpret(Long.MAX_VALUE).getString(0, StandardCharsets.UTF_8);
    }

    /** Reads the string that the core wrote through {@code out} (a {@code char **}), and frees it. */
    static String takeString(MemorySegment out) {
        var p = out.get(ADDRESS, 0);
        try {
            return readCString(p);
        } finally {
            call(FREE, p);
        }
    }

    static MemorySegment bytes(Arena arena, byte[] data) {
        var seg = arena.allocate(Math.max(1, data.length));
        MemorySegment.copy(data, 0, seg, JAVA_BYTE, 0, data.length);
        return seg;
    }

    /** A NUL-terminated UTF-8 copy ({@code null} stays {@link MemorySegment#NULL}). */
    static MemorySegment cString(Arena arena, String s) {
        return s == null ? MemorySegment.NULL : arena.allocateFrom(s, StandardCharsets.UTF_8);
    }

    /** A pointer to a {@code uint64_t} seed, or NULL for no seed. */
    static MemorySegment seed(Arena arena, Long seed) {
        if (seed == null) {
            return MemorySegment.NULL;
        }
        var seg = arena.allocate(JAVA_LONG);
        seg.set(JAVA_LONG, 0, seed);
        return seg;
    }

    /** A buffer for {@code count} unsigned 32-bit numbers. */
    static MemorySegment ints(Arena arena, int count) {
        return arena.allocate(JAVA_INT, Math.max(1, count));
    }

    static int[] readInts(MemorySegment seg, int count) {
        return seg.asSlice(0, 4L * count).toArray(JAVA_INT);
    }
}
