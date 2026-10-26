#ifdef HAVE_CONFIG_H
# include "config.h"
#endif

#include "php.h"
#include "ext/standard/info.h"
#include "ext/json/php_json.h"
#include "zend_exceptions.h"
#include "php_sortition.h"
#include "sortition.h"

#if PHP_VERSION_ID < 80200
# error "ext/sortition requires PHP 8.2 or later"
#endif

#define SORTITION_THROW_VALUE(msg) zend_throw_exception(zend_ce_value_error, (msg), 0)

ZEND_BEGIN_ARG_INFO_EX(arginfo_sortition_draw, 0, 0, 2)
	ZEND_ARG_INFO(0, n)
	ZEND_ARG_INFO(0, k)
	ZEND_ARG_INFO(0, seed)
ZEND_END_ARG_INFO()

ZEND_BEGIN_ARG_INFO_EX(arginfo_sortition_draw_text, 0, 0, 2)
	ZEND_ARG_INFO(0, text)
	ZEND_ARG_INFO(0, k)
	ZEND_ARG_INFO(0, seed)
ZEND_END_ARG_INFO()

ZEND_BEGIN_ARG_INFO_EX(arginfo_sortition_draw_data, 0, 0, 3)
	ZEND_ARG_INFO(0, text)
	ZEND_ARG_INFO(0, k)
	ZEND_ARG_INFO(0, format)
	ZEND_ARG_INFO(0, path)
	ZEND_ARG_INFO(0, seed)
ZEND_END_ARG_INFO()

ZEND_BEGIN_ARG_INFO_EX(arginfo_sortition_version, 0, 0, 0)
ZEND_END_ARG_INFO()

/* 0..n-1 から k 個を重複なしで抽選して int の配列で返す */
PHP_FUNCTION(sortition_draw)
{
	zend_long n, k, seed = 0;
	bool seed_is_null = 1;
	uint64_t useed;
	uint32_t *buf;
	int32_t rc;

	ZEND_PARSE_PARAMETERS_START(2, 3)
		Z_PARAM_LONG(n)
		Z_PARAM_LONG(k)
		Z_PARAM_OPTIONAL
		Z_PARAM_LONG_OR_NULL(seed, seed_is_null)
	ZEND_PARSE_PARAMETERS_END();

	if (n < 0 || n > UINT32_MAX || k < 0 || k > n) {
		SORTITION_THROW_VALUE("sortition_draw(): require 0 <= k <= n <= 4294967295");
		RETURN_THROWS();
	}
	useed = (uint64_t) seed;

	buf = safe_emalloc((size_t) (k ? k : 1), sizeof(uint32_t), 0);
	rc = sortition_draw((uint32_t) n, (uint32_t) k, seed_is_null ? NULL : &useed, buf);
	if (rc != SORTITION_OK) {
		efree(buf);
		zend_throw_exception_ex(NULL, rc, "sortition_draw() failed (code %d)", (int) rc);
		RETURN_THROWS();
	}

	array_init_size(return_value, (uint32_t) k);
	for (zend_long i = 0; i < k; i++) {
		add_next_index_long(return_value, (zend_long) buf[i]);
	}
	efree(buf);
}

/* 1 行 1 候補のテキストから k 個を抽選し、選ばれた候補の文字列配列を返す */
PHP_FUNCTION(sortition_draw_text)
{
	char *text;
	size_t text_len;
	zend_long k, seed = 0;
	bool seed_is_null = 1;
	uint64_t useed;
	uint8_t *buf;
	int64_t n;

	ZEND_PARSE_PARAMETERS_START(2, 3)
		Z_PARAM_STRING(text, text_len)
		Z_PARAM_LONG(k)
		Z_PARAM_OPTIONAL
		Z_PARAM_LONG_OR_NULL(seed, seed_is_null)
	ZEND_PARSE_PARAMETERS_END();

	if (k < 0 || k > UINT32_MAX) {
		SORTITION_THROW_VALUE("sortition_draw_text(): require 0 <= k <= 4294967295");
		RETURN_THROWS();
	}
	useed = (uint64_t) seed;

	/* 出力は入力より長くならない */
	buf = safe_emalloc(text_len, 1, 1);
	n = sortition_draw_text((const uint8_t *) text, text_len, (uint32_t) k,
		seed_is_null ? NULL : &useed, buf, text_len + 1);
	if (n < 0) {
		efree(buf);
		if (n == SORTITION_ERR_UTF8) {
			zend_throw_exception(zend_ce_value_error, "sortition_draw_text(): text is not valid UTF-8", (zend_long) n);
		} else if (n == SORTITION_ERR_INVALID) {
			zend_throw_exception(zend_ce_value_error, "sortition_draw_text(): k exceeds the number of candidates", (zend_long) n);
		} else {
			zend_throw_exception_ex(NULL, (zend_long) n, "sortition_draw_text() failed (code %d)", (int) n);
		}
		RETURN_THROWS();
	}

	array_init(return_value);
	if (n > 0) {
		const char *p = (const char *) buf, *end = p + n, *nl;
		while ((nl = memchr(p, '\n', (size_t) (end - p))) != NULL) {
			add_next_index_stringl(return_value, p, (size_t) (nl - p));
			p = nl + 1;
		}
		add_next_index_stringl(return_value, p, (size_t) (end - p));
	}
	efree(buf);
}

/* JSON / YAML / TOML のテキストにある配列から k 個を抽選し、選ばれた要素を返す。
 * 要素は JSON を経由して正規化され、オブジェクトは連想配列になる。
 * 失敗は例外（getCode() にコアのエラーコード）。 */
PHP_FUNCTION(sortition_draw_data)
{
	char *text, *format, *path = "";
	size_t text_len, format_len, path_len = 0;
	zend_long k, seed = 0;
	bool seed_is_null = 1;
	uint64_t useed;
	int32_t fmt, rc;
	char *json = NULL;
	const char *detail;

	ZEND_PARSE_PARAMETERS_START(3, 5)
		Z_PARAM_STRING(text, text_len)
		Z_PARAM_LONG(k)
		Z_PARAM_STRING(format, format_len)
		Z_PARAM_OPTIONAL
		Z_PARAM_STRING(path, path_len)
		Z_PARAM_LONG_OR_NULL(seed, seed_is_null)
	ZEND_PARSE_PARAMETERS_END();

	if (k < 0 || k > UINT32_MAX) {
		SORTITION_THROW_VALUE("sortition_draw_data(): require 0 <= k <= 4294967295");
		RETURN_THROWS();
	}
	/* C 文字列として渡すので NUL を含めない */
	if (strlen(format) != format_len || strlen(path) != path_len) {
		SORTITION_THROW_VALUE("sortition_draw_data(): format and path must not contain NUL bytes");
		RETURN_THROWS();
	}
	if (strcmp(format, "json") == 0) {
		fmt = SORTITION_FORMAT_JSON;
	} else if (strcmp(format, "yaml") == 0) {
		fmt = SORTITION_FORMAT_YAML;
	} else if (strcmp(format, "toml") == 0) {
		fmt = SORTITION_FORMAT_TOML;
	} else {
		SORTITION_THROW_VALUE("sortition_draw_data(): format must be \"json\", \"yaml\" or \"toml\"");
		RETURN_THROWS();
	}
	useed = (uint64_t) seed;

	rc = sortition_draw_data(fmt, (const uint8_t *) text, text_len, path, (uint32_t) k,
		seed_is_null ? NULL : &useed, &json);
	if (rc != SORTITION_OK) {
		detail = sortition_last_error();
		zend_throw_exception_ex(
			(rc == SORTITION_ERR_INTERNAL || rc == SORTITION_ERR_RNG) ? zend_ce_exception : zend_ce_value_error,
			(zend_long) rc, "sortition_draw_data(): %s", (detail && *detail) ? detail : "failed");
		RETURN_THROWS();
	}

	/* オブジェクトは連想配列として受け取る */
	php_json_decode_ex(return_value, json, strlen(json), PHP_JSON_OBJECT_AS_ARRAY, PHP_JSON_PARSER_DEFAULT_DEPTH);
	sortition_free(json);
	if (Z_TYPE_P(return_value) != IS_ARRAY) {
		zval_ptr_dtor(return_value);
		ZVAL_NULL(return_value);
		zend_throw_exception(zend_ce_exception, "sortition_draw_data(): failed to decode result", 0);
		RETURN_THROWS();
	}
}

/* ---- SortitionSession: 続きから引ける抽選セッション ----
 * new SortitionSession(int $n, ?int $seed = null) / SortitionSession::load(string $state)
 * ->draw(int $k): int[] / ->remaining(): int / ->drawnCount(): int / ->save(): string / ->close(): void
 * クローン不可。close() 済み（または構築に失敗した）インスタンスの利用は例外。 */

static zend_class_entry *sortition_session_ce;
static zend_object_handlers sortition_session_handlers;

/* 数値のセッション（KIND_NUMBERS）とバイト列の候補のセッション（KIND_ITEMS）で同じ殻を使う */
enum { SORTITION_KIND_NUMBERS = 0, SORTITION_KIND_ITEMS = 1 };

typedef struct {
	void *s;
	int kind;
	zend_object std;
} sortition_session_obj;

static inline sortition_session_obj *sortition_session_from_obj(zend_object *obj)
{
	return (sortition_session_obj *) ((char *) obj - XtOffsetOf(sortition_session_obj, std));
}
#define Z_SESSION_P(zv) sortition_session_from_obj(Z_OBJ_P(zv))

static zend_object *sortition_session_create_kind(zend_class_entry *ce, int kind)
{
	sortition_session_obj *o = zend_object_alloc(sizeof(sortition_session_obj), ce);
	o->s = NULL;
	o->kind = kind;
	zend_object_std_init(&o->std, ce);
	object_properties_init(&o->std, ce);
	o->std.handlers = &sortition_session_handlers;
	return &o->std;
}

static zend_object *sortition_session_create(zend_class_entry *ce)
{
	return sortition_session_create_kind(ce, SORTITION_KIND_NUMBERS);
}

static zend_object *sortition_item_session_create(zend_class_entry *ce)
{
	return sortition_session_create_kind(ce, SORTITION_KIND_ITEMS);
}

/* コア側のセッションを解放する。二重に呼んでもよい */
static void sortition_session_release(sortition_session_obj *o)
{
	if (!o->s) {
		return;
	}
	if (o->kind == SORTITION_KIND_ITEMS) {
		sortition_item_session_free(o->s);
	} else {
		sortition_session_free(o->s);
	}
	o->s = NULL;
}

static void sortition_session_free_obj(zend_object *obj)
{
	sortition_session_release(sortition_session_from_obj(obj));
	zend_object_std_dtor(obj);
}

/* コアのエラーコードを例外にする。値の誤りは ValueError 系、内部・乱数の失敗は Exception */
static const char *sortition_default_message(int32_t rc)
{
	switch (rc) {
	case SORTITION_ERR_INVALID: return "invalid argument (k exceeds the remaining count, or the key is invalid)";
	case SORTITION_ERR_UTF8: return "text is not valid UTF-8";
	case SORTITION_ERR_STATE: return "invalid session state";
	case SORTITION_ERR_CONFLICT: return "idempotency key was already used with a different request";
	default: return "failed";
	}
}

static void sortition_throw(int32_t rc, const char *cls, const char *what, const char *detail)
{
	zend_class_entry *ce = (rc == SORTITION_ERR_INTERNAL || rc == SORTITION_ERR_RNG)
		? zend_ce_exception : zend_ce_value_error;
	zend_throw_exception_ex(ce, (zend_long) rc, "%s::%s(): %s", cls, what,
		(detail && *detail) ? detail : sortition_default_message(rc));
}

/* 羃等キー（NULL ならキーなし）と、封をする鍵（ちょうど 32 バイト）・鍵 ID の検査。違反なら例外を投げて FAILURE */
static int sortition_check_key(const char *key, size_t key_len, const char *where)
{
	if (key && memchr(key, '\0', key_len) != NULL) {
		zend_throw_exception_ex(zend_ce_value_error, 0, "%s: key must not contain NUL bytes", where);
		return FAILURE;
	}
	return SUCCESS;
}

static int sortition_check_secret(size_t secret_len, zend_long key_id, const char *where)
{
	if (secret_len != 32) {
		zend_throw_exception_ex(zend_ce_value_error, 0, "%s: key must be exactly 32 bytes", where);
		return FAILURE;
	}
	if (key_id < 0 || key_id > 255) {
		zend_throw_exception_ex(zend_ce_value_error, 0, "%s: keyId must be in 0..255", where);
		return FAILURE;
	}
	return SUCCESS;
}

/* 関数（クラスのメソッドでないもの）用 */
static void sortition_throw_fn(int32_t rc, const char *fn)
{
	zend_class_entry *ce = (rc == SORTITION_ERR_INTERNAL || rc == SORTITION_ERR_RNG)
		? zend_ce_exception : zend_ce_value_error;
	zend_throw_exception_ex(ce, (zend_long) rc, "%s(): failed (code %d)", fn, (int) rc);
}

static void *sortition_session_open(zval *zv, const char *cls, const char *what)
{
	sortition_session_obj *o = Z_SESSION_P(zv);
	if (!o->s) {
		zend_throw_exception_ex(zend_ce_exception, 0, "%s::%s(): session is closed", cls, what);
		return NULL;
	}
	return o->s;
}

ZEND_BEGIN_ARG_INFO_EX(arginfo_session_construct, 0, 0, 1)
	ZEND_ARG_INFO(0, n)
	ZEND_ARG_INFO(0, seed)
ZEND_END_ARG_INFO()

ZEND_BEGIN_ARG_INFO_EX(arginfo_session_load, 0, 0, 1)
	ZEND_ARG_INFO(0, state)
ZEND_END_ARG_INFO()

ZEND_BEGIN_ARG_INFO_EX(arginfo_session_draw, 0, 0, 1)
	ZEND_ARG_INFO(0, k)
	ZEND_ARG_INFO(0, key)
ZEND_END_ARG_INFO()

ZEND_BEGIN_ARG_INFO_EX(arginfo_session_seal, 0, 0, 1)
	ZEND_ARG_INFO(0, key)
	ZEND_ARG_INFO(0, keyId)
ZEND_END_ARG_INFO()

ZEND_BEGIN_ARG_INFO_EX(arginfo_session_unseal, 0, 0, 2)
	ZEND_ARG_INFO(0, token)
	ZEND_ARG_INFO(0, key)
ZEND_END_ARG_INFO()

ZEND_BEGIN_ARG_INFO_EX(arginfo_item_session_unseal, 0, 0, 3)
	ZEND_ARG_INFO(0, token)
	ZEND_ARG_INFO(0, key)
	ZEND_ARG_INFO(0, items)
ZEND_END_ARG_INFO()

ZEND_BEGIN_ARG_INFO_EX(arginfo_sortition_seal_key_id, 0, 0, 1)
	ZEND_ARG_INFO(0, token)
ZEND_END_ARG_INFO()

ZEND_BEGIN_ARG_INFO_EX(arginfo_session_noargs, 0, 0, 0)
ZEND_END_ARG_INFO()

ZEND_METHOD(SortitionSession, __construct)
{
	zend_long n, seed = 0;
	bool seed_is_null = 1;
	uint64_t useed;
	sortition_session_obj *o = Z_SESSION_P(ZEND_THIS);
	sortition_session *created = NULL;
	int32_t rc;

	ZEND_PARSE_PARAMETERS_START(1, 2)
		Z_PARAM_LONG(n)
		Z_PARAM_OPTIONAL
		Z_PARAM_LONG_OR_NULL(seed, seed_is_null)
	ZEND_PARSE_PARAMETERS_END();

	if (o->s) {
		zend_throw_exception(zend_ce_exception, "SortitionSession::__construct(): already constructed", 0);
		RETURN_THROWS();
	}
	if (n < 0 || n > UINT32_MAX) {
		SORTITION_THROW_VALUE("SortitionSession::__construct(): require 0 <= n <= 4294967295");
		RETURN_THROWS();
	}
	useed = (uint64_t) seed;
	rc = sortition_session_new((uint32_t) n, seed_is_null ? NULL : &useed, &created);
	if (rc != SORTITION_OK) {
		sortition_throw(rc, "SortitionSession", "__construct", NULL);
		RETURN_THROWS();
	}
	o->s = created;
}

ZEND_METHOD(SortitionSession, load)
{
	char *state;
	size_t state_len;
	sortition_session *s = NULL;
	int32_t rc;

	ZEND_PARSE_PARAMETERS_START(1, 1)
		Z_PARAM_STRING(state, state_len)
	ZEND_PARSE_PARAMETERS_END();

	rc = sortition_session_load((const uint8_t *) state, state_len, &s);
	if (rc != SORTITION_OK) {
		sortition_throw(rc, "SortitionSession", "load", sortition_last_error());
		RETURN_THROWS();
	}
	object_init_ex(return_value, sortition_session_ce);
	Z_SESSION_P(return_value)->s = s;
}

/* 続きから k 個引く。$key（羃等キー、1〜128 バイト）を付けると、同じ key と同じ k の再送は、
 * 状態を変えずに前回と同じ結果を返す。同じ key で別の k は ValueError（code -9） */
ZEND_METHOD(SortitionSession, draw)
{
	zend_long k;
	char *key = NULL;
	size_t key_len = 0;
	sortition_session *s;
	uint32_t *buf;
	int32_t rc;

	ZEND_PARSE_PARAMETERS_START(1, 2)
		Z_PARAM_LONG(k)
		Z_PARAM_OPTIONAL
		Z_PARAM_STRING_OR_NULL(key, key_len)
	ZEND_PARSE_PARAMETERS_END();

	if (k < 0 || k > UINT32_MAX) {
		SORTITION_THROW_VALUE("SortitionSession::draw(): require 0 <= k <= 4294967295");
		RETURN_THROWS();
	}
	if (sortition_check_key(key, key_len, "SortitionSession::draw()") != SUCCESS) {
		RETURN_THROWS();
	}
	if (!(s = sortition_session_open(ZEND_THIS, "SortitionSession", "draw"))) {
		RETURN_THROWS();
	}
	/* キー付きの再送は、全部引いたあとでも前回の結果を返すので、事前の検査はキーなしのときだけ */
	if (!key && (uint64_t) k > sortition_session_remaining(s)) {
		SORTITION_THROW_VALUE("SortitionSession::draw(): k exceeds the remaining count");
		RETURN_THROWS();
	}

	buf = safe_emalloc((size_t) (k ? k : 1), sizeof(uint32_t), 0);
	rc = sortition_session_draw_keyed(s, (uint32_t) k, key, buf, NULL);
	if (rc != SORTITION_OK) {
		efree(buf);
		sortition_throw(rc, "SortitionSession", "draw", NULL);
		RETURN_THROWS();
	}
	array_init_size(return_value, (uint32_t) k);
	for (zend_long i = 0; i < k; i++) {
		add_next_index_long(return_value, (zend_long) buf[i]);
	}
	efree(buf);
}

/* 状態を変えた draw の回数。保存側が「読んだ revision のときだけ更新する」ために使える */
ZEND_METHOD(SortitionSession, revision)
{
	sortition_session *s;
	ZEND_PARSE_PARAMETERS_NONE();
	if (!(s = sortition_session_open(ZEND_THIS, "SortitionSession", "revision"))) {
		RETURN_THROWS();
	}
	RETURN_LONG((zend_long) sortition_session_revision(s));
}

/* セッションの識別子（小文字 16 進 32 文字） */
ZEND_METHOD(SortitionSession, id)
{
	sortition_session *s;
	uint8_t buf[33];
	ZEND_PARSE_PARAMETERS_NONE();
	if (!(s = sortition_session_open(ZEND_THIS, "SortitionSession", "id"))) {
		RETURN_THROWS();
	}
	sortition_session_id(s, buf);
	RETURN_STRINGL((const char *) buf, 32);
}

/* 状態に封をしたトークンを返す。$key は 32 バイトの秘密鍵、$keyId は鍵の入れ替え用の目印（0〜255） */
ZEND_METHOD(SortitionSession, seal)
{
	char *key;
	size_t key_len;
	zend_long key_id = 0;
	sortition_session *s;
	char *token = NULL;
	int32_t rc;

	ZEND_PARSE_PARAMETERS_START(1, 2)
		Z_PARAM_STRING(key, key_len)
		Z_PARAM_OPTIONAL
		Z_PARAM_LONG(key_id)
	ZEND_PARSE_PARAMETERS_END();

	if (sortition_check_secret(key_len, key_id, "SortitionSession::seal()") != SUCCESS) {
		RETURN_THROWS();
	}
	if (!(s = sortition_session_open(ZEND_THIS, "SortitionSession", "seal"))) {
		RETURN_THROWS();
	}
	rc = sortition_session_seal(s, (const uint8_t *) key, (uint8_t) key_id, &token);
	if (rc != SORTITION_OK) {
		sortition_throw(rc, "SortitionSession", "seal", sortition_last_error());
		RETURN_THROWS();
	}
	RETVAL_STRING(token);
	sortition_free(token);
}

/* seal() のトークンから復元する。形式の不正・鍵違い・改ざんは ValueError（code -8） */
ZEND_METHOD(SortitionSession, unseal)
{
	char *token, *key;
	size_t token_len, key_len;
	sortition_session *s = NULL;
	int32_t rc;

	ZEND_PARSE_PARAMETERS_START(2, 2)
		Z_PARAM_STRING(token, token_len)
		Z_PARAM_STRING(key, key_len)
	ZEND_PARSE_PARAMETERS_END();

	if (sortition_check_secret(key_len, 0, "SortitionSession::unseal()") != SUCCESS) {
		RETURN_THROWS();
	}
	rc = sortition_session_unseal((const uint8_t *) token, token_len, (const uint8_t *) key, &s);
	if (rc != SORTITION_OK) {
		sortition_throw(rc, "SortitionSession", "unseal", sortition_last_error());
		RETURN_THROWS();
	}
	object_init_ex(return_value, sortition_session_ce);
	Z_SESSION_P(return_value)->s = s;
}

ZEND_METHOD(SortitionSession, remaining)
{
	sortition_session *s;
	ZEND_PARSE_PARAMETERS_NONE();
	if (!(s = sortition_session_open(ZEND_THIS, "SortitionSession", "remaining"))) {
		RETURN_THROWS();
	}
	RETURN_LONG((zend_long) sortition_session_remaining(s));
}

ZEND_METHOD(SortitionSession, drawnCount)
{
	sortition_session *s;
	ZEND_PARSE_PARAMETERS_NONE();
	if (!(s = sortition_session_open(ZEND_THIS, "SortitionSession", "drawnCount"))) {
		RETURN_THROWS();
	}
	RETURN_LONG((zend_long) sortition_session_drawn_count(s));
}

/* 状態を JSON 文字列で返す。引いた結果（"drawn"）も含む */
ZEND_METHOD(SortitionSession, save)
{
	sortition_session *s;
	char *json = NULL;
	int32_t rc;

	ZEND_PARSE_PARAMETERS_NONE();
	if (!(s = sortition_session_open(ZEND_THIS, "SortitionSession", "save"))) {
		RETURN_THROWS();
	}
	rc = sortition_session_save(s, &json);
	if (rc != SORTITION_OK) {
		sortition_throw(rc, "SortitionSession", "save", sortition_last_error());
		RETURN_THROWS();
	}
	RETVAL_STRING(json);
	sortition_free(json);
}

/* コア側のセッションを解放する。二重に呼んでもよい */
ZEND_METHOD(SortitionSession, close)
{
	ZEND_PARSE_PARAMETERS_NONE();
	sortition_session_release(Z_SESSION_P(ZEND_THIS));
}

static const zend_function_entry sortition_session_methods[] = {
	ZEND_ME(SortitionSession, __construct, arginfo_session_construct, ZEND_ACC_PUBLIC)
	ZEND_ME(SortitionSession, load, arginfo_session_load, ZEND_ACC_PUBLIC | ZEND_ACC_STATIC)
	ZEND_ME(SortitionSession, draw, arginfo_session_draw, ZEND_ACC_PUBLIC)
	ZEND_ME(SortitionSession, revision, arginfo_session_noargs, ZEND_ACC_PUBLIC)
	ZEND_ME(SortitionSession, id, arginfo_session_noargs, ZEND_ACC_PUBLIC)
	ZEND_ME(SortitionSession, seal, arginfo_session_seal, ZEND_ACC_PUBLIC)
	ZEND_ME(SortitionSession, unseal, arginfo_session_unseal, ZEND_ACC_PUBLIC | ZEND_ACC_STATIC)
	ZEND_ME(SortitionSession, remaining, arginfo_session_noargs, ZEND_ACC_PUBLIC)
	ZEND_ME(SortitionSession, drawnCount, arginfo_session_noargs, ZEND_ACC_PUBLIC)
	ZEND_ME(SortitionSession, save, arginfo_session_noargs, ZEND_ACC_PUBLIC)
	ZEND_ME(SortitionSession, close, arginfo_session_noargs, ZEND_ACC_PUBLIC)
	PHP_FE_END
};

/* ---- バイト列の候補 ----
 * 各候補は ID（SHA-256、小文字 16 進 64 文字）に変換され、ID の昇順に並べてから引かれる。
 * 同じバイト列が複数あっても出現番号が混ざるので ID は一意。候補は文字列（バイナリ可）の配列。 */

typedef struct {
	uint8_t *data;
	size_t *lens;
	size_t count;
} sortition_items_buf;

/* 文字列の配列を、連結バッファと長さ配列にする。成功したら sortition_items_buf_free で解放する */
static int sortition_items_pack(HashTable *ht, const char *where, sortition_items_buf *b)
{
	zval *zv;
	size_t total = 0, i = 0, off = 0;

	b->data = NULL;
	b->lens = NULL;
	b->count = zend_hash_num_elements(ht);

	ZEND_HASH_FOREACH_VAL(ht, zv) {
		ZVAL_DEREF(zv);
		if (Z_TYPE_P(zv) != IS_STRING) {
			zend_type_error("%s: candidates must be strings", where);
			return FAILURE;
		}
		total += Z_STRLEN_P(zv);
	} ZEND_HASH_FOREACH_END();

	b->data = safe_emalloc(total, 1, 1);
	b->lens = safe_emalloc(b->count ? b->count : 1, sizeof(size_t), 0);
	ZEND_HASH_FOREACH_VAL(ht, zv) {
		ZVAL_DEREF(zv);
		memcpy(b->data + off, Z_STRVAL_P(zv), Z_STRLEN_P(zv));
		b->lens[i++] = Z_STRLEN_P(zv);
		off += Z_STRLEN_P(zv);
	} ZEND_HASH_FOREACH_END();
	return SUCCESS;
}

static void sortition_items_buf_free(sortition_items_buf *b)
{
	if (b->data) {
		efree(b->data);
	}
	if (b->lens) {
		efree(b->lens);
	}
	b->data = NULL;
	b->lens = NULL;
}

static void sortition_hex32(const uint8_t *id, char out[64])
{
	static const char digits[] = "0123456789abcdef";
	for (int i = 0; i < 32; i++) {
		out[2 * i] = digits[id[i] >> 4];
		out[2 * i + 1] = digits[id[i] & 15];
	}
}

/* 当選結果を [['index' => int, 'id' => string], ...] にして dest に入れる */
static void sortition_picks_to_array(zval *dest, const uint32_t *indexes, const uint8_t *ids, uint32_t k)
{
	char hex[64];
	array_init_size(dest, k);
	for (uint32_t i = 0; i < k; i++) {
		zval row;
		array_init(&row);
		add_assoc_long(&row, "index", (zend_long) indexes[i]);
		sortition_hex32(ids + 32 * (size_t) i, hex);
		add_assoc_stringl(&row, "id", hex, 64);
		add_next_index_zval(dest, &row);
	}
}

ZEND_BEGIN_ARG_INFO_EX(arginfo_sortition_item_ids, 0, 0, 1)
	ZEND_ARG_INFO(0, items)
ZEND_END_ARG_INFO()

ZEND_BEGIN_ARG_INFO_EX(arginfo_sortition_draw_items, 0, 0, 2)
	ZEND_ARG_INFO(0, items)
	ZEND_ARG_INFO(0, k)
	ZEND_ARG_INFO(0, seed)
ZEND_END_ARG_INFO()

/* 各候補の ID（小文字 16 進 64 文字）を items と同じ並びの配列で返す */
PHP_FUNCTION(sortition_item_ids)
{
	HashTable *items;
	sortition_items_buf b;
	uint8_t *out;
	int32_t rc;
	char hex[64];

	ZEND_PARSE_PARAMETERS_START(1, 1)
		Z_PARAM_ARRAY_HT(items)
	ZEND_PARSE_PARAMETERS_END();

	if (sortition_items_pack(items, "sortition_item_ids()", &b) != SUCCESS) {
		RETURN_THROWS();
	}
	out = safe_emalloc(b.count, 32, 1);
	rc = sortition_item_ids(b.data, b.lens, b.count, out);
	sortition_items_buf_free(&b);
	if (rc != SORTITION_OK) {
		efree(out);
		sortition_throw_fn(rc, "sortition_item_ids");
		RETURN_THROWS();
	}
	array_init_size(return_value, (uint32_t) b.count);
	for (size_t i = 0; i < b.count; i++) {
		sortition_hex32(out + 32 * i, hex);
		add_next_index_stringl(return_value, hex, 64);
	}
	efree(out);
}

/* 候補集合の指紋（小文字 16 進 64 文字）。items の並びに依らない */
PHP_FUNCTION(sortition_item_fingerprint)
{
	HashTable *items;
	sortition_items_buf b;
	uint8_t out[32];
	int32_t rc;
	char hex[64];

	ZEND_PARSE_PARAMETERS_START(1, 1)
		Z_PARAM_ARRAY_HT(items)
	ZEND_PARSE_PARAMETERS_END();

	if (sortition_items_pack(items, "sortition_item_fingerprint()", &b) != SUCCESS) {
		RETURN_THROWS();
	}
	rc = sortition_item_fingerprint(b.data, b.lens, b.count, out);
	sortition_items_buf_free(&b);
	if (rc != SORTITION_OK) {
		sortition_throw_fn(rc, "sortition_item_fingerprint");
		RETURN_THROWS();
	}
	sortition_hex32(out, hex);
	RETURN_STRINGL(hex, 64);
}

/* items から k 個を重複なしで抽選し、[['index' => 入力内の位置, 'id' => ID], ...] を返す。
 * 同じ候補集合とシードなら、items の並びが違っても同じ候補が当選する */
PHP_FUNCTION(sortition_draw_items)
{
	HashTable *items;
	zend_long k, seed = 0;
	bool seed_is_null = 1;
	uint64_t useed;
	sortition_items_buf b;
	uint32_t *indexes;
	uint8_t *ids;
	int32_t rc;

	ZEND_PARSE_PARAMETERS_START(2, 3)
		Z_PARAM_ARRAY_HT(items)
		Z_PARAM_LONG(k)
		Z_PARAM_OPTIONAL
		Z_PARAM_LONG_OR_NULL(seed, seed_is_null)
	ZEND_PARSE_PARAMETERS_END();

	if (k < 0 || k > UINT32_MAX) {
		SORTITION_THROW_VALUE("sortition_draw_items(): require 0 <= k <= 4294967295");
		RETURN_THROWS();
	}
	if ((uint64_t) k > zend_hash_num_elements(items)) {
		SORTITION_THROW_VALUE("sortition_draw_items(): k exceeds the number of candidates");
		RETURN_THROWS();
	}
	if (sortition_items_pack(items, "sortition_draw_items()", &b) != SUCCESS) {
		RETURN_THROWS();
	}
	useed = (uint64_t) seed;
	indexes = safe_emalloc((size_t) (k ? k : 1), sizeof(uint32_t), 0);
	ids = safe_emalloc((size_t) k, 32, 1);
	rc = sortition_items_draw(b.data, b.lens, b.count, (uint32_t) k,
		seed_is_null ? NULL : &useed, indexes, ids);
	sortition_items_buf_free(&b);
	if (rc != SORTITION_OK) {
		efree(indexes);
		efree(ids);
		sortition_throw_fn(rc, "sortition_draw_items");
		RETURN_THROWS();
	}
	sortition_picks_to_array(return_value, indexes, ids, (uint32_t) k);
	efree(indexes);
	efree(ids);
}

/* ---- SortitionItemSession: 候補集合を固定した、保存・復元できる抽選セッション ----
 * new SortitionItemSession(array $items, ?int $seed = null) / SortitionItemSession::load(string $state, array $items)
 * ->draw(int $k): array / ->remaining(): int / ->drawnCount(): int / ->save(): string / ->close(): void
 * load は同じ候補集合（並びは違ってよい）を渡し直す。違えば ValueError（code -8）。 */

static zend_class_entry *sortition_item_session_ce;

ZEND_BEGIN_ARG_INFO_EX(arginfo_item_session_construct, 0, 0, 1)
	ZEND_ARG_INFO(0, items)
	ZEND_ARG_INFO(0, seed)
ZEND_END_ARG_INFO()

ZEND_BEGIN_ARG_INFO_EX(arginfo_item_session_load, 0, 0, 2)
	ZEND_ARG_INFO(0, state)
	ZEND_ARG_INFO(0, items)
ZEND_END_ARG_INFO()

ZEND_METHOD(SortitionItemSession, __construct)
{
	HashTable *items;
	zend_long seed = 0;
	bool seed_is_null = 1;
	uint64_t useed;
	sortition_session_obj *o = Z_SESSION_P(ZEND_THIS);
	sortition_items_buf b;
	sortition_item_session *created = NULL;
	int32_t rc;

	ZEND_PARSE_PARAMETERS_START(1, 2)
		Z_PARAM_ARRAY_HT(items)
		Z_PARAM_OPTIONAL
		Z_PARAM_LONG_OR_NULL(seed, seed_is_null)
	ZEND_PARSE_PARAMETERS_END();

	if (o->s) {
		zend_throw_exception(zend_ce_exception, "SortitionItemSession::__construct(): already constructed", 0);
		RETURN_THROWS();
	}
	if (sortition_items_pack(items, "SortitionItemSession::__construct()", &b) != SUCCESS) {
		RETURN_THROWS();
	}
	useed = (uint64_t) seed;
	rc = sortition_item_session_new(b.data, b.lens, b.count, seed_is_null ? NULL : &useed, &created);
	sortition_items_buf_free(&b);
	if (rc != SORTITION_OK) {
		sortition_throw(rc, "SortitionItemSession", "__construct", NULL);
		RETURN_THROWS();
	}
	o->s = created;
}

ZEND_METHOD(SortitionItemSession, load)
{
	char *state;
	size_t state_len;
	HashTable *items;
	sortition_items_buf b;
	sortition_item_session *s = NULL;
	int32_t rc;

	ZEND_PARSE_PARAMETERS_START(2, 2)
		Z_PARAM_STRING(state, state_len)
		Z_PARAM_ARRAY_HT(items)
	ZEND_PARSE_PARAMETERS_END();

	if (sortition_items_pack(items, "SortitionItemSession::load()", &b) != SUCCESS) {
		RETURN_THROWS();
	}
	rc = sortition_item_session_load((const uint8_t *) state, state_len, b.data, b.lens, b.count, &s);
	sortition_items_buf_free(&b);
	if (rc != SORTITION_OK) {
		sortition_throw(rc, "SortitionItemSession", "load", sortition_last_error());
		RETURN_THROWS();
	}
	object_init_ex(return_value, sortition_item_session_ce);
	Z_SESSION_P(return_value)->s = s;
}

/* 続きから k 個引く。[['index' => 位置, 'id' => ID], ...]。残りを超えると ValueError（状態は変わらない）。
 * $key（羃等キー）の扱いは SortitionSession::draw と同じ */
ZEND_METHOD(SortitionItemSession, draw)
{
	zend_long k;
	char *key = NULL;
	size_t key_len = 0;
	sortition_item_session *s;
	uint32_t *indexes;
	uint8_t *ids;
	int32_t rc;

	ZEND_PARSE_PARAMETERS_START(1, 2)
		Z_PARAM_LONG(k)
		Z_PARAM_OPTIONAL
		Z_PARAM_STRING_OR_NULL(key, key_len)
	ZEND_PARSE_PARAMETERS_END();

	if (k < 0 || k > UINT32_MAX) {
		SORTITION_THROW_VALUE("SortitionItemSession::draw(): require 0 <= k <= 4294967295");
		RETURN_THROWS();
	}
	if (sortition_check_key(key, key_len, "SortitionItemSession::draw()") != SUCCESS) {
		RETURN_THROWS();
	}
	if (!(s = sortition_session_open(ZEND_THIS, "SortitionItemSession", "draw"))) {
		RETURN_THROWS();
	}
	if (!key && (uint64_t) k > sortition_item_session_remaining(s)) {
		SORTITION_THROW_VALUE("SortitionItemSession::draw(): k exceeds the remaining count");
		RETURN_THROWS();
	}
	indexes = safe_emalloc((size_t) (k ? k : 1), sizeof(uint32_t), 0);
	ids = safe_emalloc((size_t) k, 32, 1);
	rc = sortition_item_session_draw_keyed(s, (uint32_t) k, key, indexes, ids, NULL);
	if (rc != SORTITION_OK) {
		efree(indexes);
		efree(ids);
		sortition_throw(rc, "SortitionItemSession", "draw", NULL);
		RETURN_THROWS();
	}
	sortition_picks_to_array(return_value, indexes, ids, (uint32_t) k);
	efree(indexes);
	efree(ids);
}

ZEND_METHOD(SortitionItemSession, revision)
{
	sortition_item_session *s;
	ZEND_PARSE_PARAMETERS_NONE();
	if (!(s = sortition_session_open(ZEND_THIS, "SortitionItemSession", "revision"))) {
		RETURN_THROWS();
	}
	RETURN_LONG((zend_long) sortition_item_session_revision(s));
}

ZEND_METHOD(SortitionItemSession, id)
{
	sortition_item_session *s;
	uint8_t buf[33];
	ZEND_PARSE_PARAMETERS_NONE();
	if (!(s = sortition_session_open(ZEND_THIS, "SortitionItemSession", "id"))) {
		RETURN_THROWS();
	}
	sortition_item_session_id(s, buf);
	RETURN_STRINGL((const char *) buf, 32);
}

/* 状態に封をしたトークンを返す。引数は SortitionSession::seal と同じ */
ZEND_METHOD(SortitionItemSession, seal)
{
	char *key;
	size_t key_len;
	zend_long key_id = 0;
	sortition_item_session *s;
	char *token = NULL;
	int32_t rc;

	ZEND_PARSE_PARAMETERS_START(1, 2)
		Z_PARAM_STRING(key, key_len)
		Z_PARAM_OPTIONAL
		Z_PARAM_LONG(key_id)
	ZEND_PARSE_PARAMETERS_END();

	if (sortition_check_secret(key_len, key_id, "SortitionItemSession::seal()") != SUCCESS) {
		RETURN_THROWS();
	}
	if (!(s = sortition_session_open(ZEND_THIS, "SortitionItemSession", "seal"))) {
		RETURN_THROWS();
	}
	rc = sortition_item_session_seal(s, (const uint8_t *) key, (uint8_t) key_id, &token);
	if (rc != SORTITION_OK) {
		sortition_throw(rc, "SortitionItemSession", "seal", sortition_last_error());
		RETURN_THROWS();
	}
	RETVAL_STRING(token);
	sortition_free(token);
}

/* seal() のトークンと、同じ候補集合（並びは違ってよい）から復元する。違えば ValueError（code -8） */
ZEND_METHOD(SortitionItemSession, unseal)
{
	char *token, *key;
	size_t token_len, key_len;
	HashTable *items;
	sortition_items_buf b;
	sortition_item_session *s = NULL;
	int32_t rc;

	ZEND_PARSE_PARAMETERS_START(3, 3)
		Z_PARAM_STRING(token, token_len)
		Z_PARAM_STRING(key, key_len)
		Z_PARAM_ARRAY_HT(items)
	ZEND_PARSE_PARAMETERS_END();

	if (sortition_check_secret(key_len, 0, "SortitionItemSession::unseal()") != SUCCESS) {
		RETURN_THROWS();
	}
	if (sortition_items_pack(items, "SortitionItemSession::unseal()", &b) != SUCCESS) {
		RETURN_THROWS();
	}
	rc = sortition_item_session_unseal((const uint8_t *) token, token_len, (const uint8_t *) key,
		b.data, b.lens, b.count, &s);
	sortition_items_buf_free(&b);
	if (rc != SORTITION_OK) {
		sortition_throw(rc, "SortitionItemSession", "unseal", sortition_last_error());
		RETURN_THROWS();
	}
	object_init_ex(return_value, sortition_item_session_ce);
	Z_SESSION_P(return_value)->s = s;
}

ZEND_METHOD(SortitionItemSession, remaining)
{
	sortition_item_session *s;
	ZEND_PARSE_PARAMETERS_NONE();
	if (!(s = sortition_session_open(ZEND_THIS, "SortitionItemSession", "remaining"))) {
		RETURN_THROWS();
	}
	RETURN_LONG((zend_long) sortition_item_session_remaining(s));
}

ZEND_METHOD(SortitionItemSession, drawnCount)
{
	sortition_item_session *s;
	ZEND_PARSE_PARAMETERS_NONE();
	if (!(s = sortition_session_open(ZEND_THIS, "SortitionItemSession", "drawnCount"))) {
		RETURN_THROWS();
	}
	RETURN_LONG((zend_long) sortition_item_session_drawn_count(s));
}

/* 状態を JSON 文字列で返す。引いた候補の ID（"drawn"）と候補集合の指紋（"fingerprint"）を含む */
ZEND_METHOD(SortitionItemSession, save)
{
	sortition_item_session *s;
	char *json = NULL;
	int32_t rc;

	ZEND_PARSE_PARAMETERS_NONE();
	if (!(s = sortition_session_open(ZEND_THIS, "SortitionItemSession", "save"))) {
		RETURN_THROWS();
	}
	rc = sortition_item_session_save(s, &json);
	if (rc != SORTITION_OK) {
		sortition_throw(rc, "SortitionItemSession", "save", sortition_last_error());
		RETURN_THROWS();
	}
	RETVAL_STRING(json);
	sortition_free(json);
}

ZEND_METHOD(SortitionItemSession, close)
{
	ZEND_PARSE_PARAMETERS_NONE();
	sortition_session_release(Z_SESSION_P(ZEND_THIS));
}

static const zend_function_entry sortition_item_session_methods[] = {
	ZEND_ME(SortitionItemSession, __construct, arginfo_item_session_construct, ZEND_ACC_PUBLIC)
	ZEND_ME(SortitionItemSession, load, arginfo_item_session_load, ZEND_ACC_PUBLIC | ZEND_ACC_STATIC)
	ZEND_ME(SortitionItemSession, draw, arginfo_session_draw, ZEND_ACC_PUBLIC)
	ZEND_ME(SortitionItemSession, revision, arginfo_session_noargs, ZEND_ACC_PUBLIC)
	ZEND_ME(SortitionItemSession, id, arginfo_session_noargs, ZEND_ACC_PUBLIC)
	ZEND_ME(SortitionItemSession, seal, arginfo_session_seal, ZEND_ACC_PUBLIC)
	ZEND_ME(SortitionItemSession, unseal, arginfo_item_session_unseal, ZEND_ACC_PUBLIC | ZEND_ACC_STATIC)
	ZEND_ME(SortitionItemSession, remaining, arginfo_session_noargs, ZEND_ACC_PUBLIC)
	ZEND_ME(SortitionItemSession, drawnCount, arginfo_session_noargs, ZEND_ACC_PUBLIC)
	ZEND_ME(SortitionItemSession, save, arginfo_session_noargs, ZEND_ACC_PUBLIC)
	ZEND_ME(SortitionItemSession, close, arginfo_session_noargs, ZEND_ACC_PUBLIC)
	PHP_FE_END
};

PHP_MINIT_FUNCTION(sortition)
{
	zend_class_entry ce;
	INIT_CLASS_ENTRY(ce, "SortitionSession", sortition_session_methods);
	sortition_session_ce = zend_register_internal_class(&ce);
	sortition_session_ce->ce_flags |= ZEND_ACC_FINAL;
	sortition_session_ce->create_object = sortition_session_create;

	INIT_CLASS_ENTRY(ce, "SortitionItemSession", sortition_item_session_methods);
	sortition_item_session_ce = zend_register_internal_class(&ce);
	sortition_item_session_ce->ce_flags |= ZEND_ACC_FINAL;
	sortition_item_session_ce->create_object = sortition_item_session_create;

	memcpy(&sortition_session_handlers, &std_object_handlers, sizeof(zend_object_handlers));
	sortition_session_handlers.offset = XtOffsetOf(sortition_session_obj, std);
	sortition_session_handlers.free_obj = sortition_session_free_obj;
	sortition_session_handlers.clone_obj = NULL;
	return SUCCESS;
}

/* トークンに記された鍵 ID（0〜255）。鍵を入れ替えている場合に、開く前に鍵を選ぶのに使う */
PHP_FUNCTION(sortition_seal_key_id)
{
	char *token;
	size_t token_len;
	uint8_t id = 0;
	int32_t rc;

	ZEND_PARSE_PARAMETERS_START(1, 1)
		Z_PARAM_STRING(token, token_len)
	ZEND_PARSE_PARAMETERS_END();

	rc = sortition_seal_key_id((const uint8_t *) token, token_len, &id);
	if (rc != SORTITION_OK) {
		zend_throw_exception_ex(zend_ce_value_error, (zend_long) rc, "sortition_seal_key_id(): %s",
			sortition_last_error());
		RETURN_THROWS();
	}
	RETURN_LONG((zend_long) id);
}

PHP_FUNCTION(sortition_version)
{
	ZEND_PARSE_PARAMETERS_NONE();
	RETURN_STRING(sortition_version());
}

static const zend_function_entry sortition_functions[] = {
	PHP_FE(sortition_draw, arginfo_sortition_draw)
	PHP_FE(sortition_draw_text, arginfo_sortition_draw_text)
	PHP_FE(sortition_draw_data, arginfo_sortition_draw_data)
	PHP_FE(sortition_item_ids, arginfo_sortition_item_ids)
	PHP_FE(sortition_item_fingerprint, arginfo_sortition_item_ids)
	PHP_FE(sortition_draw_items, arginfo_sortition_draw_items)
	PHP_FE(sortition_seal_key_id, arginfo_sortition_seal_key_id)
	PHP_FE(sortition_version, arginfo_sortition_version)
	PHP_FE_END
};

PHP_MINFO_FUNCTION(sortition)
{
	php_info_print_table_start();
	php_info_print_table_row(2, "sortition support", "enabled");
	php_info_print_table_row(2, "extension version", PHP_SORTITION_VERSION);
	php_info_print_table_row(2, "libsortition version", sortition_version());
	php_info_print_table_end();
}

zend_module_entry sortition_module_entry = {
	STANDARD_MODULE_HEADER,
	"sortition",
	sortition_functions,
	PHP_MINIT(sortition), NULL, NULL, NULL,
	PHP_MINFO(sortition),
	PHP_SORTITION_VERSION,
	STANDARD_MODULE_PROPERTIES
};

#ifdef COMPILE_DL_SORTITION
ZEND_GET_MODULE(sortition)
#endif
