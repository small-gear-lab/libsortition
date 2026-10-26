#ifdef HAVE_CONFIG_H
# include "config.h"
#endif

#include "php.h"
#include "ext/standard/info.h"
#include "ext/spl/spl_exceptions.h"
#include "ext/json/php_json.h"
#include "zend_exceptions.h"
#include "php_sortition.h"
#include "sortition.h"

#if PHP_VERSION_ID < 70400
# error "ext/sortition requires PHP 7.4 or later"
#endif

/* PHP 8.0 以降の API を 7.4 でも同じ書き方で使うための互換シム。
 * 値の誤りは 8.x が ValueError、7.x が InvalidArgumentException。
 * 引数の型の誤りはどちらも TypeError（7.x は THROW フラグを付けないと警告になる）。 */
#if PHP_VERSION_ID >= 80000
# define SORTITION_VALUE_ERROR zend_ce_value_error
# define SORTITION_PARSE_START(min, max) ZEND_PARSE_PARAMETERS_START(min, max)
# define SORTITION_PARAM_LONG_OR_NULL(dest, is_null) Z_PARAM_LONG_OR_NULL(dest, is_null)
typedef bool sortition_bool;
#else
# define SORTITION_VALUE_ERROR spl_ce_InvalidArgumentException
# define SORTITION_PARSE_START(min, max) ZEND_PARSE_PARAMETERS_START_EX(ZEND_PARSE_PARAMS_THROW, min, max)
# define SORTITION_PARAM_LONG_OR_NULL(dest, is_null) Z_PARAM_LONG_EX(dest, is_null, 1, 0)
# define RETURN_THROWS() return
typedef zend_bool sortition_bool;
#endif

#define SORTITION_THROW_VALUE(msg) zend_throw_exception(SORTITION_VALUE_ERROR, (msg), 0)

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
	sortition_bool seed_is_null = 1;
	uint64_t useed;
	uint32_t *buf;
	int32_t rc;

	SORTITION_PARSE_START(2, 3)
		Z_PARAM_LONG(n)
		Z_PARAM_LONG(k)
		Z_PARAM_OPTIONAL
		SORTITION_PARAM_LONG_OR_NULL(seed, seed_is_null)
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
	sortition_bool seed_is_null = 1;
	uint64_t useed;
	uint8_t *buf;
	int64_t n;

	SORTITION_PARSE_START(2, 3)
		Z_PARAM_STRING(text, text_len)
		Z_PARAM_LONG(k)
		Z_PARAM_OPTIONAL
		SORTITION_PARAM_LONG_OR_NULL(seed, seed_is_null)
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
			zend_throw_exception(SORTITION_VALUE_ERROR, "sortition_draw_text(): text is not valid UTF-8", (zend_long) n);
		} else if (n == SORTITION_ERR_INVALID) {
			zend_throw_exception(SORTITION_VALUE_ERROR, "sortition_draw_text(): k exceeds the number of candidates", (zend_long) n);
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
	sortition_bool seed_is_null = 1;
	uint64_t useed;
	int32_t fmt, rc;
	char *json = NULL;
	const char *detail;

	SORTITION_PARSE_START(3, 5)
		Z_PARAM_STRING(text, text_len)
		Z_PARAM_LONG(k)
		Z_PARAM_STRING(format, format_len)
		Z_PARAM_OPTIONAL
		Z_PARAM_STRING(path, path_len)
		SORTITION_PARAM_LONG_OR_NULL(seed, seed_is_null)
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
			(rc == SORTITION_ERR_INTERNAL || rc == SORTITION_ERR_RNG) ? zend_ce_exception : SORTITION_VALUE_ERROR,
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

typedef struct {
	sortition_session *s;
	zend_object std;
} sortition_session_obj;

static inline sortition_session_obj *sortition_session_from_obj(zend_object *obj)
{
	return (sortition_session_obj *) ((char *) obj - XtOffsetOf(sortition_session_obj, std));
}
#define Z_SESSION_P(zv) sortition_session_from_obj(Z_OBJ_P(zv))

static zend_object *sortition_session_create(zend_class_entry *ce)
{
	sortition_session_obj *o = zend_object_alloc(sizeof(sortition_session_obj), ce);
	o->s = NULL;
	zend_object_std_init(&o->std, ce);
	object_properties_init(&o->std, ce);
	o->std.handlers = &sortition_session_handlers;
	return &o->std;
}

static void sortition_session_free_obj(zend_object *obj)
{
	sortition_session_obj *o = sortition_session_from_obj(obj);
	if (o->s) {
		sortition_session_free(o->s);
		o->s = NULL;
	}
	zend_object_std_dtor(obj);
}

/* コアのエラーコードを例外にする。値の誤りは ValueError 系、内部・乱数の失敗は Exception */
static void sortition_session_throw(int32_t rc, const char *what, const char *detail)
{
	zend_class_entry *ce = (rc == SORTITION_ERR_INTERNAL || rc == SORTITION_ERR_RNG)
		? zend_ce_exception : SORTITION_VALUE_ERROR;
	zend_throw_exception_ex(ce, (zend_long) rc, "SortitionSession::%s(): %s", what,
		(detail && *detail) ? detail : "failed");
}

static sortition_session *sortition_session_open(zval *zv, const char *what)
{
	sortition_session_obj *o = Z_SESSION_P(zv);
	if (!o->s) {
		zend_throw_exception_ex(zend_ce_exception, 0, "SortitionSession::%s(): session is closed", what);
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
ZEND_END_ARG_INFO()

ZEND_BEGIN_ARG_INFO_EX(arginfo_session_noargs, 0, 0, 0)
ZEND_END_ARG_INFO()

ZEND_METHOD(SortitionSession, __construct)
{
	zend_long n, seed = 0;
	sortition_bool seed_is_null = 1;
	uint64_t useed;
	sortition_session_obj *o = Z_SESSION_P(ZEND_THIS);
	int32_t rc;

	SORTITION_PARSE_START(1, 2)
		Z_PARAM_LONG(n)
		Z_PARAM_OPTIONAL
		SORTITION_PARAM_LONG_OR_NULL(seed, seed_is_null)
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
	rc = sortition_session_new((uint32_t) n, seed_is_null ? NULL : &useed, &o->s);
	if (rc != SORTITION_OK) {
		o->s = NULL;
		sortition_session_throw(rc, "__construct", NULL);
		RETURN_THROWS();
	}
}

ZEND_METHOD(SortitionSession, load)
{
	char *state;
	size_t state_len;
	sortition_session *s = NULL;
	int32_t rc;

	SORTITION_PARSE_START(1, 1)
		Z_PARAM_STRING(state, state_len)
	ZEND_PARSE_PARAMETERS_END();

	rc = sortition_session_load((const uint8_t *) state, state_len, &s);
	if (rc != SORTITION_OK) {
		sortition_session_throw(rc, "load", sortition_last_error());
		RETURN_THROWS();
	}
	object_init_ex(return_value, sortition_session_ce);
	Z_SESSION_P(return_value)->s = s;
}

ZEND_METHOD(SortitionSession, draw)
{
	zend_long k;
	sortition_session *s;
	uint32_t *buf;
	int32_t rc;

	SORTITION_PARSE_START(1, 1)
		Z_PARAM_LONG(k)
	ZEND_PARSE_PARAMETERS_END();

	if (k < 0 || k > UINT32_MAX) {
		SORTITION_THROW_VALUE("SortitionSession::draw(): require 0 <= k <= 4294967295");
		RETURN_THROWS();
	}
	if (!(s = sortition_session_open(ZEND_THIS, "draw"))) {
		RETURN_THROWS();
	}
	if ((uint64_t) k > sortition_session_remaining(s)) {
		SORTITION_THROW_VALUE("SortitionSession::draw(): k exceeds the remaining count");
		RETURN_THROWS();
	}

	buf = safe_emalloc((size_t) (k ? k : 1), sizeof(uint32_t), 0);
	rc = sortition_session_draw(s, (uint32_t) k, buf);
	if (rc != SORTITION_OK) {
		efree(buf);
		sortition_session_throw(rc, "draw", NULL);
		RETURN_THROWS();
	}
	array_init_size(return_value, (uint32_t) k);
	for (zend_long i = 0; i < k; i++) {
		add_next_index_long(return_value, (zend_long) buf[i]);
	}
	efree(buf);
}

ZEND_METHOD(SortitionSession, remaining)
{
	sortition_session *s;
	ZEND_PARSE_PARAMETERS_NONE();
	if (!(s = sortition_session_open(ZEND_THIS, "remaining"))) {
		RETURN_THROWS();
	}
	RETURN_LONG((zend_long) sortition_session_remaining(s));
}

ZEND_METHOD(SortitionSession, drawnCount)
{
	sortition_session *s;
	ZEND_PARSE_PARAMETERS_NONE();
	if (!(s = sortition_session_open(ZEND_THIS, "drawnCount"))) {
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
	if (!(s = sortition_session_open(ZEND_THIS, "save"))) {
		RETURN_THROWS();
	}
	rc = sortition_session_save(s, &json);
	if (rc != SORTITION_OK) {
		sortition_session_throw(rc, "save", sortition_last_error());
		RETURN_THROWS();
	}
	RETVAL_STRING(json);
	sortition_free(json);
}

/* コア側のセッションを解放する。二重に呼んでもよい */
ZEND_METHOD(SortitionSession, close)
{
	sortition_session_obj *o = Z_SESSION_P(ZEND_THIS);
	ZEND_PARSE_PARAMETERS_NONE();
	if (o->s) {
		sortition_session_free(o->s);
		o->s = NULL;
	}
}

static const zend_function_entry sortition_session_methods[] = {
	ZEND_ME(SortitionSession, __construct, arginfo_session_construct, ZEND_ACC_PUBLIC)
	ZEND_ME(SortitionSession, load, arginfo_session_load, ZEND_ACC_PUBLIC | ZEND_ACC_STATIC)
	ZEND_ME(SortitionSession, draw, arginfo_session_draw, ZEND_ACC_PUBLIC)
	ZEND_ME(SortitionSession, remaining, arginfo_session_noargs, ZEND_ACC_PUBLIC)
	ZEND_ME(SortitionSession, drawnCount, arginfo_session_noargs, ZEND_ACC_PUBLIC)
	ZEND_ME(SortitionSession, save, arginfo_session_noargs, ZEND_ACC_PUBLIC)
	ZEND_ME(SortitionSession, close, arginfo_session_noargs, ZEND_ACC_PUBLIC)
	PHP_FE_END
};

PHP_MINIT_FUNCTION(sortition)
{
	zend_class_entry ce;
	INIT_CLASS_ENTRY(ce, "SortitionSession", sortition_session_methods);
	sortition_session_ce = zend_register_internal_class(&ce);
	sortition_session_ce->ce_flags |= ZEND_ACC_FINAL;
	sortition_session_ce->create_object = sortition_session_create;

	memcpy(&sortition_session_handlers, &std_object_handlers, sizeof(zend_object_handlers));
	sortition_session_handlers.offset = XtOffsetOf(sortition_session_obj, std);
	sortition_session_handlers.free_obj = sortition_session_free_obj;
	sortition_session_handlers.clone_obj = NULL;
	return SUCCESS;
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

#if PHP_VERSION_ID < 80000
static const zend_module_dep sortition_deps[] = {
	ZEND_MOD_REQUIRED("json")
	ZEND_MOD_END
};
# define SORTITION_MODULE_HEADER STANDARD_MODULE_HEADER_EX, NULL, sortition_deps,
#else
# define SORTITION_MODULE_HEADER STANDARD_MODULE_HEADER,
#endif

zend_module_entry sortition_module_entry = {
	SORTITION_MODULE_HEADER
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
