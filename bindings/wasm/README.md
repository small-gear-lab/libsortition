# libsortition-wasm

Fair, repeatable and verifiable random lotteries for browsers and Node.js: it picks `k` of `n` candidates without replacement, in a way that anyone can repeat and check afterwards. This is the WebAssembly build of [libsortition](https://github.com/small-gear-lab/libsortition). It has no dependencies and needs no compiler.

> **Status: experimental (0.1).** The interface may change before 1.0.

```js
import { load, seedFromText } from "libsortition-wasm";

const s = await load();
const people = ["alice", "bob", "carol", "dave"];
const seed = await seedFromText("my seed");   // publish the SHA-256 of the seed string before the draw

for (const pick of s.drawItems(people, 2, { seed })) console.log(people[pick.index]);   // dave, bob
```

The winners depend only on the set of candidates and the seed, not on the order of the list, and they are the same in every language libsortition supports and in its `sortition` command. In a browser, import `sortition.js` from a versioned build under <https://small-gear-lab.github.io/libsortition/> (each folder has a `manifest.json` with the SHA-384 of its files).

- `drawItems`, `drawStages`, `itemFingerprint`: candidates (strings or bytes) identified by IDs that do not depend on their position; prize stages; the fingerprint of the candidate set.
- `createSession`, `createItemSession`, `loadSession`, `seal`: draw in steps, save and restore the state, retry safely with idempotency keys, and hand a sealed state to a browser.
- `libsortition-wasm/persist.js`: keeps sessions in IndexedDB so that a reload loses nothing.

The README of the repository has the full guide, [docs/SPEC.md](https://github.com/small-gear-lab/libsortition/blob/main/docs/SPEC.md) writes the algorithm down exactly, and the licenses are MIT or Apache-2.0, at your option.
