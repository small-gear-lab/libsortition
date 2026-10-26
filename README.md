# libsortition

[![CI](https://github.com/small-gear-lab/libsortition/actions/workflows/ci.yml/badge.svg)](https://github.com/small-gear-lab/libsortition/actions/workflows/ci.yml)

[日本語](README.ja.md)

A small library for fair random lotteries: it picks `k` of `n` candidates without replacement, in a way that anyone can repeat and check afterwards. One Rust core with a C ABI, bindings for Node.js, PHP, Python, Ruby and WebAssembly, and the `sortition` command.

> **Status: experimental (0.1).** The interface may change before 1.0.

**Try it:** the [demo](https://small-gear-lab.github.io/libsortition/next/demo/) draws prizes from a list you paste in. All builds are listed at <https://small-gear-lab.github.io/libsortition/>.

## Quick start

The programs below print the same winners (`dave`, then `bob`) for the same candidates and seed, and so do the C and C++ programs in [examples/c](examples/c/demo.c).

**Command line** ([more](#command-line)):

```sh
cargo install --path cli                 # from a checkout
printf 'alice\nbob\ncarol\ndave\n' > names.txt
sortition draw names.txt --seed "my seed" -k 2
```

**Python** (from a checkout):

```python
import sys; sys.path.insert(0, "bindings/python")   # after `cargo build --release`
import sortition

people = ["alice", "bob", "carol", "dave"]
seed = sortition.seed_from_text("my seed")
for pick in sortition.draw_items(people, 2, seed=seed):
    print(people[pick.index])
```

**JavaScript** (WebAssembly):

```js
import { load, seedFromText } from "./sortition.js";   // bindings/wasm, after bindings/wasm/build.sh
const s = await load();

const people = ["alice", "bob", "carol", "dave"];
const seed = await seedFromText("my seed");
for (const pick of s.drawItems(people, 2, { seed })) console.log(people[pick.index]);
```

**C and C++** ([examples/c/demo.c](examples/c/demo.c) builds as both): after `cargo build --release`, run `make -C examples/c run`. Include [core/include/sortition.h](core/include/sortition.h) and link `libsortition`.

## Which function to use

Use the candidate functions (`draw_items` / `drawItems`, `draw_stages` / `drawStages`, the item sessions, and the CLI) when the winners must not depend on the order of your list: each candidate gets an ID from its bytes, and the draw works on the IDs in sorted order. The other entry points (`draw(n, k)`, and drawing from text, JSON, YAML or TOML) return positions in the list as you gave it, so a reordered list gives different winners.

## What you get

- **Fair draws.** Sampling without replacement with no modulo bias (rejection sampling). `draw(n, k)` picks `k` distinct numbers from `0..n`.
- **Repeatable and checkable.** A seed string whose SHA-256 you can publish before the draw (the commitment), and a fingerprint of the candidate set that does not depend on its order. The same candidates and seed give the same winners in every language and in the command. The rules are written down in [docs/SPEC.md](docs/SPEC.md), with [test vectors](test-vectors/vectors.json) and an independent [reference implementation](test-vectors/reference.py).
- **Any bytes as candidates.** Each candidate gets an ID that does not depend on its position, so the winners do not depend on the order of your list. Identical entries stay separate candidates. Lines of text and arrays in JSON, YAML or TOML work too.
- **Prize stages.** Draw the first prize, then the next from the rest; nobody wins twice.
- **Sessions you can pause.** Save the state as JSON and keep drawing later where you left off. A retried request carries an idempotency key and never draws twice. A sealed token (XChaCha20-Poly1305) lets a browser hold a state it cannot read or change, and `persist.js` keeps sessions in IndexedDB so that a reload loses nothing.
- **A C ABI** ([core/include/sortition.h](core/include/sortition.h)) under all of it.

## Install and build

Nothing is published yet (0.1). The first release is planned for crates.io (`sortition` and `sortition-cli`), GitHub Releases (the command and the shared library), npm (`libsortition-wasm`) and the GitHub Pages builds. Until then, build from a checkout:

```sh
cargo build --release                         # target/release/libsortition.so (.dylib, .dll)
cargo build --release --no-default-features   # without the YAML and TOML parsers
cargo install --path cli                      # the `sortition` command
bindings/wasm/build.sh                        # the WebAssembly build; needs: rustup target add wasm32-unknown-unknown
```

The `yaml` and `toml` features are on by default. The Python and Ruby bindings load the shared library built above (through `LIBSORTITION_PATH`, next to the module, and in `target/release`); the PHP extension and the Node.js addon link against it.

**Node.js:** use the WebAssembly build (`libsortition-wasm`); it needs no compiler and runs in Node.js. The native addon in `bindings/node` (`cd bindings/node && npm install && npm run build`) is for experiments and only builds on Linux and macOS.

## Layout

| path | what |
|---|---|
| `core/` | the Rust crate (`cdylib` and `rlib`) and the C header |
| `cli/` | the `sortition` command: draw from a list and verify a recorded draw |
| `bindings/dotnet/` | .NET: C#, F#, VB.NET (P/Invoke, .NET 10) |
| `bindings/dart/` | Dart (`dart:ffi`; the Dart VM and Flutter on desktop) |
| `bindings/go/` | Go (the WebAssembly build run with wazero, no cgo) |
| `bindings/java/` | Java 25 (Foreign Function & Memory API; works from Kotlin too) |
| `bindings/node/` | Node.js (N-API native addon) |
| `bindings/php/` | PHP extension (PHP 8.2 or later) |
| `bindings/python/` | Python (ctypes, no dependencies) |
| `bindings/ruby/` | Ruby (Fiddle, standard library only) |
| `bindings/swift/` | Swift (SwiftPM; `Package.swift` is at the root of the repository) |
| `bindings/wasm/` | WebAssembly for browsers and Node (no dependencies), with a demo |

## Using the libraries

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

## Command line

`sortition` draws from a list and checks a recorded draw. It runs the same code as the libraries, so it gives the same winners as every binding for the same candidates and seed.

```sh
cargo install --path cli                 # from a checkout; installs the `sortition` command

sortition commit "my seed"               # the commitment to publish before the draw, and the seed number
sortition draw names.txt --seed "my seed" --stage "Runner-up:3" --stage "Grand prize:1" --record record.json
sortition verify record.json names.txt --commit <the commitment you published>
```

`names.txt` has one candidate per line. The record holds the seed string, the fingerprint of the candidate set and the winners. Anyone who has the candidate list can run `sortition verify`; the order of the list does not matter. `sortition fingerprint names.txt` prints the fingerprint to publish before the draw, and `--random-seed` makes a seed string for you. Exit status is 0 on success, 1 when a verification fails and 2 on a usage or input error.

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

How a draw is made is written down exactly in [docs/SPEC.md](docs/SPEC.md), with [test vectors](test-vectors/vectors.json) and a [reference implementation](test-vectors/reference.py) written from that text alone, so that other programs can repeat a draw.

Whatever `save()` returns contains the random generator's state. Give untrusted parties a sealed token instead.

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option. The [NOTICE](NOTICE) file names the copyright holders. Unless you state otherwise, any contribution you intentionally submit for inclusion in this project is licensed as above, without additional terms or conditions.
