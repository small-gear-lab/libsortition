#!/usr/bin/env python3
"""GitHub Pages（gh-pages ブランチ）に置く WebAssembly 版のファイルを組み立てる。

  pages.py assemble --repo <リポジトリのルート> --dest <出力先> --name <vX.Y.Z | next> --commit <SHA>
      公開するファイルを出力先に集め、各ファイルの SHA-384（SRI の形式）を書いた manifest.json を作る
  pages.py index --root <gh-pages の作業ツリー>
      バージョンのフォルダを数えて、ルートの index.html を作り直す
"""
import argparse
import base64
import hashlib
import html
import json
import re
import shutil
import sys
from pathlib import Path

# bindings/wasm から、公開先の同じ相対位置へ置くファイル（デモは ../sortition.js のように参照する）
WASM_FILES = [
    "sortition.js", "sortition.d.ts", "persist.js", "persist.d.ts", "sortition.wasm", "demo/index.html",
]
# リポジトリのルートから、各フォルダへ添えるファイル（再配布にはライセンスの写しが要る）
LEGAL_FILES = ["LICENSE-MIT", "LICENSE-APACHE", "NOTICE"]

VERSION_DIR = re.compile(r"^v(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?$")


def sri(path: Path) -> str:
    return "sha384-" + base64.b64encode(hashlib.sha384(path.read_bytes()).digest()).decode()


def assemble(repo: Path, dest: Path, name: str, commit: str) -> None:
    if dest.exists():
        shutil.rmtree(dest)
    sources = [(repo / "bindings" / "wasm" / f, f) for f in WASM_FILES] + [(repo / f, f) for f in LEGAL_FILES]
    for src, rel in sources:
        if not src.is_file():
            sys.exit(f"missing file: {src}")
        target = dest / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(src, target)
    files = {rel: {"integrity": sri(dest / rel), "bytes": (dest / rel).stat().st_size} for _, rel in sources}
    manifest = {"name": name, "commit": commit, "files": dict(sorted(files.items()))}
    (dest / "manifest.json").write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def version_key(name: str):
    m = VERSION_DIR.match(name)
    major, minor, patch, pre = m.groups()
    # 同じ番号なら、プレリリースより安定版を新しいとみなす
    return (int(major), int(minor), int(patch), pre is None, pre or "")


def read_manifest(path: Path):
    try:
        return json.loads((path / "manifest.json").read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}


def index(root: Path) -> None:
    versions = sorted((d.name for d in root.iterdir() if d.is_dir() and VERSION_DIR.match(d.name)),
                      key=version_key, reverse=True)
    rows = []

    def row(label: str, note: str, folder: str) -> None:
        m = read_manifest(root / folder)
        commit = m.get("commit", "")[:7]
        rows.append(
            f'<tr><td><a href="{html.escape(folder)}/demo/">{html.escape(label)}</a></td>'
            f"<td>{html.escape(note)}</td>"
            f'<td><a href="{html.escape(folder)}/manifest.json">manifest</a></td>'
            f"<td><code>{html.escape(commit)}</code></td></tr>"
        )

    if (root / "latest").is_dir():
        row("latest", "the newest stable release", "latest")
    if (root / "next").is_dir():
        row("next", "the current main branch (may change at any time)", "next")
    for v in versions:
        row(v, "release (never changes)", v)

    body = f"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>libsortition</title>
<style>
  body {{ font: 16px/1.6 system-ui, sans-serif; max-width: 46rem; margin: 2rem auto; padding: 0 1rem; }}
  table {{ border-collapse: collapse; width: 100%; }}
  th, td {{ text-align: left; padding: .3rem .6rem; border-bottom: 1px solid #8884; }}
  pre {{ overflow-x: auto; padding: .8rem; background: #8881; }}
</style>
</head>
<body>
<h1>libsortition</h1>
<p>Builds of the WebAssembly binding and the demo. Each folder holds <code>sortition.js</code>,
<code>persist.js</code>, <code>sortition.wasm</code>, the type definitions, the licenses and a
<code>manifest.json</code> with the SHA-384 hash of every file.</p>
<p><strong>For a draw that others must trust, use a versioned folder</strong> (for example
<code>/v0.1.0/</code>), not <code>latest</code> or <code>next</code>: a versioned folder never changes, and you can
check its files against the manifest.</p>
<pre>import {{ load }} from "https://small-gear-lab.github.io/libsortition/{html.escape(versions[0] if versions else "vX.Y.Z")}/sortition.js";
const sortition = await load();</pre>
<table>
<thead><tr><th>Folder</th><th></th><th></th><th>Commit</th></tr></thead>
<tbody>
{chr(10).join(rows) if rows else '<tr><td colspan="4">Nothing has been published yet.</td></tr>'}
</tbody>
</table>
<p><a href="https://github.com/small-gear-lab/libsortition">Source and documentation on GitHub</a></p>
</body>
</html>
"""
    (root / "index.html").write_text(body, encoding="utf-8")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="cmd", required=True)
    a = sub.add_parser("assemble")
    a.add_argument("--repo", type=Path, required=True)
    a.add_argument("--dest", type=Path, required=True)
    a.add_argument("--name", required=True)
    a.add_argument("--commit", required=True)
    i = sub.add_parser("index")
    i.add_argument("--root", type=Path, required=True)
    args = parser.parse_args()
    if args.cmd == "assemble":
        assemble(args.repo, args.dest, args.name, args.commit)
    else:
        index(args.root)


if __name__ == "__main__":
    main()
