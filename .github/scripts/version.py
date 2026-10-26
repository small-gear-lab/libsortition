#!/usr/bin/env python3
"""Keeps the version number the same everywhere it is written.

  version.py show            print the version of core/Cargo.toml
  version.py check [X.Y.Z]   fail unless every place has the same version (and X.Y.Z, if given; a leading "v" is fine)
  version.py bump X.Y.Z      rewrite every place

Run `cargo check` after a bump (it updates Cargo.lock), and `go generate` in bindings/go (the embedded
WebAssembly carries the version). See "Releasing" in AGENTS.md.
"""
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(subprocess.check_output(["git", "rev-parse", "--show-toplevel"], text=True).strip())

# (path, regex with exactly one group around the version, how many matches are expected)
PLACES = [
    ("core/Cargo.toml", r'(?m)^version = "([^"]+)"', 1),
    ("cli/Cargo.toml", r'(?m)^version = "([^"]+)"', 1),
    ("cli/Cargo.toml", r'(?m)^sortition = \{ version = "([^"]+)"', 1),
    ("Cargo.lock", r'(?m)^name = "sortition"\nversion = "([^"]+)"', 1),
    ("Cargo.lock", r'(?m)^name = "sortition-cli"\nversion = "([^"]+)"', 1),
    ("bindings/wasm/package.json", r'(?m)^  "version": "([^"]+)"', 1),
    ("bindings/node/package.json", r'(?m)^  "version": "([^"]+)"', 1),
    ("bindings/node/package-lock.json", r'(?m)^(?:  |      )"version": "([^"]+)"(?=,\n(?:  |      )"?(?:license|lockfileVersion|hasInstallScript|requires))', 2),
    ("bindings/dart/pubspec.yaml", r'(?m)^version: (\S+)', 1),
    ("bindings/dotnet/Sortition/Sortition.csproj", r"<Version>([^<]+)</Version>", 1),
    ("bindings/java/build.gradle.kts", r'(?m)^version = "([^"]+)"', 1),
    ("bindings/php/php_sortition.h", r'#define PHP_SORTITION_VERSION "([^"]+)"', 1),
    # the tests that check that the native version is reported
    ("bindings/dart/test/binding_test.dart", r"Lottery\.version, '([^']+)'", 1),
    ("bindings/dotnet/Sortition.Tests/BindingTests.cs", r'Assert\.Equal\("([^"]+)", Lottery\.Version\)', 1),
    ("bindings/go/sortition_test.go", r'v != "([^"]+)"', 1),
    ("bindings/java/src/test/java/io/github/smallgearlab/sortition/BindingTest.java", r'assertEquals\("([^"]+)", Lottery\.version\(\)\)', 1),
    ("bindings/node/test/sortition.test.js", r"s\.version\(\), '([^']+)'", 1),
    ("bindings/swift/Tests/SortitionTests/BindingTests.swift", r'Lottery\.version == "([^"]+)"', 1),
    ("bindings/wasm/test/sortition.test.mjs", r's\.version\(\), "([^"]+)"', 1),
]

SEMVER = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$")


def found():
    """Yields (path, versions found there, expected count)."""
    for path, pattern, expected in PLACES:
        text = (ROOT / path).read_text(encoding="utf-8")
        yield path, re.findall(pattern, text), expected


def core_version():
    return re.search(r'(?m)^version = "([^"]+)"', (ROOT / "core/Cargo.toml").read_text()).group(1)


def check(want):
    problems = []
    for path, versions, expected in found():
        if len(versions) != expected:
            problems.append(f"{path}: expected {expected} place(s) for the version, found {len(versions)}")
        for v in versions:
            if v != want:
                problems.append(f"{path}: {v} (want {want})")
    for problem in problems:
        print(f"::error::{problem}" if "GITHUB_ACTIONS" in __import__("os").environ else problem, file=sys.stderr)
    return 1 if problems else 0


def bump(new):
    for path, pattern, expected in PLACES:
        file = ROOT / path
        text = file.read_text(encoding="utf-8")
        count = 0

        def replace(m):
            nonlocal count
            count += 1
            return m.group(0)[: m.start(1) - m.start(0)] + new + m.group(0)[m.end(1) - m.start(0):]

        text = re.sub(pattern, replace, text)
        if count != expected:
            sys.exit(f"{path}: expected {expected} place(s) for the version, found {count}")
        file.write_text(text, encoding="utf-8")
    print(f"bumped to {new}: now run `cargo check`, then `go generate` in bindings/go")


def main(argv):
    cmd = argv[1] if len(argv) > 1 else ""
    if cmd == "show":
        print(core_version())
        return 0
    if cmd == "check":
        want = (argv[2] if len(argv) > 2 else core_version()).removeprefix("v")
        if not SEMVER.match(want):
            sys.exit(f"not a version: {want}")
        return check(want)
    if cmd == "bump" and len(argv) == 3:
        if not SEMVER.match(argv[2]):
            sys.exit(f"not a version: {argv[2]}")
        bump(argv[2])
        return 0
    sys.exit(__doc__)


if __name__ == "__main__":
    sys.exit(main(sys.argv))
