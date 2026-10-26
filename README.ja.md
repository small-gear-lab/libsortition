# libsortition

[![CI](https://github.com/small-gear-lab/libsortition/actions/workflows/ci.yml/badge.svg)](https://github.com/small-gear-lab/libsortition/actions/workflows/ci.yml)

[English](README.md)

公平な無作為抽選のための小さなライブラリです。`n` 個の候補から `k` 個を重複なしで選び、あとから誰でも同じ抽選を繰り返して確かめられます。C ABI を持つ Rust 製のコアが 1 つあり、Node.js、PHP、Python、Ruby、WebAssembly 向けのバインディングと、`sortition` コマンドがあります。

> **状態: 実験的（0.1）。** インターフェースは 1.0 までに変わるかもしれません。

**試す:** [デモ](https://small-gear-lab.github.io/libsortition/next/demo/)では、貼り付けたリストから賞ごとに抽選できます。すべてのビルドは <https://small-gear-lab.github.io/libsortition/> に並んでいます。

## クイックスタート

次のプログラムは、同じ候補と seed なら、同じ当選者（`dave`、次に `bob`）を出します。[examples/c](examples/c/demo.c) の C と C++ のプログラムも同じです。

**コマンドライン**（[詳しく](#コマンドライン)）:

```sh
cargo install --path cli                 # チェックアウトから
printf 'alice\nbob\ncarol\ndave\n' > names.txt
sortition draw names.txt --seed "my seed" -k 2
```

**Python**（チェックアウトから）:

```python
import sys; sys.path.insert(0, "bindings/python")   # `cargo build --release` のあと
import sortition

people = ["alice", "bob", "carol", "dave"]
seed = sortition.seed_from_text("my seed")
for pick in sortition.draw_items(people, 2, seed=seed):
    print(people[pick.index])
```

**JavaScript**（WebAssembly）:

```js
import { load, seedFromText } from "./sortition.js";   // bindings/wasm。bindings/wasm/build.sh のあと
const s = await load();

const people = ["alice", "bob", "carol", "dave"];
const seed = await seedFromText("my seed");
for (const pick of s.drawItems(people, 2, { seed })) console.log(people[pick.index]);
```

**C と C++**（[examples/c/demo.c](examples/c/demo.c) は両方でビルドできます）: `cargo build --release` のあと、`make -C examples/c run` を実行します。[core/include/sortition.h](core/include/sortition.h) を include して、`libsortition` にリンクしてください。

## どの関数を使うか

当選者がリストの並びに依ってはいけないときは、候補の関数（`draw_items` / `drawItems`、`draw_stages` / `drawStages`、候補のセッション、CLI）を使ってください。各候補はバイト列から ID が決まり、抽選は ID を昇順に並べた上で行われます。ほかの入口（`draw(n, k)`、テキストや JSON・YAML・TOML からの抽選）は、渡したリストの位置を返すので、リストの並びを変えると当選者も変わります。

## できること

- **公平な抽選。** 剰余の偏りのない非復元抽出（棄却法）。`draw(n, k)` は `0..n` から重複しない `k` 個の数を選びます。
- **繰り返せて、確かめられる。** 抽選の前に公開できる seed 文字列の SHA-256（コミット値）と、並びに依らない候補集合の指紋があります。同じ候補と seed なら、どの言語でもコマンドでも同じ当選者になります。規則は [docs/SPEC.md](docs/SPEC.md)（概要は[日本語](docs/SPEC.ja.md)）に書いてあり、[テストベクトル](test-vectors/vectors.json)と、独立した[参照実装](test-vectors/reference.py)があります。
- **任意のバイト列を候補にできる。** 各候補には位置に依らない ID が付くので、当選者はリストの並びに依りません。同じ内容の項目は別の候補のままです。テストの行や、JSON・YAML・TOML の配列も使えます。
- **賞ごとの抽選。** 最初の賞を引き、残りから次の賞を引きます。同じ人が 2 つに当たることはありません。
- **途中で止められるセッション。** 状態を JSON で保存して、あとで続きから引けます。再送されたリクエストは羃等キーで、二重に引きません。封をしたトークン（XChaCha20-Poly1305）で、ブラウザに、読めず変えられない状態を預けられます。`persist.js` は IndexedDB にセッションを保存するので、リロードしても何も失われません。
- **C ABI**（[core/include/sortition.h](core/include/sortition.h)）が、そのすべての土台です。

## インストールとビルド

まだ何も公開していません（0.1）。最初のリリースでは、crates.io（`sortition` と `sortition-cli`）、GitHub Releases（コマンドと共有ライブラリ）、npm（`libsortition-wasm`）、GitHub Pages のビルドを公開する予定です。それまでは、チェックアウトからビルドしてください。

```sh
cargo build --release                         # target/release/libsortition.so（.dylib、.dll）
cargo build --release --no-default-features   # YAML と TOML のパーサなし
cargo install --path cli                      # `sortition` コマンド
bindings/wasm/build.sh                        # WebAssembly のビルド。事前に rustup target add wasm32-unknown-unknown
```

`yaml` と `toml` の feature は既定で有効です。Python と Ruby のバインディングは、上でビルドした共有ライブラリを読み込みます（`LIBSORTITION_PATH`、モジュールの隣、`target/release` の順に探します）。PHP 拡張と Node.js のアドオンは、これにリンクします。

**Node.js:** WebAssembly 版（`libsortition-wasm`）を使ってください。コンパイラが要らず、Node.js でそのまま動きます。`bindings/node` のネイティブアドオン（`cd bindings/node && npm install && npm run build`）は実験用で、Linux と macOS でしかビルドできません。

## 構成

| パス | 内容 |
|---|---|
| `core/` | Rust のクレート（`cdylib` と `rlib`）と C ヘッダ |
| `cli/` | `sortition` コマンド（リストからの抽選と、記録した抽選の検証） |
| `bindings/node/` | Node.js（N-API ネイティブアドオン） |
| `bindings/php/` | PHP 拡張（PHP 8.2 以降） |
| `bindings/python/` | Python（ctypes、依存なし） |
| `bindings/ruby/` | Ruby（Fiddle、標準ライブラリのみ） |
| `bindings/wasm/` | ブラウザと Node 向けの WebAssembly（依存なし）とデモ |

## ライブラリの使い方

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

賞ごとに 1 つずつ引くときは、セッションを作るときに賞の計画を渡します。各賞は、頼んだときに引かれ、間でリロードしても何も失われません。

```js
const lot = await lots.createStaged(["alice", "bob", "carol", "dave", "erin"], [
  { name: "副賞", count: 3 },
  { name: "大賞", count: 1 },
], { seed: 42n });

const first = await lot.drawStage();      // { stage: 0, name: "副賞", winners: [...] }
// ……リロード……
const again = await lots.resume(lot.id);
await again.drawStage(0);                 // 引いた賞を指定すると、同じ当選者を返す
await again.drawStage();                  // 次の賞（大賞）
await again.draw(1);                      // すべての賞のあとなら、補欠を 1 人、続きから
```

結果は、合計人数を一度に引いて、先頭から賞ごとに切り分けたものと同じです。計画は作ったあとに変えられません。前の賞を飛ばして引くことはできず、`draw()` はすべての賞を引くまで使えません。

**サーバーあり**（たとえば Workers や Lambda の関数で、データベースを持たない場合）: 保存した状態には乱数の内部状態が入っているので、持っている人は以降の当選者を予測でき、偽の状態も作れます。状態をそのままブラウザに渡さず、封をしてください。鍵はサーバーに置きます。

```python
key = bytes.fromhex(os.environ["SORTITION_KEY"])   # 32 バイトの乱数。サーバーだけが持つ
lot = sortition.Session.unseal(token, key)         # 偽造・改ざん・鍵違いは SortitionError (-8)
picks = lot.draw(3, key=request_id)                # 再送されたリクエストは、同じ結果を返す
new_token = lot.seal(key)                          # picks と new_token をブラウザに返す
```

次の抽選は保存状態だけで決まるので、古いトークンを送り直しても同じ結果になります。すでに進んだトークンが再送された場合は、羃等キーが二重に引くのを防ぎます。セッションを保存するサーバーなら、レコードを上書きする前に `revision` を比べることもできます。考え方はどのバインディングでも同じです（`seal` / `unseal` / `sealKeyId`）。

主催者が結果を捨てて、新しいセッションを作り直すことは、この仕組みでは防げません。抽選の前に、セッションの `id`、候補集合の指紋、seed のコミット値を公開してください。

## コマンドライン

`sortition` コマンドは、リストからの抽選と、記録した抽選の検証ができます。ライブラリと同じコードで動くので、同じ候補と seed なら、どのバインディングとも同じ当選者になります。

```sh
cargo install --path cli                 # チェックアウトから。`sortition` コマンドが入る

sortition commit "my seed"               # 抽選の前に公開するコミット値と、seed の数値
sortition draw names.txt --seed "my seed" --stage "副賞:3" --stage "大賞:1" --record record.json
sortition verify record.json names.txt --commit <公開しておいたコミット値>
```

`names.txt` は 1 行 1 候補です。記録には、seed の文字列、候補集合の指紋、当選者が入ります。候補のリストを持っている人は誰でも `sortition verify` で確かめられ、リストの並びは問いません。`sortition fingerprint names.txt` で、抽選の前に公開する指紋を出せます。`--random-seed` で seed の文字列を作ることもできます。終了コードは、成功が 0、検証の失敗が 1、使い方や入力の誤りが 2 です。

## 公平さと安全性

libsortition は、便利なだけでなく、公平な抽選であることを目指しています。

ライブラリが引き受けること:

- **偏りのない選び方。** 棄却法で剰余の偏りを取り除くので、どの候補も同じ確率で選ばれます。分布はテストで確かめています。
- **既定で強い乱数源。** シードなしでは、暗号論的な乱数（OS から取った鍵で回す ChaCha20）を使います。シードありでは xoshiro256\*\* を使い、再現はできますが暗号論的ではありません。
- **抽選のあとで確かめられる。** 候補集合には並びに依らない指紋があり、シードは抽選の前にコミット（たとえば SHA-256 のハッシュ）できます。
- **保存しても、やり直してもよい。** 保存した状態には封をできるので、ブラウザに預けても、中身を読まれたり変えられたりしません。羃等キーで、再送されたリクエストが二重に引くのも防ぎます。

引き受けられないこと:

- **主催者が結果を操作するのを止めること。** 都合のいいシードを選ぶ、結果を捨ててやり直す、といったことはコードでは防げません。第三者に信頼してもらう抽選では、先に候補集合の指紋とシードのコミット値を公開し、そのあと誰にも操作できない値（たとえば公開の乱数ビーコン）を混ぜてシードにしてください。
- **レビューの代わりになること。** このライブラリは監査を受けておらず、0.1 の間は変わることもあります。抽選、景品、記録の保存に関する決まりは、ライブラリの範囲外です。

抽選のやり方は、[docs/SPEC.md](docs/SPEC.md)（概要は[日本語](docs/SPEC.ja.md)）に正確に書いてあります。[テストベクトル](test-vectors/vectors.json)と、その文面だけから書いた[参照実装](test-vectors/reference.py)もあり、ほかのプログラムでも同じ抽選を繰り返せます。

`save()` が返すものには、乱数の内部状態が入っています。信頼できない相手には、封をしたトークンを渡してください。

## ライセンス

[MIT](LICENSE-MIT) または [Apache-2.0](LICENSE-APACHE) のどちらかを選んで使えます。著作権者は [NOTICE](NOTICE) に書いてあります。特に明示しない限り、このプロジェクトに取り込む目的で意図的に提出された貢献は、追加の条件なしに上記と同じライセンスで提供されたものとします。
