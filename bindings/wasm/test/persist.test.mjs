import { test } from "node:test";
import assert from "node:assert/strict";
import { load, SortitionError } from "../sortition.js";
import {
  createPersistence, memoryStore, localStorageStore, ConflictError, NotFoundError,
} from "../persist.js";

const s = await load();
const names = ["alice", "bob", "carol", "dave", "erin", "frank", "grace", "heidi"];

test("数値のセッション: 引くたびに保存され、リロード後（resume）も続きから引ける", async () => {
  const lots = createPersistence(s, { store: memoryStore() });
  const lot = await lots.create({ n: 200, seed: 9n, label: "テスト" });
  const got = await lot.draw(20);
  lot.close(); // リロードしたつもり
  const again = await lots.resume(lot.id);
  got.push(...(await again.draw(30)));
  assert.deepEqual(got, s.draw(200, 50, { seed: 9n }));
  assert.deepEqual([again.remaining, again.drawnCount, again.revision], [150, 50, 2]);
  assert.deepEqual(again.drawn, got);
  assert.equal(await lots.resume("0".repeat(32)), null);
});

test("候補のセッション: 文字列とバイナリの候補も一緒に保存され、復元できる", async () => {
  const lots = createPersistence(s, { store: memoryStore() });
  const items = [...names, new Uint8Array([0, 255, 1]), ""];
  const lot = await lots.createItems(items, { seed: 3n });
  const first = await lot.draw(4);
  lot.close();
  const again = await lots.resume(lot.id);
  const rest = await again.draw(6);
  const whole = s.drawItems(items, 10, { seed: 3n });
  assert.deepEqual([...first, ...rest].map((p) => p.id), whole.map((p) => p.id));
  assert.deepEqual(again.drawn, whole.map((p) => p.id));
});

test("同じ羃等キーで呼び直しても二重に引かない", async () => {
  const lots = createPersistence(s, { store: memoryStore() });
  const lot = await lots.create({ n: 50 });
  const a = await lot.draw(3, { key: "request-1" });
  const b = await lot.draw(3, { key: "request-1" });
  assert.deepEqual(a, b);
  assert.deepEqual([lot.drawnCount, lot.revision], [3, 1]);
  await assert.rejects(lot.draw(4, { key: "request-1" }), (e) => e instanceof SortitionError && e.code === -9);
  // キーを省略すると呼び出しごとに別のキー: 別の抽選になる
  await lot.draw(2);
  assert.equal(lot.drawnCount, 5);
});

test("別のタブが先に進めていても、読み直して続きから引く（重複しない）", async () => {
  const lots = createPersistence(s, { store: memoryStore() });
  const tab1 = await lots.create({ n: 100 });
  const tab2 = await lots.resume(tab1.id);
  const a = await tab1.draw(3);
  const b = await tab2.draw(3); // tab2 の手元は古いが、保存されている状態から続ける
  assert.equal(new Set([...a, ...b]).size, 6);
  assert.deepEqual([tab2.drawnCount, tab2.revision], [6, 2]);
  // 同時に呼んでも直列になり、重複しない
  const [c, d] = await Promise.all([tab1.draw(4), tab2.draw(4)]);
  assert.equal(new Set([...a, ...b, ...c, ...d]).size, 14);
  assert.equal((await lots.resume(tab1.id)).drawnCount, 14);
});

test("保存で競合したら、読み直してやり直し、結果は二重にならない", async () => {
  const inner = memoryStore();
  let failures = 2;
  const flaky = {
    ...inner,
    async put(id, record, options) {
      // 作成のあとの最初の 2 回の保存を、競合として失敗させる
      if (options.expectedRevision !== null && failures-- > 0) throw new ConflictError();
      return inner.put(id, record, options);
    },
  };
  const lots = createPersistence(s, { store: flaky });
  const lot = await lots.create({ n: 30, seed: 5n });
  const picks = await lot.draw(5);
  assert.deepEqual(picks, s.draw(30, 5, { seed: 5n })); // 失敗した試行が結果を変えない
  assert.deepEqual([lot.drawnCount, lot.revision], [5, 1]);
  assert.equal((await inner.get(lot.id)).revision, 1);

  failures = Infinity;
  await assert.rejects(lot.draw(1), ConflictError); // 試行の上限を超えたら諦める
  failures = 0;
  assert.equal((await lot.draw(1)).length, 1); // 失敗しても次の呼び出しで続けられる
  assert.equal(lot.drawnCount, 6);
});

test("list / remove / delete / 保存されていないセッション", async () => {
  const lots = createPersistence(s, { store: memoryStore() });
  const a = await lots.create({ n: 10, label: "A" });
  const b = await lots.createItems(["x", "y"], { label: "B" });
  await a.draw(1);
  const list = await lots.list();
  assert.deepEqual(list.map((l) => l.label).sort(), ["A", "B"]);
  assert.equal(list.find((l) => l.label === "A").revision, 1);
  assert.equal(list.find((l) => l.label === "B").kind, "items");
  await lots.remove(b.id);
  assert.equal(await lots.resume(b.id), null);
  await a.delete();
  await assert.rejects(lots.export(a.id), NotFoundError);
  assert.deepEqual(await lots.list(), []);
});

test("export / import: 別の保存先（別の端末）へ移して続きから引ける。不正な状態は拒否する", async () => {
  const here = createPersistence(s, { store: memoryStore() });
  const there = createPersistence(s, { store: memoryStore() });
  const lot = await here.createItems(names, { seed: 7n });
  await lot.draw(3);
  const text = await lot.export();
  const id = await there.import(text);
  assert.equal(id, lot.id);
  const moved = await there.resume(id);
  assert.deepEqual((await moved.draw(5)).map((p) => p.id), s.drawItems(names, 8, { seed: 7n }).slice(3).map((p) => p.id));

  // 手元より古い状態は、既定では上書きしない
  assert.equal(await there.import(text), id);
  assert.equal((await there.resume(id)).revision, 2);
  await there.import(text, { overwrite: true });
  assert.equal((await there.resume(id)).revision, 1);

  const record = JSON.parse(text);
  record.state = record.state.replace('"drawn":[', '"drawn":[0,');
  await assert.rejects(there.import(JSON.stringify(record)), SortitionError);
  await assert.rejects(there.import(JSON.stringify({ ...record, v: 99 })), RangeError);
});

test("localStorage の保存先でも同じように動く", async () => {
  const map = new Map();
  const storage = {
    getItem: (k) => (map.has(k) ? map.get(k) : null),
    setItem: (k, v) => void map.set(k, String(v)),
    removeItem: (k) => void map.delete(k),
    key: (i) => [...map.keys()][i] ?? null,
    get length() { return map.size; },
  };
  map.set("other:key", "x"); // 無関係なキーは触らない
  const lots = createPersistence(s, { store: localStorageStore({ storage }) });
  const lot = await lots.create({ n: 40, seed: 2n, label: "ls" });
  const first = await lot.draw(5);
  const again = await lots.resume(lot.id);
  const second = await again.draw(5);
  assert.deepEqual([...first, ...second], s.draw(40, 10, { seed: 2n }));
  assert.deepEqual((await lots.list()).map((l) => l.label), ["ls"]);
  await again.delete();
  assert.equal(map.size, 1);
});
