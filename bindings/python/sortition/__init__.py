"""libsortition の Python バインディング（ctypes、依存なし）。"""
import ctypes
import json
import os
import sys

__all__ = ["draw", "draw_text", "draw_data", "version", "SortitionError"]

_ERRORS = {
    -1: "invalid argument (k > n?)",
    -2: "OS random source failed",
    -3: "text is not valid UTF-8",
    -4: "parse error",
    -5: "path not found or not an array",
    -6: "unsupported format",
    -7: "internal error",
}

_FORMATS = {"json": 0, "yaml": 1, "toml": 2}


class SortitionError(ValueError):
    """コアが返したエラー。code に SORTITION_ERR_* の値が入る。"""

    def __init__(self, code, detail=None):
        self.code = code
        super().__init__(detail or _ERRORS.get(code, f"error {code}"))


def _lib_name():
    if sys.platform == "darwin":
        return "libsortition.dylib"
    if sys.platform.startswith("win"):
        return "sortition.dll"
    return "libsortition.so"


def _load():
    here = os.path.dirname(os.path.abspath(__file__))
    candidates = [
        os.environ.get("LIBSORTITION_PATH"),
        os.path.join(here, _lib_name()),
        os.path.join(here, "..", "..", "..", "target", "release", _lib_name()),
    ]
    for path in filter(None, candidates):
        if os.path.exists(path):
            return ctypes.CDLL(path)
    raise OSError("libsortition not found; set LIBSORTITION_PATH")


_lib = _load()
_lib.sortition_draw.argtypes = [
    ctypes.c_uint32, ctypes.c_uint32,
    ctypes.POINTER(ctypes.c_uint64), ctypes.POINTER(ctypes.c_uint32),
]
_lib.sortition_draw.restype = ctypes.c_int32
_lib.sortition_draw_text.argtypes = [
    ctypes.c_char_p, ctypes.c_size_t, ctypes.c_uint32,
    ctypes.POINTER(ctypes.c_uint64), ctypes.c_char_p, ctypes.c_size_t,
]
_lib.sortition_draw_text.restype = ctypes.c_int64
_lib.sortition_draw_data.argtypes = [
    ctypes.c_int32, ctypes.c_char_p, ctypes.c_size_t, ctypes.c_char_p,
    ctypes.c_uint32, ctypes.POINTER(ctypes.c_uint64), ctypes.POINTER(ctypes.c_void_p),
]
_lib.sortition_draw_data.restype = ctypes.c_int32
_lib.sortition_free.argtypes = [ctypes.c_void_p]
_lib.sortition_free.restype = None
_lib.sortition_last_error.argtypes = []
_lib.sortition_last_error.restype = ctypes.c_char_p
_lib.sortition_version.argtypes = []
_lib.sortition_version.restype = ctypes.c_char_p


def draw(n, k, seed=None):
    """0..n-1 から k 個を重複なしで抽選して list で返す。seed 指定で再現可能。"""
    out = (ctypes.c_uint32 * k)()
    seed_ptr = None if seed is None else ctypes.byref(ctypes.c_uint64(seed))
    rc = _lib.sortition_draw(n, k, seed_ptr, out)
    if rc != 0:
        raise SortitionError(rc)
    return list(out)


def draw_text(text, k, seed=None):
    """1 行 1 候補のテキストから k 個を重複なしで抽選し、選ばれた候補の list[str] を返す。

    各行は前後の空白を除き、空行は候補に含めない。同じ文字列の行は別の候補として数える。
    """
    data = text.encode("utf-8")
    out = ctypes.create_string_buffer(len(data) + 1)
    seed_ptr = None if seed is None else ctypes.byref(ctypes.c_uint64(seed))
    n = _lib.sortition_draw_text(data, len(data), k, seed_ptr, out, len(out))
    if n < 0:
        raise SortitionError(n)
    return out.raw[:n].decode("utf-8").split("\n") if n else []


def draw_data(text, k, format, path="", seed=None):
    """JSON / YAML / TOML のテキストにある配列から k 個を重複なしで抽選し、選ばれた要素を返す。

    format は "json" | "yaml" | "toml"。path は配列の場所（"a.b.0" のドット区切り）で、
    空ならルートが配列であること。TOML はルートがテーブルなので path が必要。
    要素は JSON に正規化される（TOML の日時は文字列。YAML のエイリアス・独自タグ・
    複数ドキュメントは SortitionError）。
    """
    try:
        fmt = _FORMATS[format]
    except KeyError:
        raise ValueError(f"format must be one of {sorted(_FORMATS)}") from None
    data = text.encode("utf-8")
    seed_ptr = None if seed is None else ctypes.byref(ctypes.c_uint64(seed))
    out = ctypes.c_void_p()
    rc = _lib.sortition_draw_data(
        fmt, data, len(data), path.encode("utf-8"), k, seed_ptr, ctypes.byref(out))
    if rc != 0:
        detail = _lib.sortition_last_error()
        raise SortitionError(rc, detail.decode("utf-8", "replace") if detail else None)
    try:
        return json.loads(ctypes.string_at(out.value).decode("utf-8"))
    finally:
        _lib.sortition_free(out)


def version():
    return _lib.sortition_version().decode()
