# AGENTS.md

Guidance for AI coding agents (and people) working in this repository.

libsortition is a small library for unbiased random lotteries: draw `k` of `n` candidates without replacement. One Rust core exposes a C ABI, and thin bindings wrap it for Node.js, PHP, Python, Ruby and WebAssembly. It is experimental (0.1): interfaces and saved formats may change before 1.0, and there is no migration path for old saved states.

## Layout

| Path | What it is |
|---|---|
| `core/src/lib.rs` | Error codes, the random sources, `draw`, `draw_lines`, and the C ABI for them |
| `core/src/data.rs` | Drawing from JSON / YAML / TOML arrays |
| `core/src/session.rs` | `Session`: continue a draw, save/restore as JSON, id, revision, idempotency keys |
| `core/src/items.rs` | Byte-string candidates: position-independent IDs, set fingerprint, `ItemSession` |
| `core/src/seal.rs` | Sealing a state into a token (XChaCha20-Poly1305) |
| `core/src/host.rs`, `hex.rs` | WASM host helpers (alloc, random import); hex helpers |
| `core/include/sortition.h` | The C header. It documents the contract of every exported function |
| `bindings/{node,php,python,ruby,wasm}` | One binding each. `wasm/persist.js` is the browser persistence helper, `wasm/demo/` the demo |

## Build and test

Every binding loads the shared library, so build it first and **rebuild after any change in `core/`**:

```sh
cargo build --release                    # target/release/libsortition.so
cargo test -p sortition                  # core tests
```

| Binding | Command (from the repository root unless noted) |
|---|---|
| Python | `python3 -m unittest discover -s bindings/python/tests` |
| Ruby | `ruby -Ibindings/ruby/lib bindings/ruby/test/sortition_test.rb` |
| Node.js | `cd bindings/node && npm install && npm run build && npm test` |
| WebAssembly | `rustup target add wasm32-unknown-unknown`, then `bindings/wasm/build.sh`, then `cd bindings/wasm && npm test` |
| PHP | `cd bindings/php && phpize && ./configure --with-sortition && make && NO_INTERACTION=1 make test TESTS=tests`, then `phpize --clean` |

- CI (`.github/workflows/ci.yml`) runs on Linux: the core tests (with and without the default features), `clippy` with `-D warnings` (default features, no default features, and wasm32), and every binding. PHP 8.4 must pass; the other PHP versions only report. Before pushing, run `cargo clippy --all-targets --locked -p sortition -- -D warnings`. `rustfmt` is not enforced yet (the code is not formatted with it). Dependabot (`.github/dependabot.yml`) opens weekly pull requests for the GitHub Actions versions, the Rust crates and the Node.js binding's npm dependencies; review a crate update with the sealing and randomness code in mind.
- Python and Ruby find the library through `LIBSORTITION_PATH`, next to the module, or in `target/release`.
- `bindings/wasm/sortition.wasm` is a build output and is ignored. `.cargo/config.toml` selects the `getrandom` custom backend for the wasm target.
- PHP: `phpize` may belong to a different PHP than `php` on `PATH`. Run the extension with `$(php-config --php-binary)`. The shims aim at PHP 7.4+, but only 8.4 has been run. `phpize` leaves many files behind (most are ignored); run `phpize --clean` and check `git status` before committing.
- The demo needs a static file server (it loads a module and the `.wasm`), for example `python3 -m http.server` in `bindings/wasm`.

## Publishing the WebAssembly build

`.github/workflows/pages.yml` publishes `bindings/wasm` and the demo to GitHub Pages (`https://small-gear-lab.github.io/libsortition/`) by pushing to the `gh-pages` branch:

- a push to `main` that touches the core or the WASM binding replaces `/next/`;
- publishing a GitHub release creates `/vX.Y.Z/` (never changed afterwards) and, for the newest stable version only, replaces `/latest/`. The tag must be `vX.Y.Z` (or `vX.Y.Z-rc.1`) and must equal the version in `core/Cargo.toml`.

Every folder has a `manifest.json` with the SHA-384 of its files. `gh-pages` is an orphan branch that holds only build output. Each run rebuilds it as a single commit and force-pushes it (with `--force-with-lease`), so do not commit to it by hand and do not protect it against force pushes. The logic lives in `.github/scripts/` (`pages.py`, `publish-pages.sh`); you can run `publish-pages.sh` against a local bare repository that has a `gh-pages` branch to try a change (see the comment at the top of the script).

## Rules that are easy to break

**Keep the five bindings in step.** A feature is not done until it exists in every binding with the same behavior (names follow each language's style), has tests, and appears in both READMEs. IDs, fingerprints and the session id are lowercase hex strings. A pick is `index` (position in the list you passed) plus `id`.

**Adding or changing a C function** means updating, together: the Rust function and its `# Safety` doc, `core/include/sortition.h` (with its comment), every binding, the tests, and the README.

**Adding an error code** means updating the constant in `lib.rs`, the header, and the message table in every binding (`_ERRORS` in Python, `ERRORS` in Ruby, `MESSAGES` in `wasm/sortition.js`, `default_message` in the Node C file, `sortition_default_message` in the PHP C file). Current codes: -1 invalid argument, -2 OS randomness failed, -3 not UTF-8, -4 parse error, -5 path not found, -6 unsupported format, -7 internal, -8 invalid saved state / sealed token, -9 idempotency key reused with a different request.

**Saved formats are versioned.** `Session` state is version 3 (`session.rs`), `ItemSession` state wraps it as version 1 (`items.rs`), the browser record is version 1 (`persist.js`), and sealed tokens start with `sortition1.`. Change the version whenever the format changes, and update the tests that reject other versions. The sealed token format is pinned by a golden test in `seal.rs`; do not change it casually.

**Behavior that other code relies on:**
- With a seed, a session uses xoshiro256** and is reproducible; it is not cryptographic. Without a seed, a session uses ChaCha20 with a key from the OS, and the key and position are part of the saved state. Drawing from the same state therefore always gives the same result.
- Drawing in several steps, with a save and restore in between, must give the same order as one draw (`draw(n, k1 + k2, seed)`).
- A draw that fails must leave no trace (state, revision, idempotency record, random position).
- An idempotency key with the same `k` replays the previous result without changing the state, even after everything has been drawn. The same key with another `k` is `-9`. `k == 0` does nothing and records nothing.
- Byte-string candidates are sorted by ID before drawing, so the result does not depend on input order. Identical byte strings get different IDs through the occurrence number.
- **`save()` output contains the random generator's state.** Anyone who holds it can predict later draws and forge a state. Anything handed to an untrusted party must be sealed (`seal`), never saved plain.

**Don't leak resources.** Sessions are freed by `close()` and, as a fallback, by finalizers (Ruby, Node.js, WASM) or object destructors (PHP, Python `__del__`). The WASM and PHP tests include memory-growth checks; keep them passing.

## Commits

- Write commit messages in English: an imperative subject of at most 72 characters, then a blank line and a wrapped body when it helps. Do not add `Co-Authored-By` or `Claude-Session` lines (a local hook rejects them).
- Describe the change itself, not the branch or merge route.
- Keep logical units: a core change first, then one commit per binding, then docs. Each commit should leave the tree buildable.
- The root commit is intentionally empty (`The first commit.`) and the license files were added right after the Cargo skeleton. History was rewritten once before 1.0; do not rewrite or force-push `main` again without the maintainer's explicit approval.
- Do not commit build output: `target/`, `node_modules/`, `*.so`, `bindings/wasm/sortition.wasm`, `phpize` files.

## Style

- Comments in `core/` and the bindings are written in Japanese; follow the language of the surrounding code. Public documentation (READMEs) exists in English and Japanese and both must be updated together.
- Core code avoids `unsafe` except at the FFI boundary; each exported `unsafe extern "C"` function states its safety conditions. Panics must not cross the FFI boundary where parsing or untrusted input is involved (`catch_unwind`).
- Keep dependencies few. Current ones: `getrandom`, `serde_json`, `saphyr` and `toml` (optional), `sha2`, `chacha20`, `chacha20poly1305`.
