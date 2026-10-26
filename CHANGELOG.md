# Changelog

All notable changes are listed here, newest first. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the version numbers follow [Semantic Versioning](https://semver.org/) (before 1.0, a minor version may change the API).

A change to the draw algorithm (`libsortition-draw/1`), the seed rule, the candidate IDs or the saved-state formats changes what a given seed produces. Such changes are always listed here, under their own heading.

## [Unreleased]

## [0.1.0] - 2026-10-26

The first release.

### Added

- **The core** (Rust, with a C ABI and header): drawing k of n without replacement, repeatable with a seed (xoshiro256\*\*) or from the operating system's randomness (ChaCha20); seeds from strings with a SHA-256 commitment; candidates as arbitrary bytes with order-independent IDs and a set fingerprint; prize stages; sessions that continue where they stopped, with idempotency keys and a revision; saved states (JSON) and sealed tokens (XChaCha20-Poly1305); drawing from lines of text and from JSON, YAML or TOML.
- **The `sortition` command**: `draw` (with a record of the draw), `verify`, `fingerprint` and `commit`.
- **Bindings**: Node.js (native addon and WebAssembly, with a browser persistence helper and a demo), PHP, Python, Ruby, .NET, Go, Java (and Kotlin), Swift and Dart, and a C and C++ example.
- **A specification** ([docs/SPEC.md](docs/SPEC.md)), shared test vectors that every binding's tests read, and an independent reference implementation in Python.
