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
