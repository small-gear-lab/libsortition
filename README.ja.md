# libsortition

[English](README.md)

偏りのない無作為抽選のための小さなライブラリです。`n` 個の候補から `k` 個を重複なしで選びます（非復元抽出）。C ABI を持つ Rust 製のコアが 1 つあり、Node.js、PHP、Python、Ruby、WebAssembly 向けのバインディングがあります。

> **状態: 実験的（0.1）。** インターフェースは 1.0 までに変わるかもしれません。

## できること

- **非復元抽出。** `draw(n, k)` は `0..n` から重複しない `k` 個の数を、選ばれた順に返します。
- **必要なときは再現できる。** シードなしなら OS の乱数源を使い、シード（`u64`）を指定すると同じシードで同じ結果になります。
- **テキストやデータから候補を取る。** 1 行 1 候補のテキスト（前後の空白は除き、空行は捨てます）か、JSON・YAML・TOML の中にある配列（`a.b.0` のような `path` で場所を指定）。YAML のエイリアス・独自タグ・複数ドキュメントは拒否します。
- **セッションで続きから引ける。** セッションは `0..n` を母集団として、呼ぶたびに非復元抽出します。状態は JSON で保存して後から復元できるので、DB やファイルに預けて抽選を続けられます。シードを指定すれば、何回に分けて引いても `draw(n, k1 + k2 + ..., seed)` を一度に引いたときと同じ並びになります。形式が壊れている、または中身が矛盾している保存状態は拒否します。
- **リロードや HTTP セッション切れのあとも、サーバーなしで続きから引ける。** 乱数には暗号論的な乱数（ChaCha20）を使い、その状態をセッションと一緒に保存します。同じ保存状態からは必ず同じ次の抽選になります。どの抽選にも羃等キーを付けられ、再送したリクエストは二重に引かず、前回の結果を返します。ブラウザでは `persist.js` が IndexedDB にセッションを保存します。サーバーを挟む場合は、`seal()` で状態を改ざんできないトークン（XChaCha20-Poly1305）にして、ブラウザに渡せます。
- **任意のバイト列を候補にして、位置に依らない一意な ID で識別する。** 任意のバイト列を渡すと、各候補に `SHA-256(接頭辞 ‖ バイト列 ‖ 出現番号)` の ID が付きます。出現番号は同じバイト列どうしを区別するためのものです。抽選は ID の昇順に並べてから引くので、同じ候補集合とシードなら、入力の並びが違っても同じ候補が当選します。候補集合の指紋（昇順の ID のハッシュ）を抽選前に公開しておけば、あとから検証できます。バイト列版のセッションは、引いた ID と指紋を保存し、同じ候補集合（並びは問わない）でだけ再開できます。
- **C ABI。** ヘッダは [core/include/sortition.h](core/include/sortition.h) で、`sortition_draw`、`sortition_draw_text`、`sortition_draw_data`、`sortition_session_*`、`sortition_item_*`、`sortition_items_draw`、`sortition_free`、`sortition_last_error`、`sortition_version` があります。

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

```python
people = [b"alice", b"bob", "carol"]        # bytes か str（UTF-8）
sortition.item_fingerprint(people)          # 並びに依らない。シードと一緒に公開する
picks = sortition.draw_items(people, 2, seed=42)   # [Pick(index=1, id="53…"), …]
lot = sortition.ItemSession(people, seed=42)
lot.draw(1)
state = lot.save()                          # 引いた ID と指紋を含む
sortition.ItemSession.load(state, reversed(people)).draw(1)   # 同じ集合なら並びは自由
```

バイト列の候補もどのバインディングにもあります: `draw_items` / `ItemSession`（Python）、`Sortition.draw_items` / `Sortition::ItemSession`（Ruby）、`drawItems` / `createItemSession` / `loadItemSession`（Node.js と WebAssembly）、`sortition_draw_items` / `SortitionItemSession`（PHP）。ID と指紋は小文字の 16 進文字列で、当選は `index`（渡したリストの中の位置）と `id` です。

セッションはどのバインディングにもあります: `Session`（Python）、`Sortition::Session`（Ruby）、`createSession` / `loadSession`（Node.js と WebAssembly）、`SortitionSession`（PHP）。

### リロードや HTTP セッション切れのあとに続きから引く

**サーバーなし**（静的なページと WebAssembly）の場合: `bindings/wasm/persist.js` が、引くたびにセッションを IndexedDB（なければ `localStorage`、それもなければメモリ）に保存します。

```js
import { load } from "./sortition.js";
import { createPersistence } from "./persist.js";

const lots = createPersistence(await load());
const lot = await lots.createItems(["alice", "bob", "carol"], { seed: 42n });
await lot.draw(1);

// ……ページをリロードした、またはタブを閉じて開き直した……
const again = await lots.resume(lot.id);   // その id で保存されたものがなければ null
await again.draw(1);                       // 止まったところから続き。当選者は重複しない
```

- `draw` には羃等キーが付きます（`{ key }` を渡さなければ自動で作ります）。保存に失敗してやり直しても、二重に引きません。同じキーで別の `k` を渡すとエラー（コード `-9`）です。
- 保存は、保存されている `revision` が読んだときのものと一致するときだけ成功します。Web Locks で、タブ同士が同時に引くのも防ぎます。古い状態のタブは、保存されている状態を読み直して続きから引きます。
- `export()` / `import()` で、セッションをテキストにして別の端末へ移せます。乱数の内部状態を含むので、本人以外には渡さないでください。
- `demo/index.html` がこれを使っています。メインの抽選のあと、辞退が出たときに補欠を 1 人ずつ引け、リロードしても全部元に戻ります。

**サーバーあり**（たとえば Workers や Lambda の関数で、データベースを持たない場合）: 保存した状態には乱数の内部状態が入っているので、持っている人は以降の当選者を予測でき、偽の状態も作れます。状態をそのままブラウザに渡さず、封をしてください。鍵はサーバーに置きます。

```python
key = bytes.fromhex(os.environ["SORTITION_KEY"])   # 32 バイトの乱数。サーバーだけが持つ
lot = sortition.Session.unseal(token, key)         # 偽造・改ざん・鍵違いは SortitionError (-8)
picks = lot.draw(3, key=request_id)                # 再送されたリクエストは、同じ結果を返す
new_token = lot.seal(key)                          # picks と new_token をブラウザに返す
```

次の抽選は保存状態だけで決まるので、古いトークンを送り直しても同じ結果になります。すでに進んだトークンが再送された場合は、羃等キーが二重に引くのを防ぎます。セッションを保存するサーバーなら、レコードを上書きする前に `revision` を比べることもできます。考え方はどのバインディングでも同じです（`seal` / `unseal` / `sealKeyId`）。

主催者が結果を捨てて、新しいセッションを作り直すことは、この仕組みでは防げません。抽選の前に、セッションの `id`、候補集合の指紋、seed のコミット値を公開してください。

```sh
cd bindings/node && npm install && npm run build && npm test   # Node.js
bindings/wasm/build.sh                                          # 事前に rustup target add wasm32-unknown-unknown
```

## ライセンス

[MIT](LICENSE-MIT) または [Apache-2.0](LICENSE-APACHE) のどちらかを選んで使えます。特に明示しない限り、このプロジェクトに取り込む目的で意図的に提出された貢献は、追加の条件なしに上記と同じライセンスで提供されたものとします。
