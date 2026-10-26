import type { Candidate, DrawOptions, Pick, Sortition } from "./sortition.js";

/** 保存先で、読んだ revision と食い違っていた（別のタブが先に進めた） */
export class ConflictError extends Error {}

/** 指定した id のセッションが保存されていない */
export class NotFoundError extends Error {
  id: string;
}

/** 保存されるレコード */
export interface SessionRecord {
  v: number;
  id: string;
  kind: "numbers" | "items";
  revision: number;
  updatedAt: number;
  label: string | null;
  /** セッションの save() の出力 */
  state: string;
  /** 候補のセッションだけ: [["s", 文字列] | ["b", base64], ...] */
  items?: Array<["s" | "b", string]>;
}

/**
 * 保存先。put の expectedRevision は、null なら「まだ存在しないとき」だけ、数値なら「その revision のとき」だけ書く
 * （違えば ConflictError を投げる）。undefined なら無条件で書く
 */
export interface Store {
  get(id: string): Promise<SessionRecord | null>;
  put(id: string, record: SessionRecord, options?: { expectedRevision?: number | null }): Promise<void>;
  delete(id: string): Promise<void>;
  list(): Promise<SessionRecord[]>;
}

/** メモリだけの保存先（テストや、保存できない環境のフォールバック） */
export function memoryStore(): Store;
/** localStorage の保存先 */
export function localStorageStore(options?: { prefix?: string; storage?: Storage }): Store;
/** IndexedDB の保存先。書き込みは 1 つのトランザクションの中で revision を比べるので、タブをまたいでも原子的 */
export function indexedDBStore(options?: { name?: string; storeName?: string; indexedDB?: IDBFactory }): Store;
/** IndexedDB → localStorage → メモリ の順に使えるものを選ぶ */
export function defaultStore(): Store;

export interface DrawCall {
  /** 羃等キー。省略すると呼び出しごとに自動で作る。同じ key で呼び直せば二重に引かない */
  key?: string;
}

export interface SessionSummary {
  id: string;
  kind: "numbers" | "items";
  revision: number;
  updatedAt: number;
  label: string | null;
}

/** 引くたびに保存されるセッション。数値のセッションの結果は number[]、候補のセッションは Pick[] */
export interface PersistentSession<T = number | Pick> {
  readonly id: string;
  readonly kind: "numbers" | "items";
  readonly revision: number;
  readonly remaining: number;
  readonly drawnCount: number;
  /** ここまでに引いた結果（引いた順）。数値のセッションは番号、候補のセッションは ID */
  readonly drawn: Array<number | string>;
  /**
   * 続きから k 個引いて保存する。別のタブが先に進めていたら読み直して続きから引く。
   * 保存に失敗しても、同じ key で呼び直せば二重に引かない
   */
  draw(k: number, options?: DrawCall): Promise<T[]>;
  /** 保存されている状態を書き出す（別の端末で import できる）。乱数の内部状態を含むので、本人以外には渡さないこと */
  export(): Promise<string>;
  /** コア側のメモリを解放する（保存した状態は残る） */
  close(): void;
  /** 保存した状態も消す */
  delete(): Promise<void>;
}

export interface Persistence {
  /** 0..n-1 を母集団とするセッションを作って保存する */
  create(options: { n: number; seed?: DrawOptions["seed"]; label?: string }): Promise<PersistentSession<number>>;
  /** 候補（string / ArrayBuffer / TypedArray）のセッションを作って保存する。候補も一緒に保存する */
  createItems(items: Candidate[], options?: { seed?: DrawOptions["seed"]; label?: string }): Promise<PersistentSession<Pick>>;
  /** 保存してあるセッションを続きから使う。なければ null */
  resume(id: string): Promise<PersistentSession | null>;
  /** 保存してあるセッションの一覧（新しい順） */
  list(): Promise<SessionSummary[]>;
  remove(id: string): Promise<void>;
  export(id: string): Promise<string>;
  /**
   * export() の出力を取り込む。状態が正しいか確かめてから保存する（不正なら SortitionError）。
   * 同じ id が既にあれば revision が大きいほうを残す（overwrite: true なら常に上書き）。取り込んだセッションの id を返す
   */
  import(text: string, options?: { overwrite?: boolean }): Promise<string>;
}

/** WASM（`load()` の結果）と保存先を結びつける。store を省略すると defaultStore() */
export function createPersistence(sortition: Sortition, options?: { store?: Store }): Persistence;
