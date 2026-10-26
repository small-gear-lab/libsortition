'use strict';
// libsortition の Node.js バインディング（N-API ネイティブアドオン）。
//
//   const s = require("libsortition");
//   s.draw(10, 3, { seed: 1n });                 // [7, 2, 6]
//   s.drawText("alice\nbob\ncarol", 2);          // ["bob", "alice"]
//   s.drawData("- {id: 1}\n- {id: 2}", 1, { format: "yaml" });
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
  draw, drawText, drawData, drawStages, seedFromText, normalizeCandidates,
  version: () => native.version(),
  SortitionError,
};
