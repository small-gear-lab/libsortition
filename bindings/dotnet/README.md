# Sortition (.NET)

Fair, repeatable and verifiable random lotteries for .NET (C#, F#, VB.NET): it picks `k` of `n` candidates without replacement, in a way that anyone can repeat and check afterwards. This is a binding of [libsortition](https://github.com/small-gear-lab/libsortition); it calls the native library through P/Invoke.

> **Status: experimental (0.1).** The interface may change before 1.0. It targets .NET 10.

```csharp
using Sortition;

string[] people = ["alice", "bob", "carol", "dave"];
ulong seed = Lottery.SeedFromText("my seed");     // publish Lottery.SeedCommit("my seed") before the draw

foreach (var pick in Lottery.DrawItems(people, 2, seed))
    Console.WriteLine(people[pick.Index]);        // dave, bob
```

The winners depend only on the set of candidates and the seed, not on the order of the list, and they are the same in every language libsortition supports and in its `sortition` command.

- `Lottery.DrawItems`, `Lottery.DrawStages`, `Lottery.ItemFingerprint`: candidates (strings or byte arrays) identified by IDs that do not depend on their position; prize stages; the fingerprint of the candidate set.
- `Session` and `ItemSession`: draw in steps, save and restore the state (`Save`, `Load`), retry safely with idempotency keys (`Draw(k, key)`), and hand a sealed state to a browser (`Seal`, `Unseal`).
- `Lottery.Draw`, `DrawText` and `DrawData` return positions in the list as given, so a reordered list gives different winners.

## Finding the native library

The package needs `libsortition` (`libsortition.so`, `libsortition.dylib` or `sortition.dll`). It is looked up through the `LIBSORTITION_PATH` environment variable, next to the program, in a `target/release` folder above the program or the working directory, and then in the usual places of the system. Build it from a checkout of the repository with `cargo build --release`.

## Build and test

```sh
cargo build --release          # in the repository root
dotnet test bindings/dotnet    # the tests read test-vectors/vectors.json
```

The licenses are MIT or Apache-2.0, at your option.
