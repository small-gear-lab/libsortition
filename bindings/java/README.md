# sortition for Java

Fair, repeatable and verifiable random lotteries: draw k of n without replacement, with seeds you
can commit to, candidate IDs that do not depend on the order of your list, prize stages, and
sessions you can save and continue. The same results as every other binding (see
[docs/SPEC.md](../../docs/SPEC.md)).

It calls the native library through the Foreign Function & Memory API (`java.lang.foreign`), so it
needs **Java 25** (the current LTS; the API is final since Java 22) and no JNI glue. It works from
Kotlin, Scala and any other JVM language too.

```java
import io.github.smallgearlab.sortition.*;

var people = List.of("alice", "bob", "carol", "dave");
var picks = Lottery.drawItems(Items.of(people), 2, Lottery.seedFromText("my seed"));
// picks.get(i).index() is the position in people; the winners are dave, bob
System.out.println(Lottery.seedCommit("my seed")); // publish this before the draw
```

```kotlin
val winners = Lottery.drawItems(Items.of("alice", "bob", "carol", "dave"), 2, Lottery.seedFromText("my seed"))
Session.create(100).use { session -> session.draw(3, "request-1") }
```

## The native library

Get it from the [releases page](https://github.com/small-gear-lab/libsortition/releases) (the `lib/` folder of the archive for your platform), or build it once (`cargo build --release` at the root of the repository). Then tell Java where it is:

- set `LIBSORTITION_PATH` to the file (`libsortition.so`, `libsortition.dylib` or `sortition.dll`), or
- put it in `target/release` of the working directory or any directory above it, or
- put it where the system's library search path finds it.

Run the JVM with `--enable-native-access=ALL-UNNAMED` (or the name of your module) to silence the
warning about native access.

## What it has

| | |
|---|---|
| `Lottery.draw(n, k[, seed])` | k distinct numbers from 0..n-1 |
| `Lottery.drawText`, `Lottery.drawData` | lines of text, or an array inside JSON, YAML or TOML |
| `Lottery.seedFromText`, `Lottery.seedCommit` | a seed from a string, and its commitment (SHA-256) |
| `Lottery.itemIds`, `Lottery.itemFingerprint` | order-independent candidate IDs and set fingerprint |
| `Lottery.drawItems`, `Lottery.drawStages` | candidates (`Items.of(strings)` or `Items.ofBytes(...)`) and prize stages |
| `Session`, `ItemSession` | draw in steps; `save`/`load`, `seal`/`unseal`, idempotency keys with `draw(k, key)` |

Give a seed for a repeatable result; without one the draw uses the operating system's randomness.
Errors are `SortitionException` (an `IllegalArgumentException`) with a `code()`. Sessions are
`AutoCloseable` (a forgotten one is freed by a `Cleaner` too) and are not thread-safe.

`save()` output contains the random generator's state: do not hand it to someone you do not trust.
`seal` it first.

## Development

```sh
cargo build --release          # the native library, at the root of the repository
cd bindings/java
./gradlew test                 # Java 25; the tests read test-vectors/vectors.json
```
