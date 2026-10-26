// libsortition の Node.js バインディング（N-API）。
// 引数の検証と JSON の解釈は index.js に任せ、ここは C ABI を呼ぶだけの薄い層にする。
#include <node_api.h>

#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include "sortition.h"

// N-API の呼び出しが失敗したら例外を投げて NULL を返す
#define CALL(env, expr)                          \
  do {                                           \
    if ((expr) != napi_ok) return throw_napi(env); \
  } while (0)

static napi_value throw_napi(napi_env env) {
  bool pending = false;
  napi_is_exception_pending(env, &pending);
  if (!pending) {
    const napi_extended_error_info *info = NULL;
    napi_get_last_error_info(env, &info);
    napi_throw_error(env, NULL, info && info->error_message ? info->error_message : "N-API call failed");
  }
  return NULL;
}

// コアのエラーコードを sortitionCode 付きの Error として投げる（クラスへの変換は index.js）
static napi_value throw_code(napi_env env, int64_t rc, const char *detail) {
  napi_value msg, err, code;
  if (napi_create_string_utf8(env, detail && *detail ? detail : "sortition error", NAPI_AUTO_LENGTH, &msg) != napi_ok) return throw_napi(env);
  if (napi_create_error(env, NULL, msg, &err) != napi_ok) return throw_napi(env);
  if (napi_create_int32(env, (int32_t)rc, &code) != napi_ok) return throw_napi(env);
  if (napi_set_named_property(env, err, "sortitionCode", code) != napi_ok) return throw_napi(env);
  napi_throw(env, err);
  return NULL;
}

// UTF-8 文字列を取り出す（呼び出し側が free する）。NUL を含んでもよいので長さも返す
static char *get_utf8(napi_env env, napi_value v, size_t *len) {
  size_t n = 0;
  if (napi_get_value_string_utf8(env, v, NULL, 0, &n) != napi_ok) { throw_napi(env); return NULL; }
  char *buf = malloc(n + 1);
  if (!buf) { napi_throw_error(env, NULL, "out of memory"); return NULL; }
  if (napi_get_value_string_utf8(env, v, buf, n + 1, &n) != napi_ok) { free(buf); throw_napi(env); return NULL; }
  buf[n] = '\0';
  *len = n;
  return buf;
}

// seed: undefined / null → NULL、bigint / number → u64（負数は 2 の補数）。成功なら 1
static int get_seed(napi_env env, napi_value v, uint64_t *seed, const uint64_t **ptr) {
  napi_valuetype t;
  *ptr = NULL;
  if (napi_typeof(env, v, &t) != napi_ok) { throw_napi(env); return 0; }
  if (t == napi_undefined || t == napi_null) return 1;
  if (t == napi_bigint) {
    bool lossless;
    if (napi_get_value_bigint_uint64(env, v, seed, &lossless) != napi_ok) { throw_napi(env); return 0; }
    *ptr = seed;
    return 1;
  }
  if (t == napi_number) {
    int64_t i;
    if (napi_get_value_int64(env, v, &i) != napi_ok) { throw_napi(env); return 0; }
    *seed = (uint64_t)i;
    *ptr = seed;
    return 1;
  }
  napi_throw_type_error(env, NULL, "seed must be a bigint, an integer number, null or undefined");
  return 0;
}

static const char *default_message(int64_t rc) {
  switch (rc) {
    case SORTITION_ERR_INVALID: return "invalid argument (k > n?)";
    case SORTITION_ERR_RNG: return "OS random source failed";
    case SORTITION_ERR_UTF8: return "text is not valid UTF-8";
    case SORTITION_ERR_PARSE: return "parse error";
    case SORTITION_ERR_PATH: return "path not found or not an array";
    case SORTITION_ERR_FORMAT: return "unsupported format";
    case SORTITION_ERR_INTERNAL: return "internal error";
    case SORTITION_ERR_STATE: return "invalid session state";
    default: return "sortition error";
  }
}

// draw(n, k, seed) -> number[]
static napi_value js_draw(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3];
  CALL(env, napi_get_cb_info(env, info, &argc, argv, NULL, NULL));
  uint32_t n, k;
  CALL(env, napi_get_value_uint32(env, argv[0], &n));
  CALL(env, napi_get_value_uint32(env, argv[1], &k));
  uint64_t seed;
  const uint64_t *seed_ptr;
  if (!get_seed(env, argv[2], &seed, &seed_ptr)) return NULL;

  uint32_t *out = malloc(sizeof(uint32_t) * (k ? k : 1));
  if (!out) { napi_throw_error(env, NULL, "out of memory"); return NULL; }
  int32_t rc = sortition_draw(n, k, seed_ptr, out);
  if (rc != SORTITION_OK) { free(out); return throw_code(env, rc, default_message(rc)); }

  napi_value arr, item;
  if (napi_create_array_with_length(env, k, &arr) != napi_ok) { free(out); return throw_napi(env); }
  for (uint32_t i = 0; i < k; i++) {
    if (napi_create_uint32(env, out[i], &item) != napi_ok || napi_set_element(env, arr, i, item) != napi_ok) {
      free(out);
      return throw_napi(env);
    }
  }
  free(out);
  return arr;
}

// drawText(text, k, seed) -> string（選ばれた候補を "\n" で連結したもの。空なら ""）
static napi_value js_draw_text(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3];
  CALL(env, napi_get_cb_info(env, info, &argc, argv, NULL, NULL));
  size_t len;
  char *text = get_utf8(env, argv[0], &len);
  if (!text) return NULL;
  uint32_t k;
  uint64_t seed;
  const uint64_t *seed_ptr;
  if (napi_get_value_uint32(env, argv[1], &k) != napi_ok) { free(text); return throw_napi(env); }
  if (!get_seed(env, argv[2], &seed, &seed_ptr)) { free(text); return NULL; }

  uint8_t *out = malloc(len + 1);
  if (!out) { free(text); napi_throw_error(env, NULL, "out of memory"); return NULL; }
  int64_t n = sortition_draw_text((const uint8_t *)text, len, k, seed_ptr, out, len + 1);
  free(text);
  if (n < 0) { free(out); return throw_code(env, n, default_message(n)); }

  napi_value result;
  napi_status st = napi_create_string_utf8(env, (const char *)out, (size_t)n, &result);
  free(out);
  if (st != napi_ok) return throw_napi(env);
  return result;
}

// drawData(format, text, path, k, seed) -> string（選ばれた要素の JSON 配列）
static napi_value js_draw_data(napi_env env, napi_callback_info info) {
  size_t argc = 5;
  napi_value argv[5];
  CALL(env, napi_get_cb_info(env, info, &argc, argv, NULL, NULL));
  int32_t format;
  uint32_t k;
  CALL(env, napi_get_value_int32(env, argv[0], &format));
  size_t len, path_len;
  char *text = get_utf8(env, argv[1], &len);
  if (!text) return NULL;
  char *path = get_utf8(env, argv[2], &path_len);
  if (!path) { free(text); return NULL; }
  uint64_t seed;
  const uint64_t *seed_ptr;
  if (napi_get_value_uint32(env, argv[3], &k) != napi_ok) { free(text); free(path); return throw_napi(env); }
  if (!get_seed(env, argv[4], &seed, &seed_ptr)) { free(text); free(path); return NULL; }

  char *json = NULL;
  int32_t rc = sortition_draw_data(format, (const uint8_t *)text, len, path, k, seed_ptr, &json);
  free(text);
  free(path);
  if (rc != SORTITION_OK) return throw_code(env, rc, sortition_last_error());

  napi_value result;
  napi_status st = napi_create_string_utf8(env, json, NAPI_AUTO_LENGTH, &result);
  sortition_free(json);
  if (st != napi_ok) return throw_napi(env);
  return result;
}

// ---- 抽選セッション ----
// JS 側には napi_external として渡す。close() 済みなら s が NULL になり、以後の利用は例外。
// 閉じ忘れは external の finalizer（GC）で解放する。
typedef struct { sortition_session *s; } session_box;

static void session_finalize(napi_env env, void *data, void *hint) {
  (void)env; (void)hint;
  session_box *box = data;
  if (box->s) sortition_session_free(box->s);
  free(box);
}

static napi_value wrap_session(napi_env env, sortition_session *s) {
  session_box *box = malloc(sizeof *box);
  if (!box) { sortition_session_free(s); napi_throw_error(env, NULL, "out of memory"); return NULL; }
  box->s = s;
  napi_value ext;
  if (napi_create_external(env, box, session_finalize, NULL, &ext) != napi_ok) {
    sortition_session_free(s);
    free(box);
    return throw_napi(env);
  }
  return ext;
}

// external から開いているセッションを取り出す。閉じていれば例外を投げて NULL
static session_box *get_box(napi_env env, napi_value v) {
  void *data = NULL;
  if (napi_get_value_external(env, v, &data) != napi_ok) { throw_napi(env); return NULL; }
  session_box *box = data;
  if (!box->s) { napi_throw_error(env, NULL, "session is closed"); return NULL; }
  return box;
}

// sessionNew(n, seed) -> external
static napi_value js_session_new(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2];
  CALL(env, napi_get_cb_info(env, info, &argc, argv, NULL, NULL));
  uint32_t n;
  CALL(env, napi_get_value_uint32(env, argv[0], &n));
  uint64_t seed;
  const uint64_t *seed_ptr;
  if (!get_seed(env, argv[1], &seed, &seed_ptr)) return NULL;
  sortition_session *s = NULL;
  int32_t rc = sortition_session_new(n, seed_ptr, &s);
  if (rc != SORTITION_OK) return throw_code(env, rc, default_message(rc));
  return wrap_session(env, s);
}

// sessionLoad(state) -> external
static napi_value js_session_load(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1];
  CALL(env, napi_get_cb_info(env, info, &argc, argv, NULL, NULL));
  size_t len;
  char *text = get_utf8(env, argv[0], &len);
  if (!text) return NULL;
  sortition_session *s = NULL;
  int32_t rc = sortition_session_load((const uint8_t *)text, len, &s);
  free(text);
  if (rc != SORTITION_OK) return throw_code(env, rc, sortition_last_error());
  return wrap_session(env, s);
}

// sessionDraw(session, k) -> number[]
static napi_value js_session_draw(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2];
  CALL(env, napi_get_cb_info(env, info, &argc, argv, NULL, NULL));
  session_box *box = get_box(env, argv[0]);
  if (!box) return NULL;
  uint32_t k;
  CALL(env, napi_get_value_uint32(env, argv[1], &k));

  uint32_t *out = malloc(sizeof(uint32_t) * (k ? k : 1));
  if (!out) { napi_throw_error(env, NULL, "out of memory"); return NULL; }
  int32_t rc = sortition_session_draw(box->s, k, out);
  if (rc != SORTITION_OK) { free(out); return throw_code(env, rc, default_message(rc)); }

  napi_value arr, item;
  if (napi_create_array_with_length(env, k, &arr) != napi_ok) { free(out); return throw_napi(env); }
  for (uint32_t i = 0; i < k; i++) {
    if (napi_create_uint32(env, out[i], &item) != napi_ok || napi_set_element(env, arr, i, item) != napi_ok) {
      free(out);
      return throw_napi(env);
    }
  }
  free(out);
  return arr;
}

// sessionRemaining(session) / sessionDrawnCount(session) -> number
static napi_value session_count(napi_env env, napi_callback_info info, int remaining) {
  size_t argc = 1;
  napi_value argv[1], result;
  CALL(env, napi_get_cb_info(env, info, &argc, argv, NULL, NULL));
  session_box *box = get_box(env, argv[0]);
  if (!box) return NULL;
  uint32_t v = remaining ? sortition_session_remaining(box->s) : sortition_session_drawn_count(box->s);
  CALL(env, napi_create_uint32(env, v, &result));
  return result;
}
static napi_value js_session_remaining(napi_env env, napi_callback_info info) { return session_count(env, info, 1); }
static napi_value js_session_drawn_count(napi_env env, napi_callback_info info) { return session_count(env, info, 0); }

// sessionSave(session) -> string（JSON）
static napi_value js_session_save(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1];
  CALL(env, napi_get_cb_info(env, info, &argc, argv, NULL, NULL));
  session_box *box = get_box(env, argv[0]);
  if (!box) return NULL;
  char *json = NULL;
  int32_t rc = sortition_session_save(box->s, &json);
  if (rc != SORTITION_OK) return throw_code(env, rc, sortition_last_error());
  napi_value result;
  napi_status st = napi_create_string_utf8(env, json, NAPI_AUTO_LENGTH, &result);
  sortition_free(json);
  if (st != napi_ok) return throw_napi(env);
  return result;
}

// sessionClose(session) -> undefined（二重に呼んでもよい）
static napi_value js_session_close(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1];
  CALL(env, napi_get_cb_info(env, info, &argc, argv, NULL, NULL));
  void *data = NULL;
  CALL(env, napi_get_value_external(env, argv[0], &data));
  session_box *box = data;
  if (box->s) { sortition_session_free(box->s); box->s = NULL; }
  return NULL;
}

static napi_value js_version(napi_env env, napi_callback_info info) {
  (void)info;
  napi_value result;
  CALL(env, napi_create_string_utf8(env, sortition_version(), NAPI_AUTO_LENGTH, &result));
  return result;
}

static napi_value init(napi_env env, napi_value exports) {
  const struct { const char *name; napi_callback fn; } fns[] = {
    {"draw", js_draw}, {"drawText", js_draw_text}, {"drawData", js_draw_data}, {"version", js_version},
    {"sessionNew", js_session_new}, {"sessionLoad", js_session_load}, {"sessionDraw", js_session_draw},
    {"sessionRemaining", js_session_remaining}, {"sessionDrawnCount", js_session_drawn_count},
    {"sessionSave", js_session_save}, {"sessionClose", js_session_close},
  };
  for (size_t i = 0; i < sizeof(fns) / sizeof(fns[0]); i++) {
    napi_value fn;
    CALL(env, napi_create_function(env, fns[i].name, NAPI_AUTO_LENGTH, fns[i].fn, NULL, &fn));
    CALL(env, napi_set_named_property(env, exports, fns[i].name, fn));
  }
  return exports;
}

NAPI_MODULE(NODE_GYP_MODULE_NAME, init)
