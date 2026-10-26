# libsortition

[English](README.md)

偏りのない無作為抽選のための小さなライブラリです。`n` 個の候補から `k` 個を重複なしで選びます（非復元抽出）。C ABI を持つ Rust 製のコアが 1 つあり、Node.js、PHP、Python、Ruby、WebAssembly 向けのバインディングがあります。

> **状態: 実験的（0.1）。** インターフェースは 1.0 までに変わるかもしれません。

## できること

- **非復元抽出。** `draw(n, k)` は `0..n` から重複しない `k` 個の数を、選ばれた順に返します。
- **必要なときは再現できる。** シードなしなら OS の乱数源を使い、シード（`u64`）を指定すると同じシードで同じ結果になります。
- **テキストやデータから候補を取る。** 1 行 1 候補のテキスト（前後の空白は除き、空行は捨てます）か、JSON・YAML・TOML の中にある配列（`a.b.0` のような `path` で場所を指定）。YAML のエイリアス・独自タグ・複数ドキュメントは拒否します。
- **セッションで続きから引ける。** セッションは `0..n` を母集団として、呼ぶたびに非復元抽出します。状態は JSON で保存して後から復元できるので、DB やファイルに預けて抽選を続けられます。シードを指定すれば、何回に分けて引いても `draw(n, k1 + k2 + ..., seed)` を一度に引いたときと同じ並びになります。形式が壊れている、または中身が矛盾している保存状態は拒否します。
- **C ABI。** ヘッダは [core/include/sortition.h](core/include/sortition.h) で、`sortition_draw`、`sortition_draw_text`、`sortition_draw_data`、`sortition_session_*`、`sortition_free`、`sortition_last_error`、`sortition_version` があります。

## 構成

| パス | 内容 |
|---|---|
| `core/` | Rust のクレート（`cdylib` と `rlib`）と C ヘッダ |
| `bindings/node/` | Node.js（N-API ネイティブアドオン） |
| `bindings/php/` | PHP 拡張 |
| `bindings/python/` | Python（ctypes、依存なし） |
| `bindings/ruby/` | Ruby（Fiddle、標準ライブラリのみ） |
| `bindings/wasm/` | ブラウザと Node 向けの WebAssembly（依存なし）とデモ |

## ビルド

```sh
cargo build --release          # target/release/libsortition.so（.dylib、.dll）
cargo build --release --no-default-features   # YAML と TOML のパーサなし
```

`yaml` と `toml` の feature は既定で有効です。

バインディングは、上でビルドした共有ライブラリを読み込みます。Python と Ruby は `LIBSORTITION_PATH`、モジュールの隣、`target/release` の順に探します。

```python
import sys; sys.path.insert(0, "bindings/python")
import sortition

sortition.draw(10, 3)                       # 例: [7, 2, 5]
sortition.draw(10, 3, seed=42)              # 毎回同じ結果
sortition.draw_text("alice\nbob\ncarol\n", 2)
sortition.draw_data('{"members": ["a", "b", "c"]}', 2, "json", path="members")

lot = sortition.Session(100, seed=1)        # セッション: 何回かに分けて引く
lot.draw(3)                                 # 最初の 3 個
state = lot.save()                          # JSON 文字列。どこに保存してもよい
sortition.Session.load(state).draw(2)       # 次の 2 個。途切れなかったときと同じ続き
```

セッションはどのバインディングにもあります: `Session`（Python）、`Sortition::Session`（Ruby）、`createSession` / `loadSession`（Node.js と WebAssembly）、`SortitionSession`（PHP）。

```sh
cd bindings/node && npm install && npm run build && npm test   # Node.js
bindings/wasm/build.sh                                          # 事前に rustup target add wasm32-unknown-unknown
```

## ライセンス

[MIT](LICENSE-MIT) または [Apache-2.0](LICENSE-APACHE) のどちらかを選んで使えます。特に明示しない限り、このプロジェクトに取り込む目的で意図的に提出された貢献は、追加の条件なしに上記と同じライセンスで提供されたものとします。
