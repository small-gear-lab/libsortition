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
// 数値のセッション（kind 0）とバイト列の候補のセッション（kind 1）で同じ殻を使う。
enum { KIND_NUMBERS = 0, KIND_ITEMS = 1 };
typedef struct { void *s; int kind; } session_box;

static void session_release(session_box *box) {
  if (!box->s) return;
  if (box->kind == KIND_ITEMS) sortition_item_session_free(box->s);
  else sortition_session_free(box->s);
  box->s = NULL;
}

static void session_finalize(napi_env env, void *data, void *hint) {
  (void)env; (void)hint;
  session_release(data);
  free(data);
}

static napi_value wrap_session(napi_env env, void *s, int kind) {
  session_box *box = malloc(sizeof *box);
  if (!box) {
    session_box tmp = { s, kind };
    session_release(&tmp);
    napi_throw_error(env, NULL, "out of memory");
    return NULL;
  }
  box->s = s;
  box->kind = kind;
  napi_value ext;
  if (napi_create_external(env, box, session_finalize, NULL, &ext) != napi_ok) {
    session_release(box);
    free(box);
    return throw_napi(env);
  }
  return ext;
}

// external から開いているセッションを取り出す。種類違い・閉じていれば例外を投げて NULL
static session_box *get_box(napi_env env, napi_value v, int kind) {
  void *data = NULL;
  if (napi_get_value_external(env, v, &data) != napi_ok) { throw_napi(env); return NULL; }
  session_box *box = data;
  if (box->kind != kind) { napi_throw_type_error(env, NULL, "wrong kind of session"); return NULL; }
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
  return wrap_session(env, s, KIND_NUMBERS);
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
  return wrap_session(env, s, KIND_NUMBERS);
}

// sessionDraw(session, k) -> number[]
static napi_value js_session_draw(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2];
  CALL(env, napi_get_cb_info(env, info, &argc, argv, NULL, NULL));
  session_box *box = get_box(env, argv[0], KIND_NUMBERS);
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
  session_box *box = get_box(env, argv[0], KIND_NUMBERS);
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
  session_box *box = get_box(env, argv[0], KIND_NUMBERS);
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
  session_release(data);
  return NULL;
}

// ---- バイト列の候補 ----
// 候補は「連結したバッファ data（Uint8Array）＋ 各候補の長さ lens（Uint32Array）」で受け取る。
// 包み・検証・16 進化は index.js に任せる。

// Uint8Array / Buffer の中身を取り出す（ポインタは JS が持つ領域を指すので解放しない）
static int get_bytes(napi_env env, napi_value v, const uint8_t **p, size_t *len) {
  napi_typedarray_type type;
  void *data = NULL;
  if (napi_get_typedarray_info(env, v, &type, len, &data, NULL, NULL) != napi_ok) { throw_napi(env); return 0; }
  if (type != napi_uint8_array) { napi_throw_type_error(env, NULL, "expected a Uint8Array"); return 0; }
  *p = data;
  return 1;
}

// Uint32Array の長さ配列を size_t の配列にする（呼び出し側が free する）。count == 0 なら NULL
static int get_lens(napi_env env, napi_value v, size_t **lens, size_t *count) {
  napi_typedarray_type type;
  void *data = NULL;
  *lens = NULL;
  if (napi_get_typedarray_info(env, v, &type, count, &data, NULL, NULL) != napi_ok) { throw_napi(env); return 0; }
  if (type != napi_uint32_array) { napi_throw_type_error(env, NULL, "expected a Uint32Array"); return 0; }
  if (*count == 0) return 1;
  size_t *out = malloc(sizeof(size_t) * *count);
  if (!out) { napi_throw_error(env, NULL, "out of memory"); return 0; }
  const uint32_t *src = data;
  for (size_t i = 0; i < *count; i++) out[i] = src[i];
  *lens = out;
  return 1;
}

// 当選結果 { indexes: number[], ids: Buffer(k * 32) } を作る
static napi_value make_picks(napi_env env, const uint32_t *indexes, const uint8_t *ids, uint32_t k) {
  napi_value obj, arr, item, buf;
  void *copy = NULL;
  CALL(env, napi_create_object(env, &obj));
  CALL(env, napi_create_array_with_length(env, k, &arr));
  for (uint32_t i = 0; i < k; i++) {
    CALL(env, napi_create_uint32(env, indexes[i], &item));
    CALL(env, napi_set_element(env, arr, i, item));
  }
  CALL(env, napi_create_buffer_copy(env, 32 * (size_t)k, ids, &copy, &buf));
  CALL(env, napi_set_named_property(env, obj, "indexes", arr));
  CALL(env, napi_set_named_property(env, obj, "ids", buf));
  return obj;
}

// itemIds(data, lens) -> Buffer（count * 32）
static napi_value js_item_ids(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2], result;
  CALL(env, napi_get_cb_info(env, info, &argc, argv, NULL, NULL));
  const uint8_t *data; size_t data_len; size_t *lens; size_t count;
  if (!get_bytes(env, argv[0], &data, &data_len)) return NULL;
  if (!get_lens(env, argv[1], &lens, &count)) return NULL;
  uint8_t *out = malloc(32 * count + 1);
  if (!out) { free(lens); napi_throw_error(env, NULL, "out of memory"); return NULL; }
  int32_t rc = sortition_item_ids(data, lens, count, out);
  free(lens);
  if (rc != SORTITION_OK) { free(out); return throw_code(env, rc, default_message(rc)); }
  void *copy = NULL;
  napi_status st = napi_create_buffer_copy(env, 32 * count, out, &copy, &result);
  free(out);
  if (st != napi_ok) return throw_napi(env);
  return result;
}

// itemFingerprint(data, lens) -> Buffer（32）
static napi_value js_item_fingerprint(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2], result;
  CALL(env, napi_get_cb_info(env, info, &argc, argv, NULL, NULL));
  const uint8_t *data; size_t data_len; size_t *lens; size_t count;
  if (!get_bytes(env, argv[0], &data, &data_len)) return NULL;
  if (!get_lens(env, argv[1], &lens, &count)) return NULL;
  uint8_t out[32];
  int32_t rc = sortition_item_fingerprint(data, lens, count, out);
  free(lens);
  if (rc != SORTITION_OK) return throw_code(env, rc, default_message(rc));
  void *copy = NULL;
  CALL(env, napi_create_buffer_copy(env, 32, out, &copy, &result));
  return result;
}

// 当選結果用のバッファを用意して呼び、結果を { indexes, ids } にする
static napi_value picks_result(napi_env env, int32_t rc, uint32_t *indexes, uint8_t *ids, uint32_t k) {
  napi_value result = NULL;
  if (rc != SORTITION_OK) result = throw_code(env, rc, default_message(rc));
  else result = make_picks(env, indexes, ids, k);
  free(indexes);
  free(ids);
  return result;
}

// itemsDraw(data, lens, k, seed) -> { indexes, ids }
static napi_value js_items_draw(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4];
  CALL(env, napi_get_cb_info(env, info, &argc, argv, NULL, NULL));
  const uint8_t *data; size_t data_len; size_t *lens; size_t count;
  if (!get_bytes(env, argv[0], &data, &data_len)) return NULL;
  if (!get_lens(env, argv[1], &lens, &count)) return NULL;
  uint32_t k;
  uint64_t seed;
  const uint64_t *seed_ptr;
  if (napi_get_value_uint32(env, argv[2], &k) != napi_ok) { free(lens); return throw_napi(env); }
  if (!get_seed(env, argv[3], &seed, &seed_ptr)) { free(lens); return NULL; }
  uint32_t *indexes = malloc(sizeof(uint32_t) * (k ? k : 1));
  uint8_t *ids = malloc(32 * (size_t)k + 1);
  if (!indexes || !ids) { free(indexes); free(ids); free(lens); napi_throw_error(env, NULL, "out of memory"); return NULL; }
  int32_t rc = sortition_items_draw(data, lens, count, k, seed_ptr, indexes, ids);
  free(lens);
  return picks_result(env, rc, indexes, ids, k);
}

// itemSessionNew(data, lens, seed) -> external
static napi_value js_item_session_new(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3];
  CALL(env, napi_get_cb_info(env, info, &argc, argv, NULL, NULL));
  const uint8_t *data; size_t data_len; size_t *lens; size_t count;
  if (!get_bytes(env, argv[0], &data, &data_len)) return NULL;
  if (!get_lens(env, argv[1], &lens, &count)) return NULL;
  uint64_t seed;
  const uint64_t *seed_ptr;
  if (!get_seed(env, argv[2], &seed, &seed_ptr)) { free(lens); return NULL; }
  sortition_item_session *s = NULL;
  int32_t rc = sortition_item_session_new(data, lens, count, seed_ptr, &s);
  free(lens);
  if (rc != SORTITION_OK) return throw_code(env, rc, default_message(rc));
  return wrap_session(env, s, KIND_ITEMS);
}

// itemSessionLoad(state, data, lens) -> external
static napi_value js_item_session_load(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3];
  CALL(env, napi_get_cb_info(env, info, &argc, argv, NULL, NULL));
  size_t state_len;
  char *state = get_utf8(env, argv[0], &state_len);
  if (!state) return NULL;
  const uint8_t *data; size_t data_len; size_t *lens; size_t count;
  if (!get_bytes(env, argv[1], &data, &data_len)) { free(state); return NULL; }
  if (!get_lens(env, argv[2], &lens, &count)) { free(state); return NULL; }
  sortition_item_session *s = NULL;
  int32_t rc = sortition_item_session_load((const uint8_t *)state, state_len, data, lens, count, &s);
  free(state);
  free(lens);
  if (rc != SORTITION_OK) return throw_code(env, rc, sortition_last_error());
  return wrap_session(env, s, KIND_ITEMS);
}

// itemSessionDraw(session, k) -> { indexes, ids }
static napi_value js_item_session_draw(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2];
  CALL(env, napi_get_cb_info(env, info, &argc, argv, NULL, NULL));
  session_box *box = get_box(env, argv[0], KIND_ITEMS);
  if (!box) return NULL;
  uint32_t k;
  CALL(env, napi_get_value_uint32(env, argv[1], &k));
  uint32_t *indexes = malloc(sizeof(uint32_t) * (k ? k : 1));
  uint8_t *ids = malloc(32 * (size_t)k + 1);
  if (!indexes || !ids) { free(indexes); free(ids); napi_throw_error(env, NULL, "out of memory"); return NULL; }
  int32_t rc = sortition_item_session_draw(box->s, k, indexes, ids);
  return picks_result(env, rc, indexes, ids, k);
}

// itemSessionRemaining(session) / itemSessionDrawnCount(session) -> number
static napi_value item_session_count(napi_env env, napi_callback_info info, int remaining) {
  size_t argc = 1;
  napi_value argv[1], result;
  CALL(env, napi_get_cb_info(env, info, &argc, argv, NULL, NULL));
  session_box *box = get_box(env, argv[0], KIND_ITEMS);
  if (!box) return NULL;
  uint32_t v = remaining ? sortition_item_session_remaining(box->s) : sortition_item_session_drawn_count(box->s);
  CALL(env, napi_create_uint32(env, v, &result));
  return result;
}
static napi_value js_item_session_remaining(napi_env env, napi_callback_info info) { return item_session_count(env, info, 1); }
static napi_value js_item_session_drawn_count(napi_env env, napi_callback_info info) { return item_session_count(env, info, 0); }

// itemSessionSave(session) -> string（JSON）
static napi_value js_item_session_save(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1];
  CALL(env, napi_get_cb_info(env, info, &argc, argv, NULL, NULL));
  session_box *box = get_box(env, argv[0], KIND_ITEMS);
  if (!box) return NULL;
  char *json = NULL;
  int32_t rc = sortition_item_session_save(box->s, &json);
  if (rc != SORTITION_OK) return throw_code(env, rc, sortition_last_error());
  napi_value result;
  napi_status st = napi_create_string_utf8(env, json, NAPI_AUTO_LENGTH, &result);
  sortition_free(json);
  if (st != napi_ok) return throw_napi(env);
  return result;
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
    {"itemIds", js_item_ids}, {"itemFingerprint", js_item_fingerprint}, {"itemsDraw", js_items_draw},
    {"itemSessionNew", js_item_session_new}, {"itemSessionLoad", js_item_session_load},
    {"itemSessionDraw", js_item_session_draw}, {"itemSessionRemaining", js_item_session_remaining},
    {"itemSessionDrawnCount", js_item_session_drawn_count}, {"itemSessionSave", js_item_session_save},
  };
  for (size_t i = 0; i < sizeof(fns) / sizeof(fns[0]); i++) {
    napi_value fn;
    CALL(env, napi_create_function(env, fns[i].name, NAPI_AUTO_LENGTH, fns[i].fn, NULL, &fn));
    CALL(env, napi_set_named_property(env, exports, fns[i].name, fn));
  }
  return exports;
}

NAPI_MODULE(NODE_GYP_MODULE_NAME, init)
