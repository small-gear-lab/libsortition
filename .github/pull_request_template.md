## What and why

<!-- What does this change, and why? Link the issue if there is one. -->

## Checklist

- [ ] The tests pass (`cargo test --workspace`, and the binding's tests if you touched one)
- [ ] If the core's behavior or API changed: every binding follows, and `test-vectors/` and `docs/SPEC.md` are updated (`SORTITION_UPDATE_VECTORS=1 cargo test -p sortition vectors`)
- [ ] If results for a given seed change: the algorithm name or the saved-format version changed, and `CHANGELOG.md` says so
- [ ] A user-visible change is in `CHANGELOG.md` under "Unreleased"

See [AGENTS.md](../AGENTS.md) for how the repository is organized and the rules that are easy to break.
