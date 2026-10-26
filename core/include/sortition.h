/* libsortition: fair, repeatable and verifiable random lotteries (draws without replacement).
 *
 * How to use this header:
 *  - Functions return SORTITION_OK (0) or a negative SORTITION_ERR_* code, unless they say otherwise.
 *    After a failure of a function that has a detailed message, sortition_last_error() gives it
 *    (it is kept per thread and is valid until the next such call on the thread; do not free it).
 *  - Strings and buffers that the library allocates (the JSON text, the sealed token, ...) are freed
 *    with sortition_free. A NULL pointer is always ignored by the free functions.
 *  - A session handle must not be used by two threads at the same time.
 *  - Where a function takes `const uint64_t *seed`, NULL means "use the operating system's randomness"
 *    (the result is then not repeatable); otherwise the result depends only on the seed and the input.
 *  - Candidates are passed as one buffer plus the length of each candidate; any bytes are allowed.
 *
 * Which functions to use: when the winners must not depend on the order of your list, use the
 * candidate functions (sortition_items_draw, sortition_item_*). sortition_draw, sortition_draw_text and
 * sortition_draw_data return positions in the list you gave.
 *
 * The rules of the draw (how a seed becomes winners) are written down in docs/SPEC.md. */
#ifndef SORTITION_H
#define SORTITION_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define SORTITION_OK           0
#define SORTITION_ERR_INVALID (-1)
#define SORTITION_ERR_RNG     (-2)
#define SORTITION_ERR_UTF8    (-3)
#define SORTITION_ERR_PARSE   (-4) /* syntax error (also YAML aliases and the like) */
#define SORTITION_ERR_PATH    (-5) /* the path was not found, or it is not an array */
#define SORTITION_ERR_FORMAT  (-6) /* unknown format, or disabled in this build */
#define SORTITION_ERR_INTERNAL (-7) /* internal error (a panic was caught) */
#define SORTITION_ERR_STATE   (-8) /* an invalid saved session state, or sealed token */
#define SORTITION_ERR_CONFLICT (-9) /* the idempotency key was used with a different request */

/* Draws k distinct numbers from 0..n-1, without replacement, and writes them to out[0..k] in the
 * order drawn. With a seed the result is repeatable (xoshiro256**). */
int32_t sortition_draw(uint32_t n, uint32_t k, const uint64_t *seed, uint32_t *out);

/* Draws k lines from UTF-8 text (one candidate per line) and writes the chosen lines to out,
 * joined by '\n' (no trailing newline or NUL).
 * Each line is trimmed of surrounding white space, and empty lines are not candidates. Identical
 * lines are separate candidates.
 * Returns the number of bytes written, or a negative error code.
 * The output is never longer than the input, so an out buffer with out_cap >= len is enough. */
int64_t sortition_draw_text(const uint8_t *text, size_t len, uint32_t k,
                            const uint64_t *seed, uint8_t *out, size_t out_cap);

#define SORTITION_FORMAT_JSON 0
#define SORTITION_FORMAT_YAML 1
#define SORTITION_FORMAT_TOML 2

/* Draws k elements from an array inside structured data (JSON, YAML or TOML) and returns the chosen
 * elements as a JSON array string in *out.
 *
 * path says where the array is ("a.b.0": dot-separated, NUL-terminated UTF-8). NULL or "" means the
 * root. TOML needs a path, because its root is always a table.
 * On success *out is a NUL-terminated JSON string (free it with sortition_free); on failure it is NULL.
 * Elements are normalized to JSON (TOML date-times become strings; YAML aliases, custom tags and
 * multiple documents are rejected).
 * sortition_last_error() has the details of a failure. */
int32_t sortition_draw_data(int32_t format, const uint8_t *text, size_t len,
                            const char *path, uint32_t k, const uint64_t *seed,
                            char **out);

/* Frees a string that the library returned (NULL is ignored). */
void sortition_free(char *s);

/* The detailed message of the last failure on this thread. Valid until the next call that can set it
 * on the same thread; do not free it. */
const char *sortition_last_error(void);

/* ---- Sessions ----
 * A session draws without replacement from 0..n-1 and continues where it stopped each time you call
 * it. Its state can be saved and restored as JSON. Drawing in several steps, with a save and a
 * restore in between, gives the same order as one draw (with a seed: the same as
 * sortition_draw(n, k1 + k2 + ..., seed)). */
typedef struct sortition_session sortition_session;

/* Creates a session. With a NULL seed, it runs ChaCha20 with a key taken from the operating system
 * when it is created (a cryptographic random stream); the key and the read position are part of the
 * saved state, so drawing from the same state gives the same result (sending an old state again does
 * not draw again). With a seed it runs xoshiro256**, which is repeatable but not cryptographic.
 * Warning: the output of sortition_session_save contains the internal state of the random generator.
 * Whoever has it can predict the next winners, so do not give it to a party you do not trust (seal it
 * instead, see below).
 * Free *out with sortition_session_free. */
int32_t sortition_session_new(uint32_t n, const uint64_t *seed, sortition_session **out);

/* Draws the next k numbers and writes them to out[0..k]. If k is more than what is left, it returns
 * SORTITION_ERR_INVALID and the state does not change. */
int32_t sortition_session_draw(sortition_session *s, uint32_t k, uint32_t *out);

uint32_t sortition_session_remaining(const sortition_session *s);   /* how many are not drawn yet */
uint32_t sortition_session_drawn_count(const sortition_session *s); /* how many were drawn */

/* Returns the state as a JSON string in *out (free it with sortition_free). The state includes the
 * results drawn so far ("drawn"), so keeping it keeps the history too. */
int32_t sortition_session_save(const sortition_session *s, char **out);

/* Restores a session from the output of sortition_session_save. An invalid or inconsistent state
 * gives SORTITION_ERR_STATE (sortition_last_error has the details). */
int32_t sortition_session_load(const uint8_t *text, size_t len, sortition_session **out);

/* Draws the next k numbers with an idempotency key. key is NUL-terminated UTF-8 (1 to 128 bytes); NULL
 * means no key (the same as sortition_session_draw). Sending the same key and the same k again does not
 * change the state: the earlier result is written to out and *replayed is set to 1. The same key with
 * another k gives SORTITION_ERR_CONFLICT. k == 0 does nothing and records no key. A failed call leaves
 * nothing behind. replayed may be NULL.
 *
 * The random stream and the records are part of the state, so overlapping requests never draw twice:
 *  - the same key sent to a state after the draw gives the earlier result (replayed = 1);
 *  - the same key and k sent to an old state from before the draw gives the same result and the same
 *    state. */
int32_t sortition_session_draw_keyed(sortition_session *s, uint32_t k, const char *key,
                                     uint32_t *out, int32_t *replayed);

/* The number of draws that changed the state. A store can update a record only when the revision is
 * the one that was read, so that two updates made from the same state do not overwrite each other. */
uint64_t sortition_session_revision(const sortition_session *s);

/* Writes the session id (32 lowercase hex characters and a NUL) to out[0..33]. It is a value taken from
 * the operating system when the session was created. */
int32_t sortition_session_id(const sortition_session *s, uint8_t *out);

/* ---- Sealing a state ----
 * The output of sortition_session_save contains the internal state of the random generator: whoever
 * has it can predict later winners and can forge a state. To keep a state with a party you do not
 * trust (a browser, for example), give it a sealed token instead. It uses authenticated encryption
 * (XChaCha20-Poly1305): only the holder of the key can open it, and changing a single bit makes it
 * unopenable.
 *
 * A token is an ASCII string that starts with "sortition1." (base64url, no padding). key is a 32-byte
 * secret key (make it with a cryptographic random generator). key_id (0 to 255) is a label for rotating
 * keys: read it with sortition_seal_key_id, choose the key for it, and then open the token.
 * A malformed token, a wrong key and tampering all give SORTITION_ERR_STATE (they are not told apart). */

/* Writes the key id of a token to *out_key_id (SORTITION_ERR_STATE if the token is malformed). */
int32_t sortition_seal_key_id(const uint8_t *token, size_t len, uint8_t *out_key_id);

/* Returns a sealed token of the state in *out (free it with sortition_free). */
int32_t sortition_session_seal(const sortition_session *s, const uint8_t *key, uint8_t key_id, char **out);
int32_t sortition_session_unseal(const uint8_t *token, size_t len, const uint8_t *key,
                                 sortition_session **out);

void sortition_session_free(sortition_session *s); /* NULL is ignored */

/* ---- Drawing candidates (arbitrary bytes) ----
 * Each candidate becomes an ID that does not depend on its position (32 bytes of SHA-256). The IDs are
 * sorted in ascending order and the draw works on that order, so the same set of candidates and the
 * same seed give the same winners whatever the order of the input.
 *
 * ID = SHA-256("libsortition/candidate/v1\0" || bytes || occurrence). The occurrence is how many
 * candidates with exactly the same bytes came before it in the input (counting from 0), as 8 bytes in
 * little-endian order. Identical entries stay separate candidates and every ID is different.
 *
 * Candidates are passed as one concatenated buffer data plus the length of each one, lens[0..count].
 * data must be readable for the sum of the lengths (a candidate may be empty). */

/* Writes the ID of each candidate, in the order of the input, to out_ids[0..count*32]. */
int32_t sortition_item_ids(const uint8_t *data, const size_t *lens, size_t count, uint8_t *out_ids);

/* Writes the fingerprint of the set of candidates (32 bytes): the SHA-256 of the sorted IDs. It does not
 * depend on the order of the input. Publish it with the commitment before the draw, so that anyone can
 * check afterwards that the candidates were the same. */
int32_t sortition_item_fingerprint(const uint8_t *data, const size_t *lens, size_t count,
                                   uint8_t *out_fingerprint);

/* Draws k candidates once. Writes the position of each winner in the input to out_index[0..k] and
 * the ID to out_ids[0..k*32] (either may be NULL, and then it is not written). k > count gives
 * SORTITION_ERR_INVALID. */
int32_t sortition_items_draw(const uint8_t *data, const size_t *lens, size_t count, uint32_t k,
                             const uint64_t *seed, uint32_t *out_index, uint8_t *out_ids);

/* A session over a fixed set of candidates, with save and restore. The saved state has the IDs of the
 * winners and the fingerprint of the set. To restore it, pass the same set again (the order may
 * differ); another set gives SORTITION_ERR_STATE. After a restore, out_index is a position in the
 * array you passed again. A session handle must not be used by two threads at the same time. */
typedef struct sortition_item_session sortition_item_session;

int32_t sortition_item_session_new(const uint8_t *data, const size_t *lens, size_t count,
                                   const uint64_t *seed, sortition_item_session **out);
/* Draws the next k candidates. It writes to the same places as sortition_items_draw. If k is more than
 * what is left, it returns SORTITION_ERR_INVALID and the state does not change. */
int32_t sortition_item_session_draw(sortition_item_session *s, uint32_t k,
                                    uint32_t *out_index, uint8_t *out_ids);
/* Draws the next k candidates with an idempotency key. The key works as in
 * sortition_session_draw_keyed. */
int32_t sortition_item_session_draw_keyed(sortition_item_session *s, uint32_t k, const char *key,
                                          uint32_t *out_index, uint8_t *out_ids, int32_t *replayed);
uint64_t sortition_item_session_revision(const sortition_item_session *s);
int32_t sortition_item_session_id(const sortition_item_session *s, uint8_t *out); /* 33 bytes */
/* Returns a sealed token of the state in *out (free it with sortition_free). The arguments and the
 * warning are the same as for sortition_session_seal. To open it, pass the same set of candidates again
 * (the order may differ); another set gives SORTITION_ERR_STATE. */
int32_t sortition_item_session_seal(const sortition_item_session *s, const uint8_t *key, uint8_t key_id,
                                    char **out);
int32_t sortition_item_session_unseal(const uint8_t *token, size_t len, const uint8_t *key,
                                      const uint8_t *data, const size_t *lens, size_t count,
                                      sortition_item_session **out);
uint32_t sortition_item_session_remaining(const sortition_item_session *s);
uint32_t sortition_item_session_drawn_count(const sortition_item_session *s);
/* Returns the state as a JSON string in *out (free it with sortition_free). */
int32_t sortition_item_session_save(const sortition_item_session *s, char **out);
int32_t sortition_item_session_load(const uint8_t *state, size_t state_len,
                                    const uint8_t *data, const size_t *lens, size_t count,
                                    sortition_item_session **out);
void sortition_item_session_free(sortition_item_session *s); /* NULL is ignored */

/* ---- Seed strings ----
 * The SHA-256 of the seed string (its bytes) is the "commitment", and the seed is the first 8 bytes of
 * that digest read as a little-endian u64. Publish only the commitment before the draw and reveal the
 * string right before it: that shows the seed was not chosen again afterwards. */

/* Makes the seed from text (len bytes; it need not be UTF-8) and writes it to *out_seed.
 * If out_digest is not NULL, the 32 bytes of the SHA-256 (the commitment) are written too. */
int32_t sortition_seed_from_text(const uint8_t *text, size_t len, uint64_t *out_seed, uint8_t *out_digest);

/* The host allocates a buffer for calls (for environments such as WebAssembly, where the host writes
 * straight into linear memory). Pass the same len that was used to allocate to sortition_dealloc. */
uint8_t *sortition_alloc(size_t len);
void sortition_dealloc(uint8_t *ptr, size_t len);

/* The version string (static storage; do not free it). */
const char *sortition_version(void);

#ifdef __cplusplus
}
#endif
#endif
