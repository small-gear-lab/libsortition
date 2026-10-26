#ifndef SORTITION_H
#define SORTITION_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define SORTITION_OK           0
#define SORTITION_ERR_INVALID (-1)
#define SORTITION_ERR_RNG     (-2)
#define SORTITION_ERR_UTF8    (-3)
#define SORTITION_ERR_PARSE   (-4) /* 構文エラー（YAML のエイリアス等も含む） */
#define SORTITION_ERR_PATH    (-5) /* path が見つからない / 配列でない */
#define SORTITION_ERR_FORMAT  (-6) /* 未知の形式、またはビルドで無効化 */
#define SORTITION_ERR_INTERNAL (-7) /* 内部エラー（パニックを捕捉） */
#define SORTITION_ERR_STATE   (-8) /* 保存したセッション状態が不正 */

/* 0..n から k 個を重複なしで抽選し out[0..k] に書く。
 * seed が NULL なら OS 乱数、非 NULL ならその値で再現可能。 */
int32_t sortition_draw(uint32_t n, uint32_t k, const uint64_t *seed, uint32_t *out);

/* UTF-8 テキスト（1 行 1 候補）から k 個を重複なしで抽選し、選ばれた候補を
 * '\n' 区切りで out に書く（末尾の改行・NUL なし）。
 * 各行は前後の空白を除き、空行は候補に含めない。同じ文字列の行は別候補。
 * 戻り値は書いたバイト数、失敗時は負のエラーコード。
 * 出力は入力より長くならないので out_cap >= len の領域を渡せばよい。 */
int64_t sortition_draw_text(const uint8_t *text, size_t len, uint32_t k,
                            const uint64_t *seed, uint8_t *out, size_t out_cap);

#define SORTITION_FORMAT_JSON 0
#define SORTITION_FORMAT_YAML 1
#define SORTITION_FORMAT_TOML 2

/* 構造化データ（JSON / YAML / TOML）の配列から k 個を重複なしで抽選し、
 * 選ばれた要素を JSON 配列の文字列として *out に返す。
 *
 * path は配列の場所（"a.b.0" のドット区切り、NUL 終端 UTF-8）。NULL か "" ならルート。
 * TOML はルートが必ずテーブルなので path が必要。
 * 成功すると *out は NUL 終端の JSON 文字列（sortition_free で解放）、失敗時は NULL。
 * 要素は JSON に正規化される（TOML の日時は文字列、YAML のエイリアス・独自タグ・
 * 複数ドキュメントは拒否）。
 * 失敗の詳細は sortition_last_error() で取れる。 */
int32_t sortition_draw_data(int32_t format, const uint8_t *text, size_t len,
                            const char *path, uint32_t k, const uint64_t *seed,
                            char **out);

/* sortition_draw_data が返した文字列を解放する（NULL は無視） */
void sortition_free(char *s);

/* 同じスレッドで最後に失敗した sortition_draw_data の詳細メッセージ。
 * 次にこのスレッドで sortition_draw_data を呼ぶまで有効、解放不要。 */
const char *sortition_last_error(void);

/* ---- 抽選セッション ----
 * 0..n を母集団に、呼ぶたびに続きから非復元抽出する。状態は JSON で保存・復元できる。
 * seed 指定ありなら乱数列の状態も保存され、何回に分けて引いても
 * sortition_draw(n, k1 + k2 + ..., seed) と同じ並びになる。
 * 1 つのセッションを複数スレッドから同時に使ってはいけない。 */
typedef struct sortition_session sortition_session;

/* seed が NULL なら OS 乱数。*out は sortition_session_free で解放。 */
int32_t sortition_session_new(uint32_t n, const uint64_t *seed, sortition_session **out);

/* 続きから k 個引いて out[0..k] に書く。k が残りを超えると SORTITION_ERR_INVALID（状態は変わらない）。 */
int32_t sortition_session_draw(sortition_session *s, uint32_t k, uint32_t *out);

uint32_t sortition_session_remaining(const sortition_session *s);   /* まだ引いていない数 */
uint32_t sortition_session_drawn_count(const sortition_session *s); /* 引いた数 */

/* 状態を JSON 文字列として *out に返す（sortition_free で解放）。
 * 引いた結果（"drawn"）も入っているので、ホストはこれを保存すれば履歴も復元できる。 */
int32_t sortition_session_save(const sortition_session *s, char **out);

/* sortition_session_save の出力から復元する。不正・矛盾した状態は SORTITION_ERR_STATE
 * （詳細は sortition_last_error）。 */
int32_t sortition_session_load(const uint8_t *text, size_t len, sortition_session **out);

void sortition_session_free(sortition_session *s); /* NULL は無視 */

/* ---- バイト列の候補を抽選する ----
 * 各候補を並び順に依存しない一意な ID（SHA-256 のバイト列、32 バイト）に変換し、ID の昇順に
 * 並べた上で番号を引く。同じ候補集合とシードなら、入力の並びが違っても同じ候補が当選する。
 *
 * ID = SHA-256("libsortition/candidate/v1\0" || バイト列 || 出現番号)。出現番号は、同じバイト列が
 * 入力の先頭から数えて何個目かで、0 始まりの 8 バイト・リトルエンディアン。同じバイト列が
 * 複数あっても別候補として数え、ID は全候補で一意になる。
 *
 * 候補は「連結したバッファ data ＋ 各候補の長さ lens[0..count]」で渡す。
 * data は lens の合計バイト以上読める領域であること（長さ 0 の候補も可）。 */

/* 各候補の ID を入力と同じ並びで out_ids[0..count*32] に書く */
int32_t sortition_item_ids(const uint8_t *data, const size_t *lens, size_t count, uint8_t *out_ids);

/* 候補集合の指紋（32 バイト）を書く。ID を昇順に並べたものの SHA-256 で、入力の並びに依らない。
 * 抽選前にシードと一緒に公開しておくと、あとから同じ候補集合だったか検証できる。 */
int32_t sortition_item_fingerprint(const uint8_t *data, const size_t *lens, size_t count,
                                   uint8_t *out_fingerprint);

/* 候補から k 個を一度だけ抽選する。当選した候補の「入力内の位置」を out_index[0..k]、
 * ID を out_ids[0..k*32] に書く（どちらも NULL なら書かない）。k > count は SORTITION_ERR_INVALID。 */
int32_t sortition_items_draw(const uint8_t *data, const size_t *lens, size_t count, uint32_t k,
                             const uint64_t *seed, uint32_t *out_index, uint8_t *out_ids);

/* 候補集合を固定した抽選セッション（保存・復元つき）。
 * 保存状態には引いた候補の ID と候補集合の指紋が入る。復元では同じ候補集合（並びは違ってよい）を
 * 渡し直し、違えば SORTITION_ERR_STATE。復元後の out_index は渡し直した配列の中の位置になる。
 * 1 つのセッションを複数スレッドから同時に使ってはいけない。 */
typedef struct sortition_item_session sortition_item_session;

int32_t sortition_item_session_new(const uint8_t *data, const size_t *lens, size_t count,
                                   const uint64_t *seed, sortition_item_session **out);
/* 続きから k 個引く。書き込み先は sortition_items_draw と同じ。k が残りを超えると
 * SORTITION_ERR_INVALID（状態は変わらない）。 */
int32_t sortition_item_session_draw(sortition_item_session *s, uint32_t k,
                                    uint32_t *out_index, uint8_t *out_ids);
uint32_t sortition_item_session_remaining(const sortition_item_session *s);
uint32_t sortition_item_session_drawn_count(const sortition_item_session *s);
/* 状態を JSON 文字列として *out に返す（sortition_free で解放） */
int32_t sortition_item_session_save(const sortition_item_session *s, char **out);
int32_t sortition_item_session_load(const uint8_t *state, size_t state_len,
                                    const uint8_t *data, const size_t *lens, size_t count,
                                    sortition_item_session **out);
void sortition_item_session_free(sortition_item_session *s); /* NULL は無視 */

/* ホストが呼び出し用バッファを確保する（WASM のようにリニアメモリへ直接書き込む環境向け）。
 * sortition_dealloc には確保時と同じ len を渡す。 */
uint8_t *sortition_alloc(size_t len);
void sortition_dealloc(uint8_t *ptr, size_t len);

/* バージョン文字列（静的領域、解放不要） */
const char *sortition_version(void);

#ifdef __cplusplus
}
#endif
#endif
