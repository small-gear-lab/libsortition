'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const s = require('..');
const fs = require('node:fs');
const path = require('node:path');

// 全言語共通のテストベクトル（docs/SPEC.md、core のテストが作る）。64 ビットの値は 10 進文字列で入っている
const V = JSON.parse(fs.readFileSync(path.join(__dirname, '..', '..', '..', 'test-vectors', 'vectors.json'), 'utf8'));
const vectorItems = (v) => v.items_hex.map((h) => Buffer.from(h, 'hex'));
const vectorSets = () => Object.fromEntries(V.item_sets.map((v) => [v.name, vectorItems(v)]));

test('version', () => assert.equal(s.version(), '0.1.1'));

test('draw: seed は再現可能で、ほかの言語バインディングと同じ結果になる', () => {
  assert.deepEqual(s.draw(10, 3, { seed: 1n }), [7, 2, 6]);
  assert.deepEqual(s.draw(10, 3, { seed: 1 }), [7, 2, 6]);
  assert.notDeepEqual(s.draw(10, 3, { seed: 2n }), [7, 2, 6]);
});

test('draw: 負数の seed は 2 の補数（u64）として扱う', () => {
  assert.deepEqual(s.draw(10, 3, { seed: -1 }), s.draw(10, 3, { seed: 0xffffffffffffffffn }));
});

test('draw: 乱数源（seed なし）でも重複なしで全件が出る', () => {
  const all = s.draw(100, 100);
  assert.deepEqual([...all].sort((a, b) => a - b), Array.from({ length: 100 }, (_, i) => i));
  assert.notDeepEqual(s.draw(1000, 5), s.draw(1000, 5));
});

test('draw: 空と境界', () => {
  assert.deepEqual(s.draw(0, 0), []);
  assert.deepEqual(s.draw(5, 0, { seed: 1 }), []);
  assert.equal(s.draw(4294967295, 3).length, 3);
});

test('draw: エラー', () => {
  assert.throws(() => s.draw(3, 4), (e) => e instanceof s.SortitionError && e.code === -1);
  assert.throws(() => s.draw(-1, 0), RangeError);
  assert.throws(() => s.draw(3, 1.5), RangeError);
  assert.throws(() => s.draw(3, 1, { seed: 'x' }), TypeError);
  assert.throws(() => s.draw(3, 1, { seed: 1.5 }), TypeError);
});

test('drawText: 日本語・空白・空行', () => {
  const t = '  alice \r\n\r\nbob\n日本語\ncarol\n';
  assert.deepEqual(s.drawText(t, 2, { seed: 1 }), ['bob', '日本語']);
  assert.deepEqual([...s.drawText(t, 4)].sort(), ['alice', 'bob', 'carol', '日本語'].sort());
  assert.deepEqual(s.drawText('', 0), []);
  assert.throws(() => s.drawText(t, 5), (e) => e.code === -1);
});

test('drawData: yaml / toml / json', () => {
  const y = '- {name: alice, age: 30}\n- {name: bob, tags: [a, b]}\n- plain\n- no\n- ~\n';
  const r = s.drawData(y, 5, { format: 'yaml', seed: 1 });
  assert.deepEqual(r, s.drawData(y, 5, { format: 'yaml', seed: 1 }));
  assert.deepEqual(r.slice(0, 3), ['plain', 'no', null]); // Python / Ruby / WASM と同じ並び
  const t = 'members = ["a","b","c"]\n[[team]]\nname = "x"\n[[team]]\nname = "y"\n';
  assert.equal(s.drawData(t, 3, { format: 'toml', path: 'members' }).length, 3);
  assert.deepEqual(s.drawData(t, 2, { format: 'toml', path: 'team' }).map((m) => m.name).sort(), ['x', 'y']);
  assert.equal(s.drawData('{"a":{"list":[{"id":1},{"id":2}]}}', 1, { format: 'json', path: 'a.list' }).length, 1);
});

test('drawData: エラーには詳細が付く', () => {
  assert.throws(
    () => s.drawData('a: &x [1]\nb: *x\n', 1, { format: 'yaml', path: 'a' }),
    (e) => e instanceof s.SortitionError && e.code === -4 && /aliases/.test(e.message),
  );
  assert.throws(() => s.drawData('[1]', 1, { format: 'xml' }), RangeError);
  assert.throws(() => s.drawData('{}', 1, { format: 'json', path: 'a' }), (e) => e.code === -5);
  assert.throws(() => s.drawData('[1]', 1, { format: 'json', path: 'a\0b' }), RangeError);
});

test('drawStages: 賞ごとに切り分け、重複当選がなく、候補の ID で引く', () => {
  const people = Array.from({ length: 30 }, (_, i) => `user${i}`);
  const stages = [{ name: '副賞', count: 3 }, { name: '大賞', count: 1 }];
  const r = s.drawStages(people, stages, { seed: 42 });
  assert.deepEqual(r.map((x) => [x.name, x.count, x.winners.length]), [['副賞', 3, 3], ['大賞', 1, 1]]);
  const flat = r.flatMap((x) => x.winners);
  assert.equal(new Set(flat.map((p) => p.id)).size, 4);
  // 合計 4 人を drawItems で一度に引いて、先頭から切り分けたものと一致する
  assert.deepEqual(flat, s.drawItems(people, 4, { seed: 42 }));
  assert.ok(flat.every((p) => people[p.index] !== undefined));
  assert.deepEqual(r, s.drawStages(people, stages, { seed: 42 }));
  // 候補の並びを変えても、同じ候補が当選する
  const shuffled = [...people].reverse();
  const again = s.drawStages(shuffled, stages, { seed: 42 });
  assert.deepEqual(again.map((x) => x.winners.map((p) => p.id)), r.map((x) => x.winners.map((p) => p.id)));
  assert.deepEqual(again.flatMap((x) => x.winners.map((p) => shuffled[p.index])), r.flatMap((x) => x.winners.map((p) => people[p.index])));
});

test('drawStages: 候補は文字列・バイナリ。空の賞や賞なし、エラー', () => {
  const items = ['a', 'b', new Uint8Array([0, 255]), 'd'];
  assert.equal(s.drawStages(items, [{ name: 'x', count: 4 }])[0].winners.length, 4);
  assert.deepEqual(s.drawStages(items, []), []);
  assert.deepEqual(s.drawStages(items, [{ name: '空', count: 0 }])[0].winners, []);
  assert.throws(() => s.drawStages(['a'], [{ name: 'x', count: 2 }]), (e) => e instanceof s.SortitionError && e.code === -1);
  assert.throws(() => s.drawStages(['a'], [{ name: 'x', count: -1 }]), RangeError);
  assert.throws(() => s.drawStages(['a'], [{ name: 'x', count: 1.5 }]), RangeError);
  assert.throws(() => s.drawStages(['a'], [{ name: 1, count: 1 }]), TypeError);
  assert.throws(() => s.drawStages([1], [{ name: 'x', count: 1 }]), TypeError);
});

test('seedFromText: SHA-256 の先頭 8 バイト（リトルエンディアン）', () => {
  // printf sortition | sha256sum の先頭 8 バイトをリトルエンディアンで読んだ値
  assert.equal(s.seedFromText('sortition'), 131367258856992384n);
  assert.notEqual(s.seedFromText('a'), s.seedFromText('b'));
});

test('normalizeCandidates: コアと同じ規則', () => {
  assert.deepEqual(s.normalizeCandidates('  a \r\n\r\n b\n\t\nc\n'), ['a', 'b', 'c']);
});

// 解放漏れの検出: 数百 KB の入力で繰り返し呼び、RSS が増えないことを見る。
// 入力が大きいので、バッファや文字列を 1 回でも解放し忘れると 1 呼び出しごとに数百 KB 増えて確実に見える。
// （小さな入力で何十万回も呼ぶ方法は、例外を投げ続けたときの V8 のメモリ増加に埋もれて検出できない）
const MiB = 1024 * 1024;
function rssGrowth(fn, { warmup, measure }) {
  for (let i = 0; i < warmup; i++) fn();
  global.gc && global.gc();
  const before = process.memoryUsage().rss;
  for (let i = 0; i < measure; i++) fn();
  global.gc && global.gc();
  return process.memoryUsage().rss - before;
}

test('大きな入力の正常系を繰り返しても RSS が増えない（解放漏れの確認）', () => {
  const lines = Array.from({ length: 20000 }, (_, i) => `candidate-${i}`).join('\n'); // 約 300 KB
  const json = '[' + Array.from({ length: 20000 }, (_, i) => `{"id":${i}}`).join(',') + ']'; // 約 240 KB
  const yaml = Array.from({ length: 5000 }, (_, i) => `- {id: ${i}}`).join('\n'); // 約 65 KB（YAML の解析は遅い）
  const grew = rssGrowth(() => {
    s.drawText(lines, 5);
    s.drawData(json, 5, { format: 'json' });
    s.drawData(json, 15000, { format: 'json' }); // 結果も大きい（sortition_free の解放漏れを見る）
    s.drawData(yaml, 5, { format: 'yaml' });
    s.draw(1000000, 1000);
  }, { warmup: 20, measure: 150 });
  assert.ok(grew < 15 * MiB, `RSS grew by ${(grew / MiB).toFixed(1)} MiB`);
});

test('大きな入力のエラー経路を繰り返しても RSS が増えない（解放漏れの確認）', () => {
  const badJson = '[' + '1,'.repeat(150000); // 閉じていない（約 300 KB）
  const alias = 'a: &x [1]\n' + 'b: *x\n'.repeat(5000); // エイリアスで拒否される
  const longPath = 'p'.repeat(100000);
  const grew = rssGrowth(() => {
    try { s.drawData(badJson, 1, { format: 'json', path: longPath }); } catch {}
    try { s.drawData(alias, 1, { format: 'yaml', path: 'a' }); } catch {}
    try { s.drawText('x\n'.repeat(100000), 200000); } catch {}
  }, { warmup: 20, measure: 100 });
  assert.ok(grew < 15 * MiB, `RSS grew by ${(grew / MiB).toFixed(1)} MiB`);
});

test('session: 分けて引いても一括と同じ並びで、保存・復元して続きから引ける', () => {
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

test('session: 乱数源でも使い切ると重複なしで全件が出る', () => {
  const a = s.createSession(30);
  const all = [...a.draw(10), ...a.draw(20)];
  assert.deepEqual([...all].sort((x, y) => x - y), Array.from({ length: 30 }, (_, i) => i));
  assert.equal(a.remaining, 0);
  a.close();
});

test('session: エラー（超過・不正な状態・閉じた後）', () => {
  const a = s.createSession(5, { seed: 1 });
  a.draw(3);
  const before = a.save();
  assert.throws(() => a.draw(3), (e) => e instanceof s.SortitionError && e.code === -1);
  assert.equal(a.save(), before); // 失敗しても状態は変わらない
  assert.throws(() => s.loadSession('{}'), (e) => e instanceof s.SortitionError && e.code === -8);
  assert.throws(() => s.loadSession('not json'), (e) => e.code === -8);
  a.close();
  a.close(); // 二重 close は無害
  assert.throws(() => a.draw(1), /closed/);
  assert.throws(() => a.remaining, /closed/);
  assert.throws(() => s.createSession(-1), RangeError);
});

const names = ['alice', 'bob', 'carol', 'dave', 'erin', 'frank', 'grace', 'heidi'];
const idsOf = (picks) => picks.map((p) => p.id);

test('items: ID は SHA-256(接頭辞 || バイト列 || 出現番号)。重複しても一意で、バイナリも使える', () => {
  assert.deepEqual(s.itemIds(['abc']), ['3b39139043226007aae3c88c03f4a1867e3cea04126ee893f8b395f7eb97899b']);
  assert.deepEqual(s.itemIds([Buffer.from('abc')]), s.itemIds(['abc']));
  const xabc = Buffer.from('xabc');
  assert.deepEqual(s.itemIds([new Uint8Array(xabc.buffer, xabc.byteOffset + 1, 3)]), s.itemIds(['abc'])); // オフセット付き
  const dup = s.itemIds(['x', 'x', '', '', new Uint8Array([255, 0, 254]), 'x']);
  assert.equal(new Set(dup).size, 6);
  assert.throws(() => s.itemIds([1]), TypeError);
  assert.deepEqual(s.itemIds([]), []);
});

test('items: 並び順に依らず、同じ集合とシードなら同じ候補が当選する', () => {
  const shuffled = [...names.slice(3), ...names.slice(0, 3)].reverse();
  assert.equal(s.itemFingerprint(names), s.itemFingerprint(shuffled));
  for (let seed = 0; seed < 10; seed++) {
    const a = s.drawItems(names, 5, { seed });
    const b = s.drawItems(shuffled, 5, { seed });
    assert.deepEqual(idsOf(a), idsOf(b));
    assert.deepEqual(a.map((p) => names[p.index]), b.map((p) => shuffled[p.index]));
  }
  assert.throws(() => s.drawItems(names, 9), (e) => e instanceof s.SortitionError && e.code === -1);
  assert.deepEqual(s.drawItems([], 0), []);
});

test('itemSession: 分けて引いても一括と同じで、並びを変えても復元して続きから引ける', () => {
  const a = s.createItemSession(names, { seed: 11n });
  const got = a.draw(3);
  const state = a.save();
  a.close();
  const shuffled = [...names.slice(3), ...names.slice(0, 3)];
  const b = s.loadItemSession(state, shuffled);
  const rest = b.draw(5);
  got.push(...rest);
  assert.deepEqual(idsOf(got), idsOf(s.drawItems(names, 8, { seed: 11n })));
  assert.ok(rest.every((p) => s.itemIds(shuffled)[p.index] === p.id)); // index は渡し直した配列の位置
  const saved = JSON.parse(b.save());
  assert.deepEqual(saved.drawn, idsOf(got));
  assert.equal(saved.fingerprint, s.itemFingerprint(names));
  assert.equal(b.remaining, 0);
  assert.equal(b.drawnCount, 8);
  b.close();
});

test('itemSession: 候補が違う・超過・閉じた後・種類違いはエラー', () => {
  const a = s.createItemSession(names, { seed: 1 });
  a.draw(2);
  const state = a.save();
  assert.throws(() => s.loadItemSession(state, names.slice(1)), (e) => e instanceof s.SortitionError && e.code === -8);
  assert.throws(() => s.loadItemSession(state, [...names.slice(1), 'mallory']), (e) => e.code === -8);
  assert.throws(() => s.loadSession(state), (e) => e.code === -8); // 数値セッションとして読めない
  assert.throws(() => a.draw(7), (e) => e.code === -1);
  assert.equal(a.save(), state); // 失敗しても状態は変わらない
  a.close();
  a.close();
  assert.throws(() => a.draw(1), /closed/);
});

test('session: 羃等キー（再送は前回と同じ結果、別の k は衝突）と revision / id', () => {
  const a = s.createSession(50);
  const first = a.draw(3, { key: 'req-1' });
  const state = a.save();
  assert.deepEqual(a.draw(3, { key: 'req-1' }), first);
  assert.equal(a.save(), state);
  assert.equal(a.revision, 1);
  assert.throws(() => a.draw(4, { key: 'req-1' }), (e) => e instanceof s.SortitionError && e.code === -9);
  assert.throws(() => a.draw(1, { key: '' }), (e) => e.code === -1);
  assert.throws(() => a.draw(1, { key: 5 }), TypeError);
  assert.throws(() => a.draw(1, { key: 'a\0b' }), RangeError);
  assert.match(a.id, /^[0-9a-f]{32}$/);
  assert.equal(JSON.parse(state).id, a.id);
  const b = s.createSession(5);
  const all = b.draw(5, { key: 'all' });
  assert.deepEqual(b.draw(5, { key: 'all' }), all); // 全部引いたあとでも再送は前回の結果
  const c = s.createSession(100);
  c.draw(4, { key: 'a' });
  const stale = c.save();
  const want = c.draw(6, { key: 'b' });
  const old = s.loadSession(stale);
  assert.deepEqual(old.draw(6, { key: 'b' }), want); // 古い状態への再送は同じ結果・同じ状態
  assert.equal(old.save(), c.save());
});

test('seal: 封をして預け、鍵がなければ開けず、改ざんも検知する', () => {
  const key = Buffer.alloc(32, 7);
  const a = s.createSession(50);
  a.draw(3, { key: 'r' });
  const token = a.seal(key, 9);
  assert.ok(token.startsWith('sortition1.'));
  assert.equal(s.sealKeyId(token), 9);
  assert.ok(!token.includes(JSON.parse(a.save()).rng.key));
  const b = s.unsealSession(token, key);
  assert.equal(b.save(), a.save());
  assert.deepEqual(b.draw(2), a.draw(2));
  const tampered = token.slice(0, -1) + (token.endsWith('A') ? 'B' : 'A');
  for (const f of [
    () => s.unsealSession(token, Buffer.alloc(32, 8)),
    () => s.unsealSession(tampered, key),
    () => s.unsealSession('junk', key),
    () => s.sealKeyId('junk'),
  ]) assert.throws(f, (e) => e instanceof s.SortitionError && e.code === -8);
  assert.throws(() => a.seal(Buffer.alloc(31)), RangeError);
  assert.throws(() => a.seal(key, 256), RangeError);
  assert.throws(() => a.seal('x'.repeat(32)), RangeError);
});

test('itemSession: 羃等キー・revision・id・seal', () => {
  const key = Buffer.alloc(32, 1);
  const lot = s.createItemSession(names, { seed: 11n });
  const got = lot.draw(3, { key: 'k1' });
  assert.deepEqual(lot.draw(3, { key: 'k1' }), got);
  assert.equal(lot.revision, 1);
  assert.match(lot.id, /^[0-9a-f]{32}$/);
  const token = lot.seal(key, 1);
  const shuffled = [...names.slice(2), ...names.slice(0, 2)];
  const again = s.unsealItemSession(token, key, shuffled);
  assert.deepEqual(idsOf(again.draw(5)), idsOf(lot.draw(5)));
  assert.throws(() => s.unsealItemSession(token, key, names.slice(1)), (e) => e.code === -8);
  assert.throws(() => s.unsealSession(token, key), (e) => e.code === -8); // 数値のセッションとしては開けない
});

test('共通ベクトル: seed と draw', () => {
  for (const v of V.seed_from_text) assert.equal(s.seedFromText(v.text), BigInt(v.seed), `seed of ${JSON.stringify(v.text)}`);
  for (const v of V.draw) assert.deepEqual(s.draw(v.n, v.k, { seed: BigInt(v.seed) }), v.result, `draw ${v.n} ${v.k} ${v.seed}`);
});

test('共通ベクトル: 候補の ID・指紋・抽選・賞', () => {
  const sets = vectorSets();
  for (const v of V.item_sets) {
    assert.deepEqual(s.itemIds(vectorItems(v)), v.ids, `ids of ${v.name}`);
    assert.equal(s.itemFingerprint(vectorItems(v)), v.fingerprint);
  }
  for (const v of V.draw_items) {
    assert.deepEqual(s.drawItems(sets[v.set], v.k, { seed: BigInt(v.seed) }), v.winners, `draw_items ${v.set} ${v.k} ${v.seed}`);
  }
  for (const v of V.stages) {
    const plan = v.stages.map(({ name, count }) => ({ name, count }));
    assert.deepEqual(s.drawStages(sets[v.set], plan, { seed: s.seedFromText(v.seed_text) }), v.stages, `stages of ${v.set}`);
  }
});

test('共通ベクトル: ChaCha20 の状態のセッションと、封をしたトークン', () => {
  let v = V.chacha20_session;
  const session = s.loadSession(JSON.stringify(v.state));
  assert.deepEqual(v.draws.map((k) => session.draw(k)), v.expected);
  v = V.seal;
  assert.equal(s.sealKeyId(v.token), v.key_id);
  const sealed = s.unsealSession(v.token, Buffer.from(v.key, 'hex'));
  assert.deepEqual(v.draws.map((k) => sealed.draw(k)), v.expected);
});
