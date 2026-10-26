# Security policy

## Reporting a vulnerability

Please report it privately, not in a public issue or pull request:

<https://github.com/small-gear-lab/libsortition/security/advisories/new>

It helps to include the version or commit, the binding and platform, and a small program or input that shows the problem. If you can, say what you think the impact is.

This is a small project maintained in spare time, so replies are best effort and there is no fixed response time. You will be told when the report has been read, and credited in the advisory if you wish.

## Supported versions

The library is experimental (0.1). Only the latest commit on `main` is supported; fixes are not backported.

## What counts

Reports about any of these are welcome:

- **Randomness and fairness**: a bias in the sampling, a predictable or reused random stream, a way to make a session draw something other than what its saved state determines.
- **Saved states and sealed tokens**: forging, altering or reading a sealed token without the key; a saved state that loads but breaks the no-repeat guarantee; an idempotency key that lets a request draw twice or return another request's result.
- **Memory safety**: a crash, out-of-bounds access or leak reachable from the C ABI or from a binding (including through malformed JSON / YAML / TOML, saved states or tokens).
- **Dependencies**: a known vulnerability in one of the crates the core uses that affects this library.

## What does not

These are described in the README under "Fairness and security" and are not vulnerabilities in the library:

- An organizer choosing a favorable seed, or discarding a result and drawing again.
- Anything that needs the holder of a plain `save()` output to be untrusted: it contains the random generator's state by design. Use a sealed token for that.
- The seeded generator (xoshiro256\*\*) being non-cryptographic.
