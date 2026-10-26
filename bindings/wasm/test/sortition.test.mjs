import { test } from "node:test";
import assert from "node:assert/strict";
import { load, seedFromText, normalizeCandidates, SortitionError } from "../sortition.js";

const s = await load();

test("version", () => assert.equal(s.version(), "0.1.0"));

test("draw: seed は再現可能で、ほかの言語バインディングと同じ結果になる", () => {
  assert.deepEqual(s.draw(10, 3, { seed: 1n }), [7, 2, 6]);
  assert.deepEqual(s.draw(10, 3, { seed: 1 }), [7, 2, 6]);
  assert.notDeepEqual(s.draw(10, 3, { seed: 2n }), [7, 2, 6]);
});

test("draw: 乱数源（seed なし）でも重複なしで全件が出る", () => {
  const all = s.draw(100, 100);
  assert.deepEqual([...all].sort((a, b) => a - b), Array.from({ length: 100 }, (_, i) => i));
  assert.notDeepEqual(s.draw(1000, 5), s.draw(1000, 5));
});

test("draw: 空と境界", () => {
  assert.deepEqual(s.draw(0, 0), []);
  assert.deepEqual(s.draw(5, 0, { seed: 1 }), []);
  assert.equal(s.draw(4294967295, 3).length, 3);
});

test("draw: エラー", () => {
  assert.throws(() => s.draw(3, 4), (e) => e instanceof SortitionError && e.code === -1);
  assert.throws(() => s.draw(-1, 0), RangeError);
  assert.throws(() => s.draw(3, 1.5), RangeError);
  assert.throws(() => s.draw(3, 1, { seed: "x" }), TypeError);
});

test("drawText: 日本語・空白・空行", () => {
  const t = "  alice \r\n\r\nbob\n日本語\ncarol\n";
  assert.deepEqual(s.drawText(t, 2, { seed: 1 }), ["bob", "日本語"]);
  assert.deepEqual([...s.drawText(t, 4)].sort(), ["alice", "bob", "carol", "日本語"].sort());
  assert.deepEqual(s.drawText("", 0), []);
  assert.throws(() => s.drawText(t, 5), (e) => e.code === -1);
});

test("drawData: yaml / toml / json", () => {
  const y = "- {name: alice, age: 30}\n- {name: bob, tags: [a, b]}\n- plain\n- no\n- ~\n";
  const r = s.drawData(y, 5, { format: "yaml", seed: 1 });
  assert.deepEqual(r, s.drawData(y, 5, { format: "yaml", seed: 1 }));
  assert.deepEqual(r.slice(0, 3), ["plain", "no", null]); // Python / Ruby と同じ並び
  const t = 'members = ["a","b","c"]\n[[team]]\nname = "x"\n[[team]]\nname = "y"\n';
  assert.equal(s.drawData(t, 3, { format: "toml", path: "members" }).length, 3);
  assert.deepEqual(
    s.drawData(t, 2, { format: "toml", path: "team" }).map((m) => m.name).sort(),
    ["x", "y"],
  );
  assert.equal(s.drawData('{"a":{"list":[{"id":1},{"id":2}]}}', 1, { format: "json", path: "a.list" }).length, 1);
});

test("drawData: エラーには詳細が付く", () => {
  assert.throws(
    () => s.drawData("a: &x [1]\nb: *x\n", 1, { format: "yaml", path: "a" }),
    (e) => e instanceof SortitionError && e.code === -4 && /aliases/.test(e.message),
  );
  assert.throws(() => s.drawData("[1]", 1, { format: "xml" }), RangeError);
  assert.throws(() => s.drawData("{}", 1, { format: "json", path: "a" }), (e) => e.code === -5);
});

test("drawStages: 賞ごとに切り分け、重複当選がない", () => {
  const names = Array.from({ length: 30 }, (_, i) => `user${i}`);
  const stages = [{ name: "副賞", count: 3 }, { name: "大賞", count: 1 }];
  const r = s.drawStages(names, stages, { seed: 42 });
  assert.deepEqual(r.map((x) => [x.name, x.winners.length]), [["副賞", 3], ["大賞", 1]]);
  const flat = r.flatMap((x) => x.winners);
  assert.equal(new Set(flat).size, 4);
  // 合計 4 人の重複なし抽選を先頭から切り分けたものと一致する
  assert.deepEqual(flat, s.draw(30, 4, { seed: 42 }).map((i) => names[i]));
  assert.deepEqual(r, s.drawStages(names, stages, { seed: 42 }));
});

test("drawStages: 配列の要素はそのまま、テキストと構造化データにも使える", () => {
  const objs = [{ id: 1 }, { id: 2 }, { id: 3 }];
  const r = s.drawStages(objs, [{ name: "a", count: 2 }, { name: "b", count: 1 }], { seed: 1 });
  assert.ok(r.flatMap((x) => x.winners).every((w) => objs.includes(w)));
  assert.equal(s.drawStages("a\nb\nc", [{ name: "x", count: 3 }])[0].winners.length, 3);
  const d = s.drawStages("- 1\n- 2\n- 3\n", [{ name: "x", count: 1 }, { name: "y", count: 1 }], { format: "yaml" });
  assert.equal(d.length, 2);
  assert.throws(() => s.drawStages(["a"], [{ name: "x", count: 2 }]), (e) => e.code === -1);
  assert.throws(() => s.drawStages(["a"], [{ name: "x", count: -1 }]), RangeError);
});

test("seedFromText: SHA-256 の先頭 8 バイト（リトルエンディアン）", async () => {
  // printf sortition | sha256sum の先頭 8 バイトをリトルエンディアンで読んだ値
  assert.equal(await seedFromText("sortition"), 131367258856992384n);
  assert.notEqual(await seedFromText("a"), await seedFromText("b"));
});

test("normalizeCandidates: コアと同じ規則", () => {
  assert.deepEqual(normalizeCandidates("  a \r\n\r\n b\n\t\nc\n"), ["a", "b", "c"]);
});

test("session: 分けて引いても一括と同じ並びで、保存・復元して続きから引ける", () => {
  const a = s.createSession(200, { seed: 9n });
  const got = a.draw(20);
  const saved = a.save();
  a.close();
  const b = s.loadSession(saved);
  got.push(...b.draw(30));
  assert.deepEqual(got, s.draw(200, 50, { seed: 9n }));
  assert.equal(b.remaining, 150);
  assert.equal(b.drawnCount, 50);
  assert.deepEqual(JSON.parse(b.save()).drawn, got);
  b.close();
});

test("session: 乱数源でも使い切ると重複なしで全件が出る", () => {
  const a = s.createSession(30);
  const all = [...a.draw(10), ...a.draw(20)];
  assert.deepEqual([...all].sort((x, y) => x - y), Array.from({ length: 30 }, (_, i) => i));
  assert.equal(a.remaining, 0);
  a.close();
});

test("session: エラー（超過・不正な状態・閉じた後）", () => {
  const a = s.createSession(5, { seed: 1 });
  a.draw(3);
  const before = a.save();
  assert.throws(() => a.draw(3), (e) => e instanceof SortitionError && e.code === -1);
  assert.equal(a.save(), before); // 失敗しても状態は変わらない
  assert.throws(() => s.loadSession("{}"), (e) => e instanceof SortitionError && e.code === -8);
  assert.throws(() => s.loadSession("not json"), (e) => e.code === -8);
  a.close();
  a.close(); // 二重 close は無害
  assert.throws(() => a.draw(1), /closed/);
  assert.throws(() => s.createSession(-1), RangeError);
});

const names = ["alice", "bob", "carol", "dave", "erin", "frank", "grace", "heidi"];
const ids = (picks) => picks.map((p) => p.id);

test("items: ID は SHA-256(接頭辞 || バイト列 || 出現番号)。重複しても一意で、バイナリも使える", () => {
  assert.deepEqual(s.itemIds(["abc"]), ["3b39139043226007aae3c88c03f4a1867e3cea04126ee893f8b395f7eb97899b"]);
  assert.deepEqual(s.itemIds([new TextEncoder().encode("abc")]), s.itemIds(["abc"]));
  const dup = s.itemIds(["x", "x", "", "", new Uint8Array([255, 0, 254]), "x"]);
  assert.equal(new Set(dup).size, 6);
  assert.throws(() => s.itemIds([1]), TypeError);
});

test("items: 並び順に依らず、同じ集合とシードなら同じ候補が当選する", () => {
  const shuffled = [...names.slice(3), ...names.slice(0, 3)].reverse();
  assert.equal(s.itemFingerprint(names), s.itemFingerprint(shuffled));
  for (let seed = 0; seed < 10; seed++) {
    const a = s.drawItems(names, 5, { seed });
    const b = s.drawItems(shuffled, 5, { seed });
    assert.deepEqual(ids(a), ids(b));
    assert.deepEqual(a.map((p) => names[p.index]), b.map((p) => shuffled[p.index]));
  }
  assert.throws(() => s.drawItems(names, 9), (e) => e instanceof SortitionError && e.code === -1);
  assert.deepEqual(s.drawItems([], 0), []);
});

test("itemSession: 分けて引いても一括と同じで、並びを変えても復元して続きから引ける", () => {
  const a = s.createItemSession(names, { seed: 11n });
  const got = a.draw(3);
  const state = a.save();
  a.close();
  const shuffled = [...names.slice(3), ...names.slice(0, 3)];
  const b = s.loadItemSession(state, shuffled);
  const rest = b.draw(5);
  got.push(...rest);
  assert.deepEqual(ids(got), ids(s.drawItems(names, 8, { seed: 11n })));
  assert.ok(rest.every((p) => s.itemIds(shuffled)[p.index] === p.id)); // index は渡し直した配列の位置
  const saved = JSON.parse(b.save());
  assert.deepEqual(saved.drawn, ids(got));
  assert.equal(saved.fingerprint, s.itemFingerprint(names));
  assert.equal(b.remaining, 0);
  assert.equal(b.drawnCount, 8);
  b.close();
});

test("itemSession: 候補が違う・超過・閉じた後はエラー", () => {
  const a = s.createItemSession(names, { seed: 1 });
  a.draw(2);
  const state = a.save();
  assert.throws(() => s.loadItemSession(state, names.slice(1)), (e) => e instanceof SortitionError && e.code === -8);
  assert.throws(() => s.loadItemSession(state, [...names.slice(1), "mallory"]), (e) => e.code === -8);
  assert.throws(() => a.draw(7), (e) => e.code === -1);
  assert.equal(a.save(), state); // 失敗しても状態は変わらない
  a.close();
  a.close();
  assert.throws(() => a.draw(1), /closed/);
});

test("メモリを解放している（呼び出しを繰り返しても増え続けない）", () => {
  const text = "a\nb\nc\nd";
  const yaml = "- {a: 1}\n- {b: 2}\n- c\n";
  const round = () => {
    s.draw(10, 3);
    s.drawText(text, 2);
    s.drawData(yaml, 2, { format: "yaml" });
    try { s.drawData("[1", 1, { format: "json" }); } catch {} // エラー経路でも解放される
    const lot = s.createSession(10, { seed: 1 });
    lot.draw(3);
    s.loadSession(lot.save()).close();
    lot.close();
    try { s.loadSession("{}"); } catch {}
    const items = s.createItemSession(["a", "b", "c", new Uint8Array([1, 2])], { seed: 1 });
    items.draw(2);
    s.loadItemSession(items.save(), ["c", "b", "a", new Uint8Array([1, 2])]).close();
    items.close();
    s.drawItems(["a", "b", "c"], 2);
    try { s.loadItemSession("{}", ["a"]); } catch {}
  };
  for (let i = 0; i < 200; i++) round(); // ウォームアップ
  const before = s.memoryBytes();
  for (let i = 0; i < 20000; i++) round();
  assert.ok(s.memoryBytes() - before <= 65536, `memory grew: ${before} -> ${s.memoryBytes()}`);
});
