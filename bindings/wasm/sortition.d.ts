export type Seed = bigint | number | null | undefined;
export type Format = "json" | "yaml" | "toml";

export class SortitionError extends RangeError {
  /** SORTITION_ERR_* の値（負数） */
  code: number;
}

export interface DrawOptions {
  /** 指定すると結果が再現できる（u64）。省略すると乱数源から取る */
  seed?: Seed;
}

export interface DataOptions extends DrawOptions {
  format: Format;
  /** 配列の場所（"a.b.0" のドット区切り）。空ならルート。TOML は必須 */
  path?: string;
}

export interface Stage {
  name: string;
  count: number;
}

export interface StageResult<T> {
  name: string;
  count: number;
  winners: T[];
}

export interface Session {
  /** 続きから k 個引く。残りを超えると SortitionError（状態は変わらない） */
  draw(k: number): number[];
  /** まだ引いていない数 */
  readonly remaining: number;
  /** ここまでに引いた数 */
  readonly drawnCount: number;
  /** 状態を JSON 文字列で返す。引いた結果（"drawn"）も含む */
  save(): string;
  /** コア側のセッションを解放する（以後の利用は Error）。二重に呼んでもよい */
  close(): void;
}

export interface Sortition {
  /** 0..n-1 から k 個を重複なしで抽選する（選ばれた順） */
  draw(n: number, k: number, options?: DrawOptions): number[];
  /** 1 行 1 候補のテキストから k 個を抽選する（前後の空白を除き、空行は捨てる） */
  drawText(text: string, k: number, options?: DrawOptions): string[];
  /** JSON / YAML / TOML のテキストにある配列から k 個を抽選する（要素は JSON に正規化される） */
  drawData(text: string, k: number, options: DataOptions): unknown[];
  /** 賞ごとに順番に抽選する（重複当選なし）。配列は要素そのまま、テキストは 1 行 1 候補 */
  drawStages<T>(input: T[], stages: Stage[], options?: DrawOptions): StageResult<T>[];
  drawStages(input: string, stages: Stage[], options?: DrawOptions): StageResult<string>[];
  drawStages(input: string, stages: Stage[], options: DataOptions): StageResult<unknown>[];
  /**
   * 0..n-1 を母集団に、呼ぶたびに続きから非復元抽出するセッションを作る。
   * seed 指定ありなら、何回に分けて引いても draw(n, k1 + k2 + ..., { seed }) と同じ並びになる
   */
  createSession(n: number, options?: DrawOptions): Session;
  /** Session.save() の出力から復元する。不正な状態は SortitionError（code -8） */
  loadSession(state: string): Session;
  version(): string;
  /** リニアメモリの現在のサイズ（バイト）。リークの確認用 */
  memoryBytes(): number;
}

/** WASM を読み込む。省略時は同じ場所の sortition.wasm */
export function load(
  source?: ArrayBuffer | ArrayBufferView | WebAssembly.Module | Response | URL | string,
): Promise<Sortition>;

/** 文字列から seed（SHA-256 の先頭 8 バイト、リトルエンディアン）を決める */
export function seedFromText(text: string): Promise<bigint>;

/** 候補テキストを抽選と同じ規則で正規化する */
export function normalizeCandidates(text: string): string[];
