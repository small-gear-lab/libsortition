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
| `docs/SPEC.md`, `docs/SPEC.ja.md` | The draw algorithm written out exactly (the Japanese file is a summary) |
| `test-vectors/` | `vectors.json` (made by the Rust tests, read by every binding's tests) and `reference.py` (a second implementation written from SPEC.md alone) |
| `cli/` | The `sortition` command (crate `sortition-cli`): `draw`, `verify`, `fingerprint`, `commit`. It uses the core's `draw_items` directly |
| `examples/c/` | A C example (it also builds as C++) that uses the C ABI; CI builds and runs it |
| `bindings/{dotnet,go,node,php,python,ruby,wasm}` | One binding each. `wasm/persist.js` is the browser persistence helper, `wasm/demo/` the demo |

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
| Go | `cd bindings/go && go test ./...` (Go 1.26+; it embeds `sortition.wasm`, so it needs no native library. After any change to the core run `go generate` there and commit the new `sortition.wasm`, or `go get` users get the old core) |
| .NET | `dotnet test bindings/dotnet` (needs the .NET 10 SDK; finds the library in `target/release`, or through `LIBSORTITION_PATH`) |
| Node.js | `cd bindings/node && npm install && npm run build && npm test` |
| Reference | `python3 test-vectors/reference.py` (reproduces `test-vectors/vectors.json` from SPEC.md's rules) |
| C / C++ | `cargo build --release && make -C examples/c check` |
| CLI | `cargo test -p sortition-cli` (runs the built binary) |
| WebAssembly | `rustup target add wasm32-unknown-unknown`, then `bindings/wasm/build.sh`, then `cd bindings/wasm && npm test` |
| PHP | `cd bindings/php && phpize && ./configure --with-sortition && make && NO_INTERACTION=1 make test TESTS=tests`, then `phpize --clean` |

- CI (`.github/workflows/ci.yml`) runs on Linux (the core and the CLI also run on macOS and Windows, informationally until they have passed a few times): the core tests (with and without the default features), `clippy` with `-D warnings` (default features, no default features, and wasm32), and every binding. PHP 8.4 must pass; the other PHP versions only report. Before pushing, run `cargo clippy --all-targets --locked -p sortition -- -D warnings`. The minimum Rust version is the `rust-version` in `core/Cargo.toml` (1.85); CI builds and tests with it, using a lockfile resolved for that version. Raising it is a breaking change for users, so say so in the commit message. `rustfmt` is not enforced yet (the code is not formatted with it). Dependabot (`.github/dependabot.yml`) opens weekly pull requests for the GitHub Actions versions, the Rust crates and the Node.js binding's npm dependencies; review a crate update with the sealing and randomness code in mind.
- Python and Ruby find the library through `LIBSORTITION_PATH`, next to the module, or in `target/release`.
- `bindings/wasm/sortition.wasm` is a build output and is ignored. `.cargo/config.toml` selects the `getrandom` custom backend for the wasm target.
- PHP: `phpize` may belong to a different PHP than `php` on `PATH`. Run the extension with `$(php-config --php-binary)`. The extension needs PHP 8.2 or later (the versions PHP itself supports; the build stops with an error below that). CI runs 8.2 to 8.4 as required and 8.5 as informational until it has passed once. `phpize` leaves many files behind (most are ignored); run `phpize --clean` and check `git status` before committing.
- The demo needs a static file server (it loads a module and the `.wasm`), for example `python3 -m http.server` in `bindings/wasm`.

## Publishing the WebAssembly build

`.github/workflows/pages.yml` publishes `bindings/wasm` and the demo to GitHub Pages (`https://small-gear-lab.github.io/libsortition/`) by pushing to the `gh-pages` branch:

- a push to `main` that touches the core or the WASM binding replaces `/next/`;
- publishing a GitHub release creates `/vX.Y.Z/` (never changed afterwards) and, for the newest stable version only, replaces `/latest/`. The tag must be `vX.Y.Z` (or `vX.Y.Z-rc.1`) and must equal the version in `core/Cargo.toml`.

Every folder has a `manifest.json` with the SHA-384 of its files. `gh-pages` is an orphan branch that holds only build output. Each run rebuilds it as a single commit and force-pushes it (with `--force-with-lease`), so do not commit to it by hand and do not protect it against force pushes. The logic lives in `.github/scripts/` (`pages.py`, `publish-pages.sh`); you can run `publish-pages.sh` against a local bare repository that has a `gh-pages` branch to try a change (see the comment at the top of the script).

## Publishing the npm package

`libsortition-wasm` is published from `bindings/wasm` (`npm publish` after `bindings/wasm/build.sh`). npm leaves symbolic links out of a package, so `scripts/pack.mjs` (the `prepack` and `postpack` scripts) copies the license files and `NOTICE` from the root into the package and removes them afterwards, and stops the publish when `sortition.wasm` has not been built. Check what goes in with `npm pack --dry-run`. The Node.js native addon in `bindings/node` is `private`: it cannot be installed from npm, because it links against a library built from the repository.

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

**License files have one source.** `LICENSE-MIT`, `LICENSE-APACHE` and `NOTICE` live at the root. `core/` holds symbolic links to them so that the crate package contains the files (CI checks that the packaged files are identical to the root ones); do not replace the links with copies. `pages.py` copies them from the root into every published folder. Any new package (npm, wheel, gem) has to ship them too.

**A change that can alter a draw result is a change of the algorithm.** The rules are in `docs/SPEC.md` and named by `ALGORITHM` in `core/src/lib.rs` (`libsortition-draw/1`). If you change anything that can give a different result for the same candidates, seed or stages (the seed rule, the random streams, `below`, the draw, candidate IDs or their order, the fingerprint, how stages are cut), then: update SPEC.md, give the algorithm a new name, regenerate the vectors with `SORTITION_UPDATE_VECTORS=1 cargo test -p sortition vectors`, and make `test-vectors/reference.py` pass again. The vectors test fails on any unexplained difference, so do not edit `vectors.json` by hand. Records written by the CLI carry the algorithm name.

**The seed rule and the draw order are shared by everything.** The seed of a string is the first 8 bytes of its SHA-256 as a little-endian u64 (`core/src/seed.rs`); staged draws take the total with `draw_items` and cut it up in order. The CLI test `draw_matches_the_library_and_other_bindings` pins the winners for one candidate list and seed, and Python, Ruby, Node.js, WebAssembly and PHP must give the same ones. The record written by `sortition draw` has a format name (`libsortition-draw-record/1`); change the number whenever its format changes. Publish `sortition` to crates.io before `sortition-cli` (the CLI's package cannot be built until the crate it depends on exists there).

**Support the versions upstream supports, and no older ones.** The bindings target PHP 8.2+, Node.js 22+, Python 3.11+ and Ruby 3.3+ (CI runs the oldest and the newest of each; a Current Node.js release such as 26 only reports until it becomes an LTS), and Rust 1.85+ for the crates. When a version reaches its end of life (check <https://endoflife.date>), drop it from the CI matrix, the documented minimum and, for Node.js, `engines` in `bindings/node/package.json`. An update from Dependabot can force this: `node-gyp` 13 needs Node.js 22.22 or later.

**Don't leak resources.** Sessions are freed by `close()` and, as a fallback, by finalizers (Ruby, Node.js, WASM) or object destructors (PHP, Python `__del__`). The WASM and PHP tests include memory-growth checks; keep them passing.

## Commits

- Write commit messages in English: an imperative subject of at most 72 characters, then a blank line and a wrapped body when it helps. Do not add `Co-Authored-By` or `Claude-Session` lines (a local hook rejects them).
- Describe the change itself, not the branch or merge route.
- Keep logical units: a core change first, then one commit per binding, then docs. Each commit should leave the tree buildable.
- The root commit is intentionally empty (`The first commit.`) and the license files were added right after the Cargo skeleton. History was rewritten once before 1.0; do not rewrite or force-push `main` again without the maintainer's explicit approval.
- Do not commit build output: `target/`, `node_modules/`, `*.so`, `bindings/wasm/sortition.wasm`, `phpize` files.

## Style

- Comments inside the code of `core/` and the bindings are written in Japanese; follow the language of the surrounding code. What users read is in English: the documentation of the public Rust API (the examples in `core/src/lib.rs` run as doc tests, and CI builds the docs with warnings denied), the C header, `examples/`, the package READMEs and `docs/SPEC.md`. The `extern "C"` functions are `#[doc(hidden)]` in the Rust docs: their documentation is the header. Public documentation (READMEs) exists in English and Japanese and both must be updated together.
- Core code avoids `unsafe` except at the FFI boundary; each exported `unsafe extern "C"` function states its safety conditions. Panics must not cross the FFI boundary where parsing or untrusted input is involved (`catch_unwind`).
- Keep dependencies few. Current ones: `getrandom`, `serde_json`, `saphyr` and `toml` (optional), `sha2`, `chacha20`, `chacha20poly1305`.
