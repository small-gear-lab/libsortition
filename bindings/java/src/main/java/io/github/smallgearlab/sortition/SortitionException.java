package io.github.smallgearlab.sortition;

/**
 * A failure reported by the core. It is an {@link IllegalArgumentException}, so a caller that does not
 * care which input was wrong can catch that.
 */
public final class SortitionException extends IllegalArgumentException {
    private static final long serialVersionUID = 1L;

    /** An argument is out of range (k larger than the number of candidates, an empty key, ...). */
    public static final int INVALID = -1;
    /** An invalid saved state or sealed token (or a different set of candidates). */
    public static final int STATE = -8;
    /** An idempotency key was used with a different request. */
    public static final int CONFLICT = -9;

    /** The negative error code of the core. */
    private final int code;

    SortitionException(int code, String detail) {
        super(detail != null && !detail.isEmpty() ? detail + " (code " + code + ")" : "sortition error code " + code);
        this.code = code;
    }

    /** The negative error code of the core (see docs/SPEC.md); {@link #INVALID}, {@link #STATE} and {@link #CONFLICT} are the common ones. */
    public int code() {
        return code;
    }
}
