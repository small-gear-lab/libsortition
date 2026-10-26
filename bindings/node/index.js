'use strict';
// libsortition の Node.js バインディング（N-API ネイティブアドオン）。
//
//   const s = require("libsortition");
//   s.draw(10, 3, { seed: 1n });                 // [7, 2, 6]
//   s.drawText("alice\nbob\ncarol", 2);          // ["bob", "alice"]
//   s.drawData("- {id: 1}\n- {id: 2}", 1, { format: "yaml" });
//   const lot = s.createSession(100, { seed: 1n }); // 続きから引ける抽選セッション
//   lot.draw(3); const saved = lot.save();         // 保存して…
//   s.loadSession(saved).draw(2);                  // …あとで続きから引く
//   s.drawItems(["alice", Buffer.from([1, 2])], 1, { seed: 1n }); // [{ index, id }]（任意のバイト列）
//   s.drawStages(["a", "b", "c", "d"], [{ name: "副賞", count: 3 }, { name: "大賞", count: 1 }]);
const crypto = require('node:crypto');
const native = require('./build/Release/sortition_node.node');

const FORMATS = { json: 0, yaml: 1, toml: 2 };

/** コアが返したエラー。code に SORTITION_ERR_* の値が入る。 */
class SortitionError extends RangeError {
  constructor(code, detail) {
    super(detail);
    this.name = 'SortitionError';
    this.code = code;
  }
}

function u32(value, name) {
  if (!Number.isInteger(value) || value < 0 || value > 0xffffffff) {
    throw new RangeError(`${name} must be an integer in 0..4294967295`);
  }
  return value;
}

function checkSeed(seed) {
  if (seed === undefined || seed === null) return seed;
  if (typeof seed === 'bigint' || (typeof seed === 'number' && Number.isSafeInteger(seed))) return seed;
  throw new TypeError('seed must be a bigint, a safe integer, null or undefined');
}

/** 羃等キー（string、1〜128 バイト）。undefined / null ならキーなし */
function idemKey(key) {
  if (key === undefined || key === null) return undefined;
  if (typeof key !== 'string') throw new TypeError('key must be a string, null or undefined');
  if (key.includes('\0')) throw new RangeError('key must not contain NUL');
  return key;
}

/** 封をする鍵（ちょうど 32 バイトの Buffer / Uint8Array / ArrayBuffer）を Uint8Array にする */
function secretKey(key) {
  let bytes = null;
  if (ArrayBuffer.isView(key)) bytes = new Uint8Array(key.buffer, key.byteOffset, key.byteLength);
  else if (key instanceof ArrayBuffer) bytes = new Uint8Array(key);
  if (!bytes || bytes.length !== 32) throw new RangeError('key must be exactly 32 bytes');
  return bytes;
}

function keyIdArg(keyId) {
  if (!Number.isInteger(keyId) || keyId < 0 || keyId > 255) throw new RangeError('keyId must be an integer in 0..255');
  return keyId;
}

/** ネイティブ層の例外（sortitionCode 付き）を SortitionError に変える */
function call(fn) {
  try {
    return fn();
  } catch (e) {
    if (e && typeof e.sortitionCode === 'number') throw new SortitionError(e.sortitionCode, e.message);
    throw e;
  }
}

function draw(n, k, { seed } = {}) {
  u32(n, 'n');
  u32(k, 'k');
  return call(() => native.draw(n, k, checkSeed(seed)));
}

function drawText(text, k, { seed } = {}) {
  u32(k, 'k');
  const joined = call(() => native.drawText(String(text), k, checkSeed(seed)));
  return joined === '' ? [] : joined.split('\n');
}

function drawData(text, k, { format, path = '', seed } = {}) {
  u32(k, 'k');
  if (!Object.hasOwn(FORMATS, format)) throw new RangeError(`format must be one of ${Object.keys(FORMATS).join(', ')}`);
  if (path.includes('\0')) throw new RangeError('path must not contain NUL');
  return JSON.parse(call(() => native.drawData(FORMATS[format], String(text), path, k, checkSeed(seed))));
}

/**
 * 抽選セッション。0..n-1 から呼ぶたびに続きから非復元抽出する。
 * save() で状態を JSON 文字列にでき、loadSession() で復元して続きから引ける。
 * seed 指定ありなら、何回に分けて引いても draw(n, k1 + k2 + ..., { seed }) と同じ並びになる。
 * close() し忘れても GC で解放される。
 */
class Session {
  #handle;

  constructor(handle) {
    this.#handle = handle;
  }

  /**
   * 続きから k 個引く。残りを超えると SortitionError（状態は変わらない）。
   * key（羃等キー、string、1〜128 バイト）を付けると、同じ key と同じ k の再送は、状態を変えずに
   * 前回と同じ結果を返す。同じ key で別の k は SortitionError（code -9）。
   */
  draw(k, { key } = {}) {
    u32(k, 'k');
    return call(() => native.sessionDraw(this.#handle, k, idemKey(key)));
  }

  /** 状態を変えた draw の回数。保存側が「読んだ revision のときだけ更新する」ために使える */
  get revision() {
    return native.sessionRevision(this.#handle);
  }

  /** セッションの識別子（小文字 16 進 32 文字）。作成時に OS から取った値 */
  get id() {
    return native.sessionId(this.#handle);
  }

  /**
   * 状態に封をしたトークン（string）を返す。乱数の内部状態を相手に見せずに預けられる。
   * key は 32 バイトの秘密鍵、keyId は鍵の入れ替え用の目印（0〜255）。
   */
  seal(key, keyId = 0) {
    return call(() => native.sessionSeal(this.#handle, secretKey(key), keyIdArg(keyId)));
  }

  /** まだ引いていない数 */
  get remaining() {
    return native.sessionRemaining(this.#handle);
  }

  /** ここまでに引いた数 */
  get drawnCount() {
    return native.sessionDrawnCount(this.#handle);
  }

  /** 状態を JSON 文字列で返す。引いた結果（"drawn"）も含む */
  save() {
    return call(() => native.sessionSave(this.#handle));
  }

  /** コア側のセッションを解放する（以後の利用は Error）。二重に呼んでもよい */
  close() {
    native.sessionClose(this.#handle);
  }
}

/** 0..n-1 を母集団とする抽選セッションを作る。 */
function createSession(n, { seed } = {}) {
  u32(n, 'n');
  return new Session(call(() => native.sessionNew(n, checkSeed(seed))));
}

/** Session#save() の出力から復元する。不正な状態は SortitionError（code -8）。 */
function loadSession(state) {
  return new Session(call(() => native.sessionLoad(String(state))));
}

// ---- バイト列の候補 ----
// 各候補は ID（SHA-256、小文字 16 進 64 文字）に変換され、ID の昇順に並べてから引かれる。
// 同じバイト列が複数あっても出現番号が混ざるので ID は一意。

/** 候補（string は UTF-8、Buffer / TypedArray / ArrayBuffer はそのまま）を連結バッファと長さ配列にする */
function packItems(items) {
  const chunks = Array.from(items, (item) => {
    let chunk;
    if (typeof item === 'string') chunk = Buffer.from(item, 'utf8');
    else if (item instanceof ArrayBuffer) chunk = Buffer.from(item);
    else if (ArrayBuffer.isView(item)) chunk = Buffer.from(item.buffer, item.byteOffset, item.byteLength);
    else throw new TypeError('candidate must be a string, Buffer, ArrayBuffer or a typed array');
    if (chunk.length > 0xffffffff) throw new RangeError('candidate is too large');
    return chunk;
  });
  return [Buffer.concat(chunks), Uint32Array.from(chunks, (c) => c.length)];
}

const hexIds = (buf, count) => Array.from({ length: count }, (_, i) => buf.toString('hex', 32 * i, 32 * i + 32));
const picks = ({ indexes, ids }) => indexes.map((index, i) => ({ index, id: ids.toString('hex', 32 * i, 32 * i + 32) }));

/** 各候補の ID（小文字 16 進 64 文字）を items と同じ並びで返す。 */
function itemIds(items) {
  const [data, lens] = packItems(items);
  return hexIds(call(() => native.itemIds(data, lens)), lens.length);
}

/** 候補集合の指紋（小文字 16 進 64 文字）。items の並びに依らない。 */
function itemFingerprint(items) {
  const [data, lens] = packItems(items);
  return call(() => native.itemFingerprint(data, lens)).toString('hex');
}

/** items から k 個を重複なしで抽選する。同じ候補集合とシードなら、items の並びが違っても同じ候補が当選する。 */
function drawItems(items, k, { seed } = {}) {
  u32(k, 'k');
  const [data, lens] = packItems(items);
  return picks(call(() => native.itemsDraw(data, lens, k, checkSeed(seed))));
}

/**
 * 候補集合を固定した抽選セッション（保存・復元つき）。
 * save() の状態には引いた候補の ID と候補集合の指紋が入る。loadItemSession() では同じ候補集合
 * （並びは違ってよい）を渡し直し、違えば SortitionError（code -8）。close() し忘れても GC で解放される。
 */
class ItemSession {
  #handle;

  constructor(handle) {
    this.#handle = handle;
  }

  /** 続きから k 個引く。残りを超えると SortitionError（状態は変わらない）。key（羃等キー）は Session#draw と同じ */
  draw(k, { key } = {}) {
    u32(k, 'k');
    return picks(call(() => native.itemSessionDraw(this.#handle, k, idemKey(key))));
  }

  /** 状態を変えた draw の回数 */
  get revision() {
    return native.itemSessionRevision(this.#handle);
  }

  /** セッションの識別子（小文字 16 進 32 文字） */
  get id() {
    return native.itemSessionId(this.#handle);
  }

  /** 状態に封をしたトークンを返す。引数と注意は Session#seal と同じ */
  seal(key, keyId = 0) {
    return call(() => native.itemSessionSeal(this.#handle, secretKey(key), keyIdArg(keyId)));
  }

  get remaining() {
    return native.itemSessionRemaining(this.#handle);
  }

  get drawnCount() {
    return native.itemSessionDrawnCount(this.#handle);
  }

  /** 状態を JSON 文字列で返す。引いた候補の ID（"drawn"）と候補集合の指紋（"fingerprint"）を含む */
  save() {
    return call(() => native.itemSessionSave(this.#handle));
  }

  /** コア側のセッションを解放する（以後の利用は Error）。二重に呼んでもよい */
  close() {
    native.sessionClose(this.#handle);
  }
}

/** 候補集合を固定した抽選セッションを作る。 */
function createItemSession(items, { seed } = {}) {
  const [data, lens] = packItems(items);
  return new ItemSession(call(() => native.itemSessionNew(data, lens, checkSeed(seed))));
}

/** ItemSession#save() の出力と、同じ候補集合（並びは違ってよい）から復元する。 */
function loadItemSession(state, items) {
  const [data, lens] = packItems(items);
  return new ItemSession(call(() => native.itemSessionLoad(String(state), data, lens)));
}

/** Session#seal のトークンから復元する。形式の不正・鍵違い・改ざんは SortitionError（code -8）。 */
function unsealSession(token, key) {
  return new Session(call(() => native.sessionUnseal(String(token), secretKey(key))));
}

/** ItemSession#seal のトークンと、同じ候補集合（並びは違ってよい）から復元する。 */
function unsealItemSession(token, key, items) {
  const [data, lens] = packItems(items);
  return new ItemSession(call(() => native.itemSessionUnseal(String(token), secretKey(key), data, lens)));
}

/** トークンに記された鍵 ID（0〜255）。鍵を入れ替えている場合に、開く前に鍵を選ぶのに使う。 */
function sealKeyId(token) {
  return call(() => native.sealKeyId(String(token)));
}

/**
 * 賞ごとに順番に抽選する（重複当選なし）。
 * 合計人数を重複なしで引いて、先頭から賞ごとに切り分ける。これは「1 つ目の賞を引き、
 * 残りから次の賞を引く」と同じ確率になる。
 *
 * input は配列（要素はそのまま返る）/ 1 行 1 候補のテキスト / format 指定時は構造化データのテキスト。
 * 戻り値は [{ name, count, winners }]。
 */
function drawStages(input, stages, { seed, format, path } = {}) {
  const counts = stages.map((s, i) => {
    if (!Number.isInteger(s.count) || s.count < 0) throw new RangeError(`stages[${i}].count must be a non-negative integer`);
    return s.count;
  });
  const total = counts.reduce((a, b) => a + b, 0);
  let picked;
  if (Array.isArray(input)) picked = draw(input.length, total, { seed }).map((i) => input[i]);
  else if (format !== undefined) picked = drawData(input, total, { format, path, seed });
  else picked = drawText(input, total, { seed });
  let offset = 0;
  return stages.map((s, i) => {
    const winners = picked.slice(offset, offset + counts[i]);
    offset += counts[i];
    return { name: s.name, count: counts[i], winners };
  });
}

/**
 * 文字列から seed（u64 の BigInt）を決める: SHA-256 の先頭 8 バイト（リトルエンディアン）。
 * 事前に公開した文字列と、当日に決まる値を連結して使うと、後から検証できる。
 */
function seedFromText(text) {
  return crypto.createHash('sha256').update(text, 'utf8').digest().readBigUInt64LE(0);
}

/** 抽選で扱う候補テキストの正規化（コアと同じ規則: 前後の空白を除き、空行を捨てる）。 */
function normalizeCandidates(text) {
  return text.split(/\r\n|\n|\r/).map((l) => l.trim()).filter((l) => l !== '');
}

module.exports = {
  draw, drawText, drawData, drawStages, createSession, loadSession,
  itemIds, itemFingerprint, drawItems, createItemSession, loadItemSession,
  unsealSession, unsealItemSession, sealKeyId, seedFromText, normalizeCandidates,
  version: () => native.version(),
  SortitionError,
};
