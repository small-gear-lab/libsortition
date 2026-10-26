# libsortition

[日本語](README.ja.md)

A small library for unbiased random lotteries: it picks `k` of `n` candidates without replacement. One Rust core with a C ABI, and bindings for Node.js, PHP, Python, Ruby and WebAssembly.

> **Status: experimental (0.1).** The interface may change before 1.0.

## What you get

- **Sampling without replacement.** `draw(n, k)` picks `k` distinct numbers from `0..n`, in the order they were drawn.
- **Reproducible when you want it.** With no seed the OS random source is used; with a seed (a `u64`) the same seed gives the same result.
- **Candidates from text or data.** One candidate per line of text (surrounding spaces are trimmed, empty lines are dropped), or an array inside JSON, YAML or TOML (`path` such as `a.b.0` says where the array is). YAML aliases, custom tags and multiple documents are rejected.
- **A C ABI.** The header is [core/include/sortition.h](core/include/sortition.h): `sortition_draw`, `sortition_draw_text`, `sortition_draw_data`, `sortition_free`, `sortition_last_error`, `sortition_version`.

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
```

```sh
cd bindings/node && npm install && npm run build && npm test   # Node.js
bindings/wasm/build.sh                                          # needs: rustup target add wasm32-unknown-unknown
```

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option. Unless you state otherwise, any contribution you intentionally submit for inclusion in this project is licensed as above, without additional terms or conditions.
