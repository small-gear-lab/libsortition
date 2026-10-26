# libsortition

[日本語](README.ja.md)

A small library for unbiased random lotteries: it picks `k` of `n` candidates without replacement. One Rust core with a C ABI, and bindings for Node.js, PHP, Python, Ruby and WebAssembly.

> **Status: experimental (0.1).** The interface may change before 1.0.

## What you get

- **Sampling without replacement.** `draw(n, k)` picks `k` distinct numbers from `0..n`, in the order they were drawn.
- **Reproducible when you want it.** With no seed the OS random source is used; with a seed (a `u64`) the same seed gives the same result.
- **Candidates from text or data.** One candidate per line of text (surrounding spaces are trimmed, empty lines are dropped), or an array inside JSON, YAML or TOML (`path` such as `a.b.0` says where the array is). YAML aliases, custom tags and multiple documents are rejected.
- **Sessions: keep drawing where you left off.** A session holds `0..n` and draws without replacement each time you call it. Its state can be saved as JSON and restored later, so the draw can continue from a database or a file. With a seed, drawing in several steps gives the same order as one big `draw(n, k1 + k2 + ..., seed)`. A saved state that is malformed or inconsistent is rejected.
- **Any bytes as candidates, identified by stable IDs.** Pass arbitrary byte strings and each one gets a unique ID that does not depend on its position: `SHA-256(prefix ‖ bytes ‖ occurrence)`, where the occurrence number tells identical byte strings apart. Candidates are sorted by ID before drawing, so the same candidate set and seed give the same winners whatever the input order. A set fingerprint (a hash of the sorted IDs) can be published before the draw and checked afterwards. Item sessions save the drawn IDs and the fingerprint, and can only be resumed with the same candidate set (in any order).
- **A C ABI.** The header is [core/include/sortition.h](core/include/sortition.h): `sortition_draw`, `sortition_draw_text`, `sortition_draw_data`, `sortition_session_*`, `sortition_item_*`, `sortition_items_draw`, `sortition_free`, `sortition_last_error`, `sortition_version`.

## Layout

| path | what |
|---|---|
| `core/` | the Rust crate (`cdylib` and `rlib`) and the C header |
| `bindings/node/` | Node.js (N-API native addon) |
| `bindings/php/` | PHP extension |
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

```sh
cd bindings/node && npm install && npm run build && npm test   # Node.js
bindings/wasm/build.sh                                          # needs: rustup target add wasm32-unknown-unknown
```

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option. Unless you state otherwise, any contribution you intentionally submit for inclusion in this project is licensed as above, without additional terms or conditions.
