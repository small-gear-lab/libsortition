// ブラウザで抽選セッションを保存し、リロードやタブの再起動のあとに続きから引くための補助（依存なし）。
// サーバーは要らない。状態は IndexedDB（なければ localStorage、それもなければメモリ）に置く。
//
//   import { load } from "./sortition.js";
//   import { createPersistence } from "./persist.js";
//   const lots = createPersistence(await load());
//
//   const lot = await lots.create({ n: 100 });              // 数値のセッション
//   const lot2 = await lots.createItems(["alice", "bob"]);  // 候補（文字列・バイナリ）のセッション
//   const picks = await lot.draw(3);                        // 引くたびに保存される
//   // …リロード…
//   const again = await lots.resume(lot.id);                // 続きから。なければ null
//   await again.draw(2);
//
// 引き継ぎの仕組み:
//  - draw は羃等キー付きで呼ぶ（省略時は呼び出しごとに自動で作る）。通信や保存の失敗でやり直しても二重に引かない。
//  - 保存は「読んだ revision のときだけ更新」で、別のタブが先に進めていたら読み直して引き直す。
//    タブ間の排他には Web Locks を使い、なければ同じページ内の排他だけになる。
//  - export / import で、状態をテキストにして別の端末へ移せる（乱数の内部状態を含むので、
//    本人以外には渡さないこと）。

const RECORD_VERSION = 1;
const MAX_ATTEMPTS = 5;

/** 保存先で、読んだ revision と食い違っていた（別のタブが先に進めた） */
export class ConflictError extends Error {
  constructor(message = "the session was updated elsewhere") {
    super(message);
    this.name = "ConflictError";
  }
}

/** 指定した id のセッションが保存されていない */
export class NotFoundError extends Error {
  constructor(id) {
    super(`session not found: ${id}`);
    this.name = "NotFoundError";
    this.id = id;
  }
}

// ---- 保存先 ----
// ストアは { get(id), put(id, record, { expectedRevision }), delete(id), list() } を非同期で実装する。
// expectedRevision: null なら「まだ存在しないとき」だけ、数値なら「その revision のとき」だけ書く（違えば ConflictError）。
// undefined なら無条件で書く。

function checkExpected(existing, expectedRevision) {
  if (expectedRevision === undefined) return;
  if (expectedRevision === null ? existing !== null && existing !== undefined : !existing || existing.revision !== expectedRevision) {
    throw new ConflictError();
  }
}

/** メモリだけの保存先（テストや、保存できない環境のフォールバック） */
export function memoryStore() {
  const map = new Map();
  return {
    async get(id) {
      const r = map.get(id);
      return r === undefined ? null : structuredClone(r);
    },
    async put(id, record, { expectedRevision } = {}) {
      checkExpected(map.get(id), expectedRevision);
      map.set(id, structuredClone(record));
    },
    async delete(id) {
      map.delete(id);
    },
    async list() {
      return [...map.values()].map((r) => structuredClone(r));
    },
  };
}

/** localStorage の保存先。別のタブとの競合は Web Locks と revision の比較で防ぐ（読み書きの間は厳密には原子的でない） */
export function localStorageStore({ prefix = "sortition:", storage = globalThis.localStorage } = {}) {
  const read = (id) => {
    const text = storage.getItem(prefix + id);
    return text === null ? null : JSON.parse(text);
  };
  return {
    async get(id) {
      return read(id);
    },
    async put(id, record, { expectedRevision } = {}) {
      checkExpected(read(id), expectedRevision);
      storage.setItem(prefix + id, JSON.stringify(record));
    },
    async delete(id) {
      storage.removeItem(prefix + id);
    },
    async list() {
      const out = [];
      for (let i = 0; i < storage.length; i++) {
        const key = storage.key(i);
        if (key && key.startsWith(prefix)) out.push(JSON.parse(storage.getItem(key)));
      }
      return out;
    },
  };
}

/** IndexedDB の保存先。書き込みは 1 つのトランザクションの中で revision を比べるので、タブをまたいでも原子的 */
export function indexedDBStore({ name = "sortition", storeName = "sessions", indexedDB = globalThis.indexedDB } = {}) {
  let opened;
  const open = () =>
    (opened ??= new Promise((resolve, reject) => {
      const req = indexedDB.open(name, 1);
      req.onupgradeneeded = () => req.result.createObjectStore(storeName, { keyPath: "id" });
      req.onsuccess = () => resolve(req.result);
      req.onerror = () => reject(req.error);
    }));
  const done = (tx) =>
    new Promise((resolve, reject) => {
      tx.oncomplete = () => resolve();
      tx.onabort = () => reject(tx.error ?? new Error("transaction aborted"));
      tx.onerror = () => reject(tx.error);
    });
  const request = (req) =>
    new Promise((resolve, reject) => {
      req.onsuccess = () => resolve(req.result);
      req.onerror = () => reject(req.error);
    });
  return {
    async get(id) {
      const db = await open();
      return (await request(db.transaction(storeName).objectStore(storeName).get(id))) ?? null;
    },
    async put(id, record, { expectedRevision } = {}) {
      const db = await open();
      const tx = db.transaction(storeName, "readwrite");
      const objects = tx.objectStore(storeName);
      const existing = (await request(objects.get(id))) ?? null;
      try {
        checkExpected(existing, expectedRevision);
      } catch (e) {
        tx.abort();
        await done(tx).catch(() => {});
        throw e;
      }
      objects.put(record);
      await done(tx);
    },
    async delete(id) {
      const db = await open();
      const tx = db.transaction(storeName, "readwrite");
      tx.objectStore(storeName).delete(id);
      await done(tx);
    },
    async list() {
      const db = await open();
      return request(db.transaction(storeName).objectStore(storeName).getAll());
    },
  };
}

/** 使える保存先を選ぶ: IndexedDB → localStorage → メモリ */
export function defaultStore() {
  try {
    if (globalThis.indexedDB) return indexedDBStore();
  } catch {}
  try {
    if (globalThis.localStorage) {
      globalThis.localStorage.getItem("sortition:probe");
      return localStorageStore();
    }
  } catch {}
  return memoryStore();
}

// ---- 排他 ----

const chains = new Map();

/** 同じ id の処理を直列にする。Web Locks があればタブをまたぐ。なければこのページ内だけ。 */
function withLock(id, fn) {
  const locks = globalThis.navigator?.locks;
  if (locks) return locks.request(`sortition:${id}`, fn);
  const prev = chains.get(id) ?? Promise.resolve();
  const next = prev.then(fn, fn);
  chains.set(id, next.catch(() => {}));
  return next;
}

// ---- 候補の保存形式 ----

const toBase64 = (bytes) => {
  let s = "";
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(s);
};
const fromBase64 = (text) => Uint8Array.from(atob(text), (c) => c.charCodeAt(0));

/** 候補（string / ArrayBuffer / TypedArray）を JSON にできる形にする */
function encodeItems(items) {
  return Array.from(items, (item) => {
    if (typeof item === "string") return ["s", item];
    if (item instanceof ArrayBuffer) return ["b", toBase64(new Uint8Array(item))];
    if (ArrayBuffer.isView(item)) return ["b", toBase64(new Uint8Array(item.buffer, item.byteOffset, item.byteLength))];
    throw new TypeError("candidate must be a string, ArrayBuffer or a typed array");
  });
}

const decodeItems = (encoded) => encoded.map(([type, value]) => (type === "s" ? value : fromBase64(value)));

// ---- 永続化つきのセッション ----

/**
 * 保存先と WASM を結びつける。
 * @param sortition `load()` が返したオブジェクト
 * @param options.store 保存先（省略時は IndexedDB → localStorage → メモリ）
 */
export function createPersistence(sortition, { store = defaultStore() } = {}) {
  const newKey = () => globalThis.crypto.randomUUID();

  /** セッションの種類ごとの違い（候補の復元だけが違う） */
  const kinds = {
    numbers: { restore: (rec) => sortition.loadSession(rec.state) },
    items: { restore: (rec) => sortition.loadItemSession(rec.state, decodeItems(rec.items)) },
  };

  const snapshot = (kind, session, items, label) => ({
    v: RECORD_VERSION,
    id: session.id,
    kind,
    revision: session.revision,
    updatedAt: Date.now(),
    label: label ?? null,
    state: session.save(),
    ...(items ? { items } : {}),
  });

  class PersistentSession {
    #id;
    #kind;
    #session;
    #items;
    #label;

    constructor(kind, session, items, label) {
      this.#id = session.id; // 閉じたあとも読めるように控えておく
      this.#kind = kind;
      this.#session = session;
      this.#items = items;
      this.#label = label;
    }

    get id() {
      return this.#id;
    }
    get kind() {
      return this.#kind;
    }
    get revision() {
      return this.#session.revision;
    }
    get remaining() {
      return this.#session.remaining;
    }
    get drawnCount() {
      return this.#session.drawnCount;
    }

    /** ここまでに引いた結果（引いた順）。数値のセッションは番号、候補のセッションは ID */
    get drawn() {
      return JSON.parse(this.#session.save()).drawn;
    }

    #rebuild(record) {
      this.#session.close();
      this.#session = kinds[this.#kind].restore(record);
    }

    /**
     * 続きから k 個引いて保存する。key を省略すると呼び出しごとに自動で作る。
     * 保存に失敗しても、同じ key で呼び直せば二重に引かない。別のタブが先に進めていたら、
     * 読み直して続きから引く。
     */
    draw(k, { key = newKey() } = {}) {
      return withLock(this.id, async () => {
        for (let attempt = 0; attempt < MAX_ATTEMPTS; attempt++) {
          const stored = await store.get(this.id);
          if (!stored) throw new NotFoundError(this.id);
          if (attempt > 0 || stored.revision !== this.#session.revision) this.#rebuild(stored);
          const picks = this.#session.draw(k, { key });
          try {
            await store.put(this.id, snapshot(this.#kind, this.#session, this.#items, this.#label), {
              expectedRevision: stored.revision,
            });
            return picks;
          } catch (e) {
            if (!(e instanceof ConflictError)) throw e;
          }
        }
        throw new ConflictError("the session keeps being updated elsewhere");
      });
    }

    /** 保存されている状態を書き出す（別の端末で import できる） */
    export() {
      return lots.export(this.id);
    }

    /** コア側のメモリを解放する（保存した状態は残る）。以後このオブジェクトは使えない */
    close() {
      this.#session.close();
    }

    /** 保存した状態も消す */
    async delete() {
      this.#session.close();
      await store.delete(this.id);
    }
  }

  async function adopt(kind, session, items, label) {
    try {
      await store.put(session.id, snapshot(kind, session, items, label), { expectedRevision: null });
    } catch (e) {
      session.close();
      throw e;
    }
    return new PersistentSession(kind, session, items, label);
  }

  const lots = {
    /** 0..n-1 を母集団とするセッションを作って保存する */
    async create({ n, seed, label } = {}) {
      return adopt("numbers", sortition.createSession(n, { seed }), undefined, label);
    },

    /** 候補（string / ArrayBuffer / TypedArray）のセッションを作って保存する。候補も一緒に保存する */
    async createItems(items, { seed, label } = {}) {
      const encoded = encodeItems(items);
      return adopt("items", sortition.createItemSession(decodeItems(encoded), { seed }), encoded, label);
    },

    /** 保存してあるセッションを続きから使う。なければ null */
    async resume(id) {
      const record = await store.get(id);
      if (!record) return null;
      return new PersistentSession(record.kind, kinds[record.kind].restore(record), record.items, record.label);
    },

    /** 保存してあるセッションの一覧（新しい順） */
    async list() {
      const records = await store.list();
      return records
        .map(({ id, kind, revision, updatedAt, label }) => ({ id, kind, revision, updatedAt, label }))
        .sort((a, b) => b.updatedAt - a.updatedAt);
    },

    async remove(id) {
      await store.delete(id);
    },

    /** 保存されている状態を JSON 文字列にする（乱数の内部状態を含むので、本人以外には渡さないこと） */
    async export(id) {
      const record = await store.get(id);
      if (!record) throw new NotFoundError(id);
      return JSON.stringify(record);
    },

    /**
     * export() の出力を取り込む。状態が正しいか確かめてから保存する（不正なら SortitionError）。
     * 同じ id が既にあれば、revision が大きいほうを残す（overwrite: true なら常に上書き）。
     */
    async import(text, { overwrite = false } = {}) {
      const record = JSON.parse(text);
      if (record?.v !== RECORD_VERSION || !kinds[record.kind]) throw new RangeError("unsupported record");
      const session = kinds[record.kind].restore(record); // 検証（不正なら例外）
      const fresh = snapshot(record.kind, session, record.items, record.label);
      session.close();
      const existing = await store.get(fresh.id);
      if (existing && !overwrite && existing.revision >= fresh.revision) return fresh.id;
      await store.put(fresh.id, fresh, { expectedRevision: existing ? existing.revision : null });
      return fresh.id;
    },
  };
  return lots;
}
