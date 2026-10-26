package io.github.smallgearlab.sortition;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collection;
import java.util.List;

import static java.lang.foreign.ValueLayout.JAVA_BYTE;
import static java.lang.foreign.ValueLayout.JAVA_LONG;

/**
 * A list of candidates: each one is a string (its UTF-8 bytes) or any bytes. Identical entries stay
 * separate candidates. The positions in {@link Pick#index()} refer to this list.
 */
public final class Items {
    private final List<byte[]> chunks;

    private Items(List<byte[]> chunks) {
        this.chunks = chunks;
    }

    /**
     * Candidates from strings.
     *
     * @param items the candidates, in your order
     * @return the list
     */
    public static Items of(String... items) {
        return of(Arrays.asList(items));
    }

    /**
     * Candidates from strings.
     *
     * @param items the candidates, in your order
     * @return the list
     */
    public static Items of(Collection<String> items) {
        var out = new ArrayList<byte[]>(items.size());
        for (var s : items) {
            out.add(s.getBytes(StandardCharsets.UTF_8));
        }
        return new Items(out);
    }

    /**
     * Candidates from bytes.
     *
     * @param items the candidates, in your order (copied)
     * @return the list
     */
    public static Items ofBytes(Collection<byte[]> items) {
        var out = new ArrayList<byte[]>(items.size());
        for (var b : items) {
            out.add(b.clone());
        }
        return new Items(out);
    }

    /**
     * The number of candidates.
     *
     * @return the count
     */
    public int size() {
        return chunks.size();
    }

    /** The candidates as one buffer and an array of lengths ({@code size_t}). */
    record Packed(MemorySegment data, MemorySegment lens, long count) {}

    Packed pack(Arena arena) {
        long total = 0;
        for (var c : chunks) {
            total += c.length;
        }
        var data = arena.allocate(Math.max(1, total));
        var lens = arena.allocate(JAVA_LONG, Math.max(1, chunks.size()));
        long offset = 0;
        for (int i = 0; i < chunks.size(); i++) {
            var c = chunks.get(i);
            MemorySegment.copy(c, 0, data, JAVA_BYTE, offset, c.length);
            lens.setAtIndex(JAVA_LONG, i, c.length);
            offset += c.length;
        }
        return new Packed(data, lens, chunks.size());
    }
}
