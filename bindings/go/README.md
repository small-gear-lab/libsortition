# sortition for Go

Fair, repeatable and verifiable random lotteries: draw k of n without replacement, with seeds you
can commit to, candidate IDs that do not depend on the order of your list, prize stages, and
sessions you can save and continue. The same results as every other binding (see
[docs/SPEC.md](../../docs/SPEC.md)).

There is no cgo and no native library to install: the Rust core is embedded as WebAssembly and runs
with [wazero](https://wazero.io/). It works wherever Go does.

```go
import "github.com/small-gear-lab/libsortition/bindings/go"

people := []string{"alice", "bob", "carol", "dave"}
picks, err := sortition.DrawItems(people, 2, sortition.WithSeed(sortition.SeedFromText("my seed")))
// picks[i].Index is the position in people; the winners are dave, bob
fmt.Println(sortition.SeedCommit("my seed")) // publish this before the draw
```

The import path ends in `go`, so the package name is `sortition` (give it an alias if you like).

## What it has

| | |
|---|---|
| `Draw(n, k, opts...)` | k distinct numbers from 0..n-1 |
| `DrawText`, `DrawData` | lines of text, or an array inside JSON, YAML or TOML |
| `SeedFromText`, `SeedCommit` | a seed from a string, and its commitment (SHA-256) |
| `ItemIDs`, `ItemFingerprint` | order-independent candidate IDs and set fingerprint |
| `DrawItems`, `DrawStages` | candidates (strings or `[]byte`; the same winners whatever the order) and prize stages |
| `NewSession`, `NewItemSession` | draw in steps; `Save`/`Load`, `Seal`/`Unseal`, idempotency keys with `DrawKeyed` |

Pass `sortition.WithSeed(...)` for a repeatable result; without it the draw uses the operating
system's randomness. Errors are `*sortition.Error` with a `Code` (`ErrInvalid`, `ErrState`,
`ErrConflict`). Sessions have a `Close` method (a forgotten session is freed by the garbage
collector too). Everything is safe for concurrent use, but calls into the core are serialized.

`Save` output contains the random generator's state: do not hand it to someone you do not trust.
`Seal` it first.

## Development

```sh
cd bindings/go
go test ./...
go generate   # rebuilds sortition.wasm from the core (needs Rust and the wasm32 target)
```

`sortition.wasm` is committed on purpose, so that `go get` works. Rebuild it whenever the core changes.
