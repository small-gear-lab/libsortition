# libsortition: the draw algorithm

This document says exactly how libsortition turns a list of candidates and a seed into winners. It is written so that someone who has never seen the code can write another implementation and get the same winners. The draw record that `sortition draw --record` writes is only worth anything if the draw can be repeated later, so the rules here are a promise.

> Status: experimental (0.1). Nothing here changes silently: any change that could give a different result for the same input gets a new algorithm name (see [Versioning](#1-versioning)).

[日本語の概要](SPEC.ja.md)

## Contents

1. [Versioning](#1-versioning)
2. [Conventions](#2-conventions)
3. [Seed from a string](#3-seed-from-a-string)
4. [Random streams](#4-random-streams)
5. [A uniform number below a bound](#5-a-uniform-number-below-a-bound)
6. [Drawing k of n](#6-drawing-k-of-n)
7. [Candidates: IDs, order and fingerprint](#7-candidates-ids-order-and-fingerprint)
8. [Drawing candidates and prize stages](#8-drawing-candidates-and-prize-stages)
9. [Sessions](#9-sessions)
10. [Saved states](#10-saved-states)
11. [Sealed tokens](#11-sealed-tokens)
12. [Draw records](#12-draw-records)
13. [Entry points that draw by position](#13-entry-points-that-draw-by-position)
14. [Test vectors and the reference implementation](#14-test-vectors-and-the-reference-implementation)

## 1. Versioning

The algorithm in sections 3 to 8 is named **`libsortition-draw/1`** (`ALGORITHM` in the Rust API). It covers: how a seed is made from a string, the seeded random stream, the uniform number below a bound, drawing k of n, candidate IDs and their order, and cutting a draw into prize stages.

For the same candidates, seed and stages, every implementation of `libsortition-draw/1` gives the same winners. A change that could alter any such result must use a new name (`libsortition-draw/2`), and records carry the name so that a verifier never checks a record with the wrong algorithm.

The random stream of a session without a seed (ChaCha20 with a random key, section 4.2), saved states (section 10) and sealed tokens (section 11) have their own version numbers.

## 2. Conventions

- Integers are unsigned. `u64` arithmetic wraps modulo 2^64 unless it says otherwise. `rotl(x, k)` rotates a 64-bit value left by `k` bits.
- `LE64(x)` is the 8-byte little-endian form of `x`. `||` joins byte strings. `hex(b)` is lowercase hexadecimal.
- Text is UTF-8. Byte strings are compared byte by byte, from the first byte, as unsigned numbers.
- `SHA256` is SHA-256 (FIPS 180-4).
- The code below is Python-like pseudo-code. `//` is integer division and `%` is the remainder.

## 3. Seed from a string

```
digest     = SHA256(bytes of the seed string)
commitment = hex(digest)                       # publish this before the draw
seed       = integer of digest[0..8] read as little-endian    # a u64
```

The seed string is revealed at the draw; anyone can check that it hashes to the commitment that was published earlier. An empty string is a valid seed string.

Example: the seed string `sortition` has commitment `80ae496fd3b5d201bb96339356ff252e2b1cdc214ad567f6ebe8de4537e6b31c` and seed `131367258856992384`.

## 4. Random streams

A random stream gives successive `u64` values with `next()`.

### 4.1 Seeded stream: xoshiro256\*\*

The state is four `u64` values `s[0..3]`, made from the seed with SplitMix64:

```
z = seed
for i in 0..3:
    z = (z + 0x9E3779B97F4A7C15) mod 2^64
    x = z
    x = ((x ^ (x >> 30)) * 0xBF58476D1CE4E5B9) mod 2^64
    x = ((x ^ (x >> 27)) * 0x94D049BB133111EB) mod 2^64
    s[i] = x ^ (x >> 31)
```

`next()` is xoshiro256\*\* (Blackman and Vigna):

```
result = (rotl((s[1] * 5) mod 2^64, 7) * 9) mod 2^64
t      = (s[1] << 17) mod 2^64
s[2] ^= s[0]
s[3] ^= s[1]
s[1] ^= s[2]
s[0] ^= s[3]
s[2] ^= t
s[3]  = rotl(s[3], 45)
return result
```

A draw with a seed always uses this stream. It is reproducible and **not** cryptographic.

### 4.2 Stream of a session without a seed: ChaCha20

A session created without a seed takes a random 32-byte key from the operating system. The stream is the ChaCha20 key stream (RFC 8439) for that key, with an all-zero 96-bit nonce and the block counter starting at 0. The state is the key and `pos`, the number of key stream bytes already used (always a multiple of 8):

```
next():
    value = integer of keystream[pos .. pos + 8] read as little-endian
    pos  += 8
    return value
```

The key stream is limited to 2^38 bytes (the 32-bit block counter); a session that would go past it fails instead of repeating. The key and `pos` are part of the saved state, so drawing from the same saved state always gives the same result.

## 5. A uniform number below a bound

`below(bound)` returns a value in `0 .. bound - 1` (`bound` is at least 1) without modulo bias:

```
zone = (2^64 - 1) - ((((2^64 - 1) % bound) + 1) % bound)
loop:
    v = next()
    if v <= zone:
        return v % bound
```

A value above `zone` is thrown away and the next one is used. For a bound that does not divide 2^64, this makes every result equally likely.

## 6. Drawing k of n

`draw(n, k)` returns `k` distinct numbers from `0 .. n - 1` in the order they were drawn (a sparse Fisher–Yates shuffle; `k <= n` is required):

```
swapped = {}                          # a map from position to value; missing means "same as the position"
result  = []
for i in 0 .. k - 1:
    j  = i + below(n - i)
    vj = swapped.get(j, j)
    vi = swapped.get(i, i)
    swapped[j] = vi
    result.append(vj)
return result
```

Drawing `k1` and then `k2` more from the same stream, without starting over, gives the same list as drawing `k1 + k2` at once.

Example (seed 1): `draw(10, 3)` is `[7, 2, 6]`.

## 7. Candidates: IDs, order and fingerprint

A candidate is a byte string (it may be empty, and may be any bytes). Identical byte strings are separate candidates.

**ID.** For each candidate, in the order given, let `occurrence` be the number of earlier candidates with exactly the same bytes (0 for the first one):

```
id = SHA256("libsortition/candidate/v1" || 0x00 || bytes || LE64(occurrence))
```

The prefix is the 25 ASCII bytes of `libsortition/candidate/v1` followed by one zero byte. IDs are 32 bytes. The ID does not depend on where the candidate is in the list (only on the earlier identical ones), and all IDs in a list are different. (If two IDs were ever equal, the draw fails.)

**Order.** The candidates sorted by ID, ascending, are the *canonical order*. Drawing works on positions in the canonical order, so the input order does not matter.

**Fingerprint.** The fingerprint of a set of candidates is:

```
SHA256("libsortition/set/v1" || 0x00 || LE64(count) || id_0 || id_1 || ... )   # IDs in canonical order
```

It names the set, not its order. Publish it with the commitment before the draw.

## 8. Drawing candidates and prize stages

`draw_items(candidates, k, seed)`:

1. Compute the IDs and the canonical order (section 7).
2. `positions = draw(len(candidates), k)` with the seeded stream of `seed` (sections 4.1 and 6).
3. The winners are the candidates at those positions in the canonical order, in the order drawn. Report each winner's ID and its index in the input list.

**Prize stages.** Stages are given in drawing order, each with a count. Let `total` be the sum of the counts (`total <= len(candidates)` is required). Draw `total` winners with `draw_items`, then cut the list into consecutive pieces: the first `count_0` winners are stage 0, the next `count_1` are stage 1, and so on. This is the same as drawing the first stage, then drawing the next from the rest, and so on. No one wins twice. A stage with count 0 has no winners.

A seed string is turned into a seed with section 3 before drawing.

Example: with the candidates `alice, bob, carol, dave, erin, frank, grace, heidi`, the seed string `stages` and the stages `(副賞, 3)` and `(大賞, 1)`, the winners are `carol, heidi, bob` for the first stage and `erin` for the second.

## 9. Sessions

A session draws without replacement over `n` positions (for candidates, the canonical order of section 7) and can continue where it stopped. Its state is the stream, the winners so far (`drawn`, in order) and the `swapped` map of section 6.

`draw(k)` needs `k <= n - len(drawn)` and continues the process of section 6 from `i = len(drawn)`. A session with a seed uses the xoshiro stream of that seed (section 4.1); a session without one uses ChaCha20 (section 4.2). A draw that fails changes nothing (not the winners, the stream position, the revision or the idempotency records).

**Revision.** The `revision` counts the draws that changed the state: a call with `k > 0` that succeeded and was not a replay. A call with `k = 0` does nothing.

**Idempotency keys.** A call may carry a key (1 to 128 bytes of UTF-8). The session records, for each key used with `k > 0`, the range of `drawn` that call produced.

- The same key with the same `k` again is a replay: it changes nothing and returns the earlier winners, even if everything has been drawn since.
- The same key with another `k` fails with a conflict.
- A key that was never used draws normally and is recorded. A failed call records nothing.

A saved state therefore decides the next draw completely, and sending an old state again gives the same winners as before.

## 10. Saved states

A saved state is a JSON object. Numbers beyond 2^53 are written as decimal strings.

**Session** (`"version": 3`):

| Field | Meaning |
|---|---|
| `id` | 32 lowercase hex characters (16 random bytes taken when the session was created) |
| `revision` | integer, see section 9 |
| `calls` | `[[key, start, count], ...]`: the keyed calls, in order; `drawn[start .. start + count]` is what each produced |
| `n` | the number of positions |
| `rng` | `{"kind": "xoshiro256", "s": [four decimal strings]}` or `{"kind": "chacha20", "key": 64 hex characters, "pos": decimal string}` |
| `drawn` | the winners so far, in the order drawn (positions) |
| `swapped` | `[[position, value], ...]` sorted by position, for positions that are not drawn yet |

A reader checks the state before using it: the version, the field types, `drawn` having distinct values below `n`, `swapped` keys in `len(drawn) .. n - 1`, the touched positions `0 .. len(drawn) - 1` plus the `swapped` keys being exactly the set of values in `drawn` and `swapped`, `calls` being in order without overlap with unique keys, `revision <= len(drawn)` and not less than the number of calls (and 0 exactly when nothing is drawn), a non-zero xoshiro state, and for ChaCha20 a `pos` that is a multiple of 8 and at most 2^38. A state that fails is rejected.

**Candidate session** (`"version": 1`, `"kind": "items"`):

```
{"version": 1, "kind": "items", "fingerprint": hex, "drawn": [hex ID, ...], "session": <a session state over the canonical order>}
```

It is restored together with a list of candidates (in any order); the fingerprint of that list must equal the saved one, and `drawn` must be the IDs at the positions in `session.drawn`.

## 11. Sealed tokens

A sealed token lets a state be kept by someone who must not read or change it. The key is 32 bytes and stays with the party that seals and opens.

```
token      = "sortition1." || base64url(key_id || nonce || ciphertext_and_tag)    # no padding
key_id     = one byte, chosen by the caller (0..255)
nonce      = 24 random bytes
plaintext  = the saved state as JSON text (UTF-8)
aad        = "libsortition/seal/v1" || key_id     # 20 ASCII bytes and the key id byte
ciphertext_and_tag = XChaCha20-Poly1305(key, nonce, aad, plaintext)    # a 16-byte tag at the end
```

XChaCha20-Poly1305 is the construction of draft-irtf-cfrg-xchacha: `subkey = HChaCha20(key, nonce[0..16])`, then ChaCha20-Poly1305 of RFC 8439 with `subkey` and the 12-byte nonce `0x00000000 || nonce[16..24]`. A token that is malformed, was made with another key, or was changed in any way cannot be opened; these cases are not told apart.

## 12. Draw records

`sortition draw --record` writes this JSON; `sortition verify` reads it:

```
{
  "format": "libsortition-draw-record/1",
  "algorithm": "libsortition-draw/1",
  "libsortition": "<version of the tool that wrote it>",
  "seed": {"text": <seed string>, "commit": <hex digest>, "u64": <decimal string>},
  "candidates": {"count": <integer>, "fingerprint": <hex>},
  "stages": [{"name": ..., "count": ..., "winners": [{"id": <hex>, "item": <the candidate's text>}, ...]}, ...]
}
```

To verify, a program checks that the seed string hashes to `commit` (and to the commitment that was published beforehand, if there is one), that `u64` follows from it, that the given candidates have the recorded count and fingerprint, and that drawing the recorded stages from them gives the recorded winners. A record with an `algorithm` the program does not implement cannot be verified.

## 13. Entry points that draw by position

`draw(n, k)`, drawing from text (one candidate per line) and drawing from JSON, YAML or TOML data return **positions** in the list as given, drawn as in section 6 with the seeded stream. They do not use IDs, so their result depends on the order of the list. For text, a line is trimmed of surrounding white space (Unicode `White_Space`) and empty lines are skipped; identical lines are separate candidates. They are not part of `libsortition-draw/1`'s promise about input order; use the candidate functions (`draw_items` and friends) when the winners must not depend on the order of the list.

## 14. Test vectors and the reference implementation

[`test-vectors/vectors.json`](../test-vectors/vectors.json) holds expected values for every part above: seeds, the xoshiro and ChaCha20 streams, `below` (including a bound that rejects often), draws, candidate IDs and fingerprints, candidate draws, stages, a session restored from a ChaCha20 state, and a sealed token. The Rust code generates the file; the tests of every binding read it and must get the same values.

[`test-vectors/reference.py`](../test-vectors/reference.py) is a second implementation, written from this document with only the Python standard library. Running `python3 test-vectors/reference.py` checks every vector with it. If you write another implementation, it should reproduce the same file.
