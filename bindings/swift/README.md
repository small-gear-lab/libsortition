# sortition for Swift

Fair, repeatable and verifiable random lotteries: draw k of n without replacement, with seeds you
can commit to, candidate IDs that do not depend on the order of your list, prize stages, and
sessions you can save and continue. The same results as every other binding (see
[docs/SPEC.md](../../docs/SPEC.md)).

It links the native library through a small C module; there is no other dependency (no Foundation).

```swift
import Sortition

let people = ["alice", "bob", "carol", "dave"]
let picks = try Lottery.drawItems(people, k: 2, seed: Lottery.seedFromText("my seed"))
// picks[i].index is the position in people; the winners are dave, bob
print(Lottery.seedCommit("my seed"))  // publish this before the draw
```

## Installing

`Package.swift` is at the root of the repository (SwiftPM requires it there):

```swift
.package(url: "https://github.com/small-gear-lab/libsortition", branch: "main"),
// ...
.product(name: "Sortition", package: "libsortition"),
```

Build the native library once (`cargo build --release`), and tell the linker where it is:

```sh
swift build -Xlinker -L/path/to/libsortition/target/release
swift test  -Xlinker -L$PWD/target/release -Xlinker -rpath -Xlinker $PWD/target/release
```

(An app needs the library at run time too: install it where the dynamic linker finds it, or add an rpath.)

## What it has

| | |
|---|---|
| `Lottery.draw(n:k:seed:)` | k distinct numbers from 0..<n |
| `Lottery.drawText`, `Lottery.drawData` | lines of text, or an array inside JSON, YAML or TOML |
| `Lottery.seedFromText`, `Lottery.seedCommit` | a seed from a string, and its commitment (SHA-256) |
| `Lottery.itemIDs`, `Lottery.itemFingerprint` | order-independent candidate IDs and set fingerprint |
| `Lottery.drawItems`, `Lottery.drawStages` | candidates (`[String]` or `[[UInt8]]`) and prize stages |
| `Session`, `ItemSession` | draw in steps; `save()`/`init(state:)`, `seal`/`init(sealed:key:)`, idempotency keys with `draw(_:key:)` |

Without a seed the draw uses the operating system's randomness. Errors are `SortitionError` with a
`code`. A session frees its native memory when it goes away, and is not thread-safe.

`save()` output contains the random generator's state: do not hand it to someone you do not trust.
`seal` it first.
