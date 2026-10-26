// libsortition の WASM バインディング（ブラウザ / Node 共通、依存なし）。
//
//   import { load } from "./sortition.js";
//   const s = await load();                       // 同じ場所の sortition.wasm を読む
//   s.draw(10, 3, { seed: 1n });                  // [7, 2, 6]
//   s.drawText("alice\nbob\ncarol", 2);           // ["bob", "alice"]
//   s.drawData("- {id: 1}\n- {id: 2}", 1, { format: "yaml" });
//   s.drawStages(["a", "b", "c", "d"], [{ name: "副賞", count: 3 }, { name: "大賞", count: 1 }]);

const FORMATS = { json: 0, yaml: 1, toml: 2 };

const MESSAGES = {
  [-1]: "invalid argument (k > n?)",
  [-2]: "OS random source failed",
  [-3]: "text is not valid UTF-8",
  [-4]: "parse error",
  [-5]: "path not found or not an array",
  [-6]: "unsupported format",
  [-7]: "internal error",
};

/** コアが返したエラー。code に SORTITION_ERR_* の値が入る。 */
export class SortitionError extends RangeError {
  constructor(code, detail) {
    super(detail || MESSAGES[code] || `error ${code}`);
    this.name = "SortitionError";
    this.code = code;
  }
}

const encoder = new TextEncoder();
const decoder = new TextDecoder("utf-8");

function u32(value, name) {
  if (!Number.isInteger(value) || value < 0 || value > 0xffffffff) {
    throw new RangeError(`${name} must be an integer in 0..4294967295`);
  }
  return value;
}

/** seed を u64 の BigInt にする。number は安全な整数のみ、負数は 2 の補数で扱う。 */
function seedToBigInt(seed) {
  if (typeof seed === "bigint") return BigInt.asUintN(64, seed);
  if (typeof seed === "number" && Number.isSafeInteger(seed)) return BigInt.asUintN(64, BigInt(seed));
  throw new TypeError("seed must be a bigint, a safe integer, null or undefined");
}

/**
 * 文字列から seed（u64 の BigInt）を決める: SHA-256 の先頭 8 バイト（リトルエンディアン）。
 * 事前に公開した文字列と、当日に決まる値を連結して使うと、後から検証できる。
 */
export async function seedFromText(text) {
  const digest = new Uint8Array(await globalThis.crypto.subtle.digest("SHA-256", encoder.encode(text)));
  return new DataView(digest.buffer).getBigUint64(0, true);
}

/** 抽選で扱う候補テキストの正規化（コアと同じ規則: 前後の空白を除き、空行を捨てる）。 */
export function normalizeCandidates(text) {
  return text.split(/\r\n|\n|\r/).map((l) => l.trim()).filter((l) => l !== "");
}

async function toWasmSource(source) {
  if (source === undefined) source = new URL("./sortition.wasm", import.meta.url);
  if (source instanceof WebAssembly.Module) return source;
  if (source instanceof ArrayBuffer || ArrayBuffer.isView(source)) return source;
  if (typeof Response !== "undefined" && source instanceof Response) return await source.arrayBuffer();
  const url = source instanceof URL ? source : new URL(String(source), import.meta.url);
  if (url.protocol === "file:") {
    const { readFile } = await import("node:fs/promises");
    return await readFile(url);
  }
  return await (await fetch(url)).arrayBuffer();
}

/**
 * WASM を読み込んで抽選 API を返す。
 * @param source ArrayBuffer / TypedArray / WebAssembly.Module / Response / URL / 文字列。省略時は同じ場所の sortition.wasm。
 */
export async function load(source) {
  const wasm = await toWasmSource(source);
  let e; // exports（import の中から参照するので後で代入）
  const imports = {
    env: {
      // wasm32-unknown-unknown には OS の乱数源がないので、ホストの CSPRNG を渡す
      sortition_random_fill(ptr, len) {
        const buf = new Uint8Array(e.memory.buffer, ptr, len);
        for (let off = 0; off < len; off += 65536) {
          globalThis.crypto.getRandomValues(buf.subarray(off, Math.min(off + 65536, len)));
        }
      },
    },
  };
  const instantiated = await WebAssembly.instantiate(wasm, imports);
  e = (instantiated.instance ?? instantiated).exports;

  const mem = () => new Uint8Array(e.memory.buffer);
  const view = () => new DataView(e.memory.buffer);

  /** リニアメモリに確保したバッファ。try/finally で必ず free する。 */
  const allocs = [];
  const alloc = (len) => {
    const ptr = e.sortition_alloc(len);
    allocs.push([ptr, len]);
    return ptr;
  };
  const freeAll = () => {
    for (const [ptr, len] of allocs.splice(0)) e.sortition_dealloc(ptr, len);
  };
  const putBytes = (bytes, extra = 0) => {
    const ptr = alloc(bytes.length + extra);
    mem().set(bytes, ptr);
    if (extra) mem().fill(0, ptr + bytes.length, ptr + bytes.length + extra);
    return ptr;
  };
  const putSeed = (seed) => {
    if (seed === undefined || seed === null) return 0;
    const ptr = alloc(8);
    view().setBigUint64(ptr, seedToBigInt(seed), true);
    return ptr;
  };
  const cString = (ptr) => {
    const bytes = mem();
    let end = ptr;
    while (bytes[end] !== 0) end++;
    return decoder.decode(bytes.subarray(ptr, end));
  };
  const run = (fn) => {
    try {
      return fn();
    } finally {
      freeAll();
    }
  };

  function draw(n, k, { seed } = {}) {
    u32(n, "n");
    u32(k, "k");
    return run(() => {
      const seedPtr = putSeed(seed);
      const out = alloc(4 * k);
      const rc = e.sortition_draw(n, k, seedPtr, out);
      if (rc !== 0) throw new SortitionError(rc);
      const dv = view();
      return Array.from({ length: k }, (_, i) => dv.getUint32(out + 4 * i, true));
    });
  }

  function drawText(text, k, { seed } = {}) {
    u32(k, "k");
    const data = encoder.encode(text);
    return run(() => {
      const textPtr = putBytes(data);
      const seedPtr = putSeed(seed);
      const out = alloc(data.length + 1);
      const n = Number(e.sortition_draw_text(textPtr, data.length, k, seedPtr, out, data.length + 1));
      if (n < 0) throw new SortitionError(n);
      return n === 0 ? [] : decoder.decode(mem().subarray(out, out + n)).split("\n");
    });
  }

  function drawData(text, k, { format, path = "", seed } = {}) {
    u32(k, "k");
    if (!(format in FORMATS)) throw new RangeError(`format must be one of ${Object.keys(FORMATS).join(", ")}`);
    const data = encoder.encode(text);
    return run(() => {
      const textPtr = putBytes(data);
      const pathPtr = putBytes(encoder.encode(path), 1);
      const seedPtr = putSeed(seed);
      const outPtr = alloc(4);
      const rc = e.sortition_draw_data(FORMATS[format], textPtr, data.length, pathPtr, k, seedPtr, outPtr);
      if (rc !== 0) throw new SortitionError(rc, cString(e.sortition_last_error()));
      const jsonPtr = view().getUint32(outPtr, true);
      try {
        return JSON.parse(cString(jsonPtr));
      } finally {
        e.sortition_free(jsonPtr);
      }
    });
  }

  /**
   * 賞ごとに順番に抽選する（重複当選なし）。
   * 合計人数を重複なしで引いて、先頭から賞ごとに切り分ける。これは「1 つ目の賞を引き、
   * 残りから次の賞を引く」と同じ確率になる。
   *
   * @param input   配列（要素はそのまま返る）/ 1 行 1 候補のテキスト / format 指定時は構造化データのテキスト
   * @param stages  [{ name, count }] を引く順に
   * @returns       [{ name, count, winners }]
   */
  function drawStages(input, stages, { seed, format, path } = {}) {
    const counts = stages.map((s, i) => {
      if (!Number.isInteger(s.count) || s.count < 0) throw new RangeError(`stages[${i}].count must be a non-negative integer`);
      return s.count;
    });
    const total = counts.reduce((a, b) => a + b, 0);
    let picked;
    if (Array.isArray(input)) {
      picked = draw(input.length, total, { seed }).map((i) => input[i]);
    } else if (format !== undefined) {
      picked = drawData(input, total, { format, path, seed });
    } else {
      picked = drawText(input, total, { seed });
    }
    let offset = 0;
    return stages.map((s, i) => {
      const winners = picked.slice(offset, offset + counts[i]);
      offset += counts[i];
      return { name: s.name, count: counts[i], winners };
    });
  }

  const version = () => cString(e.sortition_version());
  /** リニアメモリの現在のサイズ（バイト）。リークの確認用 */
  const memoryBytes = () => e.memory.buffer.byteLength;

  return { draw, drawText, drawData, drawStages, version, memoryBytes };
}
