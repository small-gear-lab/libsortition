# libsortition

[![CI](https://github.com/small-gear-lab/libsortition/actions/workflows/ci.yml/badge.svg)](https://github.com/small-gear-lab/libsortition/actions/workflows/ci.yml)

[日本語](README.ja.md)

A small library for unbiased random lotteries: it picks `k` of `n` candidates without replacement. One Rust core with a C ABI, and bindings for Node.js, PHP, Python, Ruby and WebAssembly.

> **Status: experimental (0.1).** The interface may change before 1.0.

## What you get

- **Sampling without replacement.** `draw(n, k)` picks `k` distinct numbers from `0..n`, in the order they were drawn.
- **Reproducible when you want it.** With no seed the OS random source is used; with a seed (a `u64`) the same seed gives the same result.
- **Candidates from text or data.** One candidate per line of text (surrounding spaces are trimmed, empty lines are dropped), or an array inside JSON, YAML or TOML (`path` such as `a.b.0` says where the array is). YAML aliases, custom tags and multiple documents are rejected.
- **Sessions: keep drawing where you left off.** A session holds `0..n` and draws without replacement each time you call it. Its state can be saved as JSON and restored later, so the draw can continue from a database or a file. With a seed, drawing in several steps gives the same order as one big `draw(n, k1 + k2 + ..., seed)`. A saved state that is malformed or inconsistent is rejected.
- **Resume after a reload or an expired HTTP session, with no server.** Draws use a cryptographic random stream (ChaCha20) whose state is saved with the session, so the same saved state always gives the same next draw. Every draw can carry an idempotency key, so a retried request returns the previous result instead of drawing twice. In the browser, `persist.js` keeps sessions in IndexedDB; with a server, `seal()` turns a state into a tamper-proof token (XChaCha20-Poly1305) that is safe to hand to the browser.
- **Any bytes as candidates, identified by stable IDs.** Pass arbitrary byte strings and each one gets a unique ID that does not depend on its position: `SHA-256(prefix ‖ bytes ‖ occurrence)`, where the occurrence number tells identical byte strings apart. Candidates are sorted by ID before drawing, so the same candidate set and seed give the same winners whatever the input order. A set fingerprint (a hash of the sorted IDs) can be published before the draw and checked afterwards. Item sessions save the drawn IDs and the fingerprint, and can only be resumed with the same candidate set (in any order).
- **A C ABI.** The header is [core/include/sortition.h](core/include/sortition.h): `sortition_draw`, `sortition_draw_text`, `sortition_draw_data`, `sortition_session_*`, `sortition_item_*`, `sortition_items_draw`, `sortition_free`, `sortition_last_error`, `sortition_version`.

## Layout

| path | what |
|---|---|
| `core/` | the Rust crate (`cdylib` and `rlib`) and the C header |
| `bindings/node/` | Node.js (N-API native addon) |
| `bindings/php/` | PHP extension (PHP 8.2 or later) |
| `bindings/python/` | Python (ctypes, no dependencies) |
| `bindings/ruby/` | Ruby (Fiddle, standard library only) |
| `bindings/wasm/` | WebAssembly for browsers and Node (no dependencies), with a demo |

## Build

```sh
cargo build --release          # target/release/libsortition.so (.dylib, .dll)
cargo build --release --no-default-features   # without the YAML and TOML parsers
```

The `yaml` and `toml` features are on by default.

The bindings load the shared library built above. Python and Ruby look for it through `LIBSORTITION_PATH`, next to the module, and in `target/release`.

```python
import sys; sys.path.insert(0, "bindings/python")
import sortition

sortition.draw(10, 3)                       # e.g. [7, 2, 5]
sortition.draw(10, 3, seed=42)              # the same result every time
sortition.draw_text("alice\nbob\ncarol\n", 2)
sortition.draw_data('{"members": ["a", "b", "c"]}', 2, "json", path="members")

lot = sortition.Session(100, seed=1)        # a session: draw in steps
lot.draw(3)                                 # the first 3
state = lot.save()                          # JSON string; store it anywhere
sortition.Session.load(state).draw(2)       # the next 2, as if never interrupted
```

```python
people = [b"alice", b"bob", "carol"]        # bytes, or str (UTF-8)
sortition.item_fingerprint(people)          # same for any order; publish it with the seed
picks = sortition.draw_items(people, 2, seed=42)   # [Pick(index=1, id="53…"), …]
lot = sortition.ItemSession(people, seed=42)
lot.draw(1)
state = lot.save()                          # includes the drawn IDs and the fingerprint
sortition.ItemSession.load(state, reversed(people)).draw(1)   # same set, any order
```

Byte-string candidates are in every binding too: `draw_items` / `ItemSession` (Python), `Sortition.draw_items` / `Sortition::ItemSession` (Ruby), `drawItems` / `createItemSession` / `loadItemSession` (Node.js and WebAssembly), and `sortition_draw_items` / `SortitionItemSession` (PHP). IDs and fingerprints are lowercase hex strings, and a pick is `index` (the position in the list you passed) plus `id`.

Sessions are in every binding: `Session` (Python), `Sortition::Session` (Ruby), `createSession` / `loadSession` (Node.js and WebAssembly), and `SortitionSession` (PHP).

### Resuming after a reload or an expired HTTP session

**Without a server** (a static page with WebAssembly): `bindings/wasm/persist.js` saves a session to IndexedDB (or `localStorage`, or memory) every time you draw.

```js
import { load } from "./sortition.js";
import { createPersistence } from "./persist.js";

const lots = createPersistence(await load());
const lot = await lots.createItems(["alice", "bob", "carol"], { seed: 42n });
await lot.draw(1);

// ... the page is reloaded, or the tab is closed and opened again ...
const again = await lots.resume(lot.id);   // null if nothing is saved under that id
await again.draw(1);                       // continues where it stopped, never repeats a winner
```

- Each `draw` carries an idempotency key (made automatically unless you pass `{ key }`), so retrying after a failed save does not draw twice. The same key with a different `k` is an error (code `-9`).
- A save only succeeds if the stored `revision` is the one that was read, and Web Locks keeps tabs from drawing at the same time. A tab with an old copy re-reads the stored state and continues from there.
- `export()` / `import()` move a session to another device as text. It contains the random generator's state, so keep it to yourself.
- `demo/index.html` uses this: after the main draw it can draw substitutes one at a time (for people who decline), and a reload restores everything.

To draw prizes one stage at a time, give the plan when you create the session. Each stage is drawn when you ask for it, and a reload in between loses nothing:

```js
const lot = await lots.createStaged(["alice", "bob", "carol", "dave", "erin"], [
  { name: "Runner-up", count: 3 },
  { name: "Grand prize", count: 1 },
], { seed: 42n });

const first = await lot.drawStage();      // { stage: 0, name: "Runner-up", winners: [...] }
// ... reload ...
const again = await lots.resume(lot.id);
await again.drawStage(0);                 // an already-drawn stage returns the same winners
await again.drawStage();                  // the next stage, "Grand prize"
await again.draw(1);                      // after all stages: one substitute, from where it left off
```

The results are the same as drawing the total once and cutting it up in order. The plan cannot change after creation, a stage cannot be drawn before the ones in front of it, and `draw()` waits until every stage has been drawn.

**With a server** (for example a function on Workers or Lambda that keeps no database): a saved state includes the random generator's state, so whoever holds it can predict later winners and can forge a state. Keep the state out of the browser's hands by sealing it. The key stays on the server.

```python
key = bytes.fromhex(os.environ["SORTITION_KEY"])   # 32 random bytes, kept on the server
lot = sortition.Session.unseal(token, key)         # SortitionError (-8) if forged, altered or the wrong key
picks = lot.draw(3, key=request_id)                # a retried request returns the same picks
new_token = lot.seal(key)                          # send picks and new_token back to the browser
```

Because the next draw is a function of the saved state, sending an old token again gives the same result, and the idempotency key covers a token that has already advanced. A server that stores sessions can also compare `revision` before it overwrites a record. The idea is the same in every binding (`seal` / `unseal` / `sealKeyId`).

Nothing here stops an organizer from throwing the result away and starting a new session. Publish the session `id`, the set fingerprint and the seed commitment before the draw.

```sh
cd bindings/node && npm install && npm run build && npm test   # Node.js
bindings/wasm/build.sh                                          # needs: rustup target add wasm32-unknown-unknown
```

## Fairness and security

libsortition aims to be a fair lottery, not only a convenient one.

What it does for you:

- **Unbiased sampling.** Rejection sampling removes modulo bias, so every candidate has the same chance. The tests check the distribution.
- **A strong random source by default.** Without a seed, a session draws from a cryptographic stream (ChaCha20, keyed from the OS). With a seed it uses xoshiro256\*\*, which is reproducible but not cryptographic.
- **A draw can be checked afterwards.** The candidate set has a fingerprint that does not depend on order, and the seed can be committed to (for example as a SHA-256 hash) before the draw.
- **Safe to keep and to retry.** A saved state can be sealed, so a browser can hold it without reading or altering it. An idempotency key stops a retried request from drawing twice.

What it cannot do for you:

- **Stop the organizer from steering the result.** Choosing a favorable seed, or throwing a result away and drawing again, cannot be prevented by code. For a draw that others must trust, publish the candidate-set fingerprint and the seed commitment first, then mix in a value that nobody controls (for example a public randomness beacon) to form the seed.
- **Replace a review.** The library has not been audited, and 0.1 may still change. Rules about lotteries, prizes and record keeping are outside the library.

Whatever `save()` returns contains the random generator's state. Give untrusted parties a sealed token instead.

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option. The [NOTICE](NOTICE) file names the copyright holders. Unless you state otherwise, any contribution you intentionally submit for inclusion in this project is licensed as above, without additional terms or conditions.
