# sortition for Dart

Fair, repeatable and verifiable random lotteries: draw k of n without replacement, with seeds you
can commit to, candidate IDs that do not depend on the order of your list, prize stages, and
sessions you can save and continue. The same results as every other binding (see
[docs/SPEC.md](../../docs/SPEC.md)).

It calls the native library through `dart:ffi` (the only dependency is `package:ffi`). It works
with the Dart VM and Flutter on desktop; not on the web.

```dart
import 'package:sortition/sortition.dart';

const people = ['alice', 'bob', 'carol', 'dave'];
final picks = Lottery.drawItems(people, 2, seed: Lottery.seedFromText('my seed'));
// picks[i].index is the position in people; the winners are dave, bob
print(Lottery.seedCommit('my seed')); // publish this before the draw
```

## The native library

Build it once (`cargo build --release` at the root of the repository) and tell Dart where it is:

- set `LIBSORTITION_PATH` to the file (`libsortition.so`, `libsortition.dylib` or `sortition.dll`), or
- put it in `target/release` of the working directory or any directory above it, or
- put it where the system's library search path finds it.

## What it has

| | |
|---|---|
| `Lottery.draw(n, k, {seed})` | k distinct numbers from 0..n-1 |
| `Lottery.drawText`, `Lottery.drawData` | lines of text, or an array inside JSON, YAML or TOML |
| `Lottery.seedFromText`, `Lottery.seedCommit` | a seed from a string, and its commitment (SHA-256) |
| `Lottery.itemIds`, `Lottery.itemFingerprint` | order-independent candidate IDs and set fingerprint |
| `Lottery.drawItems`, `Lottery.drawStages` | candidates (strings or lists of bytes) and prize stages |
| `Session`, `ItemSession` | draw in steps; `save`/`load`, `seal`/`unseal`, idempotency keys with `draw(k, key: ...)` |

A seed is a 64-bit pattern in a Dart `int`, so it can be negative; `seed.toUnsigned(64)` prints
the unsigned number the other bindings show. Without a seed the draw uses the operating system's
randomness. Errors are `SortitionException` with a `code`. Call `close()` on a session when done
(a forgotten one is freed when it is garbage collected). A session is not thread-safe.

`save()` output contains the random generator's state: do not hand it to someone you do not trust.
`seal` it first.

## Development

```sh
cargo build --release   # at the root of the repository
cd bindings/dart
dart pub get
dart test
```
