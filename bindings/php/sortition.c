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
	NULL, NULL, NULL, NULL,
	PHP_MINFO(sortition),
	PHP_SORTITION_VERSION,
	STANDARD_MODULE_PROPERTIES
};

#ifdef COMPILE_DL_SORTITION
ZEND_GET_MODULE(sortition)
#endif
