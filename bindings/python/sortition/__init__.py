"""libsortition の Python バインディング（ctypes、依存なし）。"""
import collections
import ctypes
import json
import os
import sys

__all__ = ["draw", "draw_text", "draw_data", "version", "Session",
           "Pick", "item_ids", "item_fingerprint", "draw_items", "ItemSession", "SortitionError"]

_ERRORS = {
    -1: "invalid argument (k > n?)",
    -2: "OS random source failed",
    -3: "text is not valid UTF-8",
    -4: "parse error",
    -5: "path not found or not an array",
    -6: "unsupported format",
    -7: "internal error",
    -8: "invalid session state",
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
_SESSION_P = ctypes.c_void_p
_lib.sortition_session_new.argtypes = [
    ctypes.c_uint32, ctypes.POINTER(ctypes.c_uint64), ctypes.POINTER(_SESSION_P)]
_lib.sortition_session_new.restype = ctypes.c_int32
_lib.sortition_session_draw.argtypes = [_SESSION_P, ctypes.c_uint32, ctypes.POINTER(ctypes.c_uint32)]
_lib.sortition_session_draw.restype = ctypes.c_int32
_lib.sortition_session_remaining.argtypes = [_SESSION_P]
_lib.sortition_session_remaining.restype = ctypes.c_uint32
_lib.sortition_session_drawn_count.argtypes = [_SESSION_P]
_lib.sortition_session_drawn_count.restype = ctypes.c_uint32
_lib.sortition_session_save.argtypes = [_SESSION_P, ctypes.POINTER(ctypes.c_void_p)]
_lib.sortition_session_save.restype = ctypes.c_int32
_lib.sortition_session_load.argtypes = [ctypes.c_char_p, ctypes.c_size_t, ctypes.POINTER(_SESSION_P)]
_lib.sortition_session_load.restype = ctypes.c_int32
_lib.sortition_session_free.argtypes = [_SESSION_P]
_lib.sortition_session_free.restype = None

_SIZE_P = ctypes.POINTER(ctypes.c_size_t)
_U64_P = ctypes.POINTER(ctypes.c_uint64)
_lib.sortition_item_ids.argtypes = [ctypes.c_char_p, _SIZE_P, ctypes.c_size_t, ctypes.c_char_p]
_lib.sortition_item_ids.restype = ctypes.c_int32
_lib.sortition_item_fingerprint.argtypes = [ctypes.c_char_p, _SIZE_P, ctypes.c_size_t, ctypes.c_char_p]
_lib.sortition_item_fingerprint.restype = ctypes.c_int32
_lib.sortition_items_draw.argtypes = [
    ctypes.c_char_p, _SIZE_P, ctypes.c_size_t, ctypes.c_uint32, _U64_P,
    ctypes.POINTER(ctypes.c_uint32), ctypes.c_char_p]
_lib.sortition_items_draw.restype = ctypes.c_int32
_lib.sortition_item_session_new.argtypes = [
    ctypes.c_char_p, _SIZE_P, ctypes.c_size_t, _U64_P, ctypes.POINTER(_SESSION_P)]
_lib.sortition_item_session_new.restype = ctypes.c_int32
_lib.sortition_item_session_draw.argtypes = [
    _SESSION_P, ctypes.c_uint32, ctypes.POINTER(ctypes.c_uint32), ctypes.c_char_p]
_lib.sortition_item_session_draw.restype = ctypes.c_int32
_lib.sortition_item_session_remaining.argtypes = [_SESSION_P]
_lib.sortition_item_session_remaining.restype = ctypes.c_uint32
_lib.sortition_item_session_drawn_count.argtypes = [_SESSION_P]
_lib.sortition_item_session_drawn_count.restype = ctypes.c_uint32
_lib.sortition_item_session_save.argtypes = [_SESSION_P, ctypes.POINTER(ctypes.c_void_p)]
_lib.sortition_item_session_save.restype = ctypes.c_int32
_lib.sortition_item_session_load.argtypes = [
    ctypes.c_char_p, ctypes.c_size_t, ctypes.c_char_p, _SIZE_P, ctypes.c_size_t,
    ctypes.POINTER(_SESSION_P)]
_lib.sortition_item_session_load.restype = ctypes.c_int32
_lib.sortition_item_session_free.argtypes = [_SESSION_P]
_lib.sortition_item_session_free.restype = None


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


class Session:
    """抽選セッション。0..n-1 から呼ぶたびに続きから非復元抽出する。

    save() で状態を JSON 文字列にでき、Session.load() で復元して続きから引ける。
    seed 指定ありなら、何回に分けて引いても draw(n, k1 + k2 + ..., seed) と同じ並びになる。
    1 つの Session を複数スレッドから同時に使わないこと。
    """

    def __init__(self, n, seed=None):
        handle = _SESSION_P()
        seed_ptr = None if seed is None else ctypes.byref(ctypes.c_uint64(seed))
        rc = _lib.sortition_session_new(n, seed_ptr, ctypes.byref(handle))
        if rc != 0:
            raise SortitionError(rc)
        self._handle = handle

    @classmethod
    def load(cls, state):
        """save() の出力（str か bytes）から復元する。不正な状態は SortitionError。"""
        data = state.encode("utf-8") if isinstance(state, str) else bytes(state)
        handle = _SESSION_P()
        rc = _lib.sortition_session_load(data, len(data), ctypes.byref(handle))
        if rc != 0:
            detail = _lib.sortition_last_error()
            raise SortitionError(rc, detail.decode("utf-8", "replace") if detail else None)
        self = cls.__new__(cls)
        self._handle = handle
        return self

    def _live(self):
        if not self._handle:
            raise ValueError("session is closed")
        return self._handle

    def draw(self, k):
        """続きから k 個引いて list で返す。残りを超えると SortitionError（状態は変わらない）。"""
        out = (ctypes.c_uint32 * k)()
        rc = _lib.sortition_session_draw(self._live(), k, out)
        if rc != 0:
            raise SortitionError(rc)
        return list(out)

    @property
    def remaining(self):
        return _lib.sortition_session_remaining(self._live())

    @property
    def drawn_count(self):
        return _lib.sortition_session_drawn_count(self._live())

    def save(self):
        """状態を JSON 文字列で返す。引いた結果（"drawn"）も含む。"""
        out = ctypes.c_void_p()
        rc = _lib.sortition_session_save(self._live(), ctypes.byref(out))
        if rc != 0:
            raise SortitionError(rc)
        try:
            return ctypes.string_at(out.value).decode("utf-8")
        finally:
            _lib.sortition_free(out)

    def close(self):
        if self._handle:
            _lib.sortition_session_free(self._handle)
            self._handle = None

    __del__ = close

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()


Pick = collections.namedtuple("Pick", ["index", "id"])
Pick.__doc__ = "当選した候補。index は渡した items の中の位置、id は候補の ID（小文字 16 進 64 文字）。"


def _pack(items):
    """候補（bytes 系、または str は UTF-8）を、連結バッファと長さ配列にする。"""
    chunks = []
    for item in items:
        if isinstance(item, str):
            chunks.append(item.encode("utf-8"))
        elif isinstance(item, (bytes, bytearray, memoryview)):
            chunks.append(bytes(item))
        else:
            raise TypeError(f"candidate must be bytes-like or str, not {type(item).__name__}")
    lens = (ctypes.c_size_t * max(len(chunks), 1))(*map(len, chunks))
    return b"".join(chunks), lens, len(chunks)


def _picks(indexes, ids, k):
    raw = ids.raw
    return [Pick(indexes[i], raw[32 * i:32 * i + 32].hex()) for i in range(k)]


def item_ids(items):
    """各候補の ID（小文字 16 進 64 文字）を items と同じ並びで返す。

    ID は SHA-256(接頭辞 || バイト列 || 出現番号)。同じバイト列が複数あっても、
    先頭から数えた出現番号が混ざるので ID は全候補で一意になる。
    """
    data, lens, n = _pack(items)
    out = ctypes.create_string_buffer(max(32 * n, 1))
    rc = _lib.sortition_item_ids(data, lens, n, out)
    if rc != 0:
        raise SortitionError(rc)
    return [out.raw[32 * i:32 * i + 32].hex() for i in range(n)]


def item_fingerprint(items):
    """候補集合の指紋（小文字 16 進 64 文字）。items の並びに依らない。

    抽選前にシードと一緒に公開しておくと、あとから同じ候補集合だったか検証できる。
    """
    data, lens, n = _pack(items)
    out = ctypes.create_string_buffer(32)
    rc = _lib.sortition_item_fingerprint(data, lens, n, out)
    if rc != 0:
        raise SortitionError(rc)
    return out.raw.hex()


def draw_items(items, k, seed=None):
    """items（bytes 系 / str）から k 個を重複なしで抽選し、list[Pick] で返す。

    候補は ID の昇順に並べてから引くので、同じ候補集合とシードなら、items の並びが違っても
    同じ候補が当選する（Pick.index は各 items の中の位置）。
    """
    data, lens, n = _pack(items)
    indexes = (ctypes.c_uint32 * max(k, 1))()
    ids = ctypes.create_string_buffer(max(32 * k, 1))
    seed_ptr = None if seed is None else ctypes.byref(ctypes.c_uint64(seed))
    rc = _lib.sortition_items_draw(data, lens, n, k, seed_ptr, indexes, ids)
    if rc != 0:
        raise SortitionError(rc)
    return _picks(indexes, ids, k)


class ItemSession:
    """候補集合を固定した抽選セッション（保存・復元つき）。

    save() の状態には引いた候補の ID と候補集合の指紋が入る。load() では同じ候補集合
    （並びは違ってよい）を渡し直し、違えば SortitionError（code -8）。復元後の Pick.index は
    渡し直した items の中の位置になる。1 つのセッションを複数スレッドから同時に使わないこと。
    """

    def __init__(self, items, seed=None):
        data, lens, n = _pack(items)
        handle = _SESSION_P()
        seed_ptr = None if seed is None else ctypes.byref(ctypes.c_uint64(seed))
        rc = _lib.sortition_item_session_new(data, lens, n, seed_ptr, ctypes.byref(handle))
        if rc != 0:
            raise SortitionError(rc)
        self._handle = handle

    @classmethod
    def load(cls, state, items):
        """save() の出力（str か bytes）と候補から復元する。"""
        raw = state.encode("utf-8") if isinstance(state, str) else bytes(state)
        data, lens, n = _pack(items)
        handle = _SESSION_P()
        rc = _lib.sortition_item_session_load(raw, len(raw), data, lens, n, ctypes.byref(handle))
        if rc != 0:
            detail = _lib.sortition_last_error()
            raise SortitionError(rc, detail.decode("utf-8", "replace") if detail else None)
        self = cls.__new__(cls)
        self._handle = handle
        return self

    def _live(self):
        if not self._handle:
            raise ValueError("session is closed")
        return self._handle

    def draw(self, k):
        """続きから k 個引いて list[Pick] で返す。残りを超えると SortitionError（状態は変わらない）。"""
        indexes = (ctypes.c_uint32 * max(k, 1))()
        ids = ctypes.create_string_buffer(max(32 * k, 1))
        rc = _lib.sortition_item_session_draw(self._live(), k, indexes, ids)
        if rc != 0:
            raise SortitionError(rc)
        return _picks(indexes, ids, k)

    @property
    def remaining(self):
        return _lib.sortition_item_session_remaining(self._live())

    @property
    def drawn_count(self):
        return _lib.sortition_item_session_drawn_count(self._live())

    def save(self):
        """状態を JSON 文字列で返す。引いた候補の ID（"drawn"）と候補集合の指紋（"fingerprint"）を含む。"""
        out = ctypes.c_void_p()
        rc = _lib.sortition_item_session_save(self._live(), ctypes.byref(out))
        if rc != 0:
            raise SortitionError(rc)
        try:
            return ctypes.string_at(out.value).decode("utf-8")
        finally:
            _lib.sortition_free(out)

    def close(self):
        if self._handle:
            _lib.sortition_item_session_free(self._handle)
            self._handle = None

    __del__ = close

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()


def version():
    return _lib.sortition_version().decode()
