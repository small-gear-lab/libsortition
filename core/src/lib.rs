//! 無作為抽選コア。
//!
//! `0..n` から重複なしで `k` 個を選ぶ（非復元抽出）。結果は選ばれた順に返す。
//! シード指定なしなら OS の乱数源、指定ありなら同じシードで同じ結果になる。

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{c_char, CStr, CString};

use chacha20::cipher::{KeyIvInit, StreamCipher, StreamCipherSeek};
use chacha20::ChaCha20;

mod data;
mod hex;
mod host;
mod items;
mod seal;
mod seed;
mod session;
#[cfg(test)]
mod vectors;
pub use items::{candidate_ids, draw_items, set_fingerprint, Id, ItemSession, Pick};
pub use seed::{seed_digest, seed_from_text};
pub use seal::{open as open_sealed, seal as seal_state, token_key_id};
pub use session::{Session, StateError};
pub use data::{
    draw_data, DataError, SORTITION_FORMAT_JSON, SORTITION_FORMAT_TOML, SORTITION_FORMAT_YAML,
};

/// ライブラリのバージョン
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 抽選のアルゴリズムの版の名前（docs/SPEC.md）。seed の決め方、乱数、候補の ID と並べ方、引き方、賞の切り分けが、
/// 同じ入力に同じ結果を返す約束になっている。結果が変わる変更をするときは、この名前を新しくする。
/// 抽選の記録に入れておくと、あとで別の版のアルゴリズムで検証してしまうのを防げる。
pub const ALGORITHM: &str = "libsortition-draw/1";

/// 成功
pub const SORTITION_OK: i32 = 0;
/// 引数が不正（`k > n`、`out` が NULL など）
pub const SORTITION_ERR_INVALID: i32 = -1;
/// OS 乱数の取得に失敗
pub const SORTITION_ERR_RNG: i32 = -2;
/// 入力テキストが UTF-8 として不正
pub const SORTITION_ERR_UTF8: i32 = -3;
/// 構造化データの構文エラー（YAML のエイリアス・独自タグ・複数ドキュメントなども含む）
pub const SORTITION_ERR_PARSE: i32 = -4;
/// `path` が見つからない、またはその位置が配列でない
pub const SORTITION_ERR_PATH: i32 = -5;
/// 未知の形式、またはビルド時に無効化された形式
pub const SORTITION_ERR_FORMAT: i32 = -6;
/// 内部エラー（パニックを FFI の境界で捕捉した）
pub const SORTITION_ERR_INTERNAL: i32 = -7;

/// 保存したセッション状態が不正（形式・バージョン・中身の矛盾）
pub const SORTITION_ERR_STATE: i32 = -8;
/// 羃等キーが、別の内容のリクエスト（別の `k`）で既に使われている
pub const SORTITION_ERR_CONFLICT: i32 = -9;

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

fn set_last_error(msg: &str) {
    // 詳細メッセージに NUL が混ざっていても落ちないようにする
    let c = CString::new(msg.replace('\0', "?")).unwrap_or_default();
    LAST_ERROR.with(|e| *e.borrow_mut() = c);
}

/// xoshiro256** （シード指定時の再現可能な乱数）
#[derive(Clone)]
struct Xoshiro256 {
    s: [u64; 4],
}

impl Xoshiro256 {
    fn from_seed(seed: u64) -> Self {
        // splitmix64 で状態を展開
        let mut z = seed;
        let mut next = || {
            z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut x = z;
            x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            x ^ (x >> 31)
        };
        Self { s: [next(), next(), next(), next()] }
    }

    fn next_u64(&mut self) -> u64 {
        let r = self.s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        r
    }
}

/// ChaCha20 の鍵流を使う暗号論的乱数。状態は（鍵, 読み出し位置）だけなので保存・復元できる。
/// 鍵を知らない者は、出力をいくら見ても以降の出力を予測できない。
#[derive(Clone)]
struct ChaChaRng {
    key: [u8; 32],
    /// 鍵流の読み出し位置（バイト）。8 の倍数
    pos: u64,
}

/// IETF 版 ChaCha20 のカウンタは 32 ビット（64 バイトのブロック × 2^32 = 2^38 バイト）
const CHACHA_LIMIT: u64 = 1 << 38;

impl ChaChaRng {
    fn from_os() -> Result<Self, i32> {
        let mut key = [0u8; 32];
        getrandom::fill(&mut key).map_err(|_| SORTITION_ERR_RNG)?;
        Ok(Self { key, pos: 0 })
    }

    fn next_u64(&mut self) -> Result<u64, i32> {
        if self.pos > CHACHA_LIMIT - 8 {
            return Err(SORTITION_ERR_RNG);
        }
        let mut cipher = ChaCha20::new(&self.key.into(), &[0u8; 12].into());
        cipher.seek(self.pos);
        let mut buf = [0u8; 8];
        cipher.apply_keystream(&mut buf);
        self.pos += 8;
        Ok(u64::from_le_bytes(buf))
    }
}

#[derive(Clone)]
enum Source {
    /// 状態を持たず、呼ぶたびに OS から取る（一回きりの `draw` 用）
    Os,
    /// シード指定（再現できるが、暗号論的ではない）
    Seeded(Xoshiro256),
    /// シード指定なしのセッション用。OS から取った鍵で ChaCha20 を回し、状態に含めて保存する
    Secure(ChaChaRng),
}

impl Source {
    fn next_u64(&mut self) -> Result<u64, i32> {
        match self {
            Source::Seeded(r) => Ok(r.next_u64()),
            Source::Secure(r) => r.next_u64(),
            Source::Os => {
                let mut buf = [0u8; 8];
                getrandom::fill(&mut buf).map_err(|_| SORTITION_ERR_RNG)?;
                Ok(u64::from_le_bytes(buf))
            }
        }
    }

    /// `0..bound` の一様乱数（剰余バイアスを棄却法で除去）
    fn below(&mut self, bound: u64) -> Result<u64, i32> {
        debug_assert!(bound > 0);
        let zone = u64::MAX - (u64::MAX % bound + 1) % bound;
        loop {
            let v = self.next_u64()?;
            if v <= zone {
                return Ok(v % bound);
            }
        }
    }
}

/// `0..n` から `k` 個を重複なしで抽選する。メモリは O(k)。
pub fn draw(n: u32, k: u32, seed: Option<u64>) -> Result<Vec<u32>, i32> {
    if k > n {
        return Err(SORTITION_ERR_INVALID);
    }
    let mut src = match seed {
        Some(s) => Source::Seeded(Xoshiro256::from_seed(s)),
        None => Source::Os,
    };
    // 疎な Fisher-Yates: 配列を実体化せず、入れ替えた位置だけ覚える
    let mut swapped: HashMap<u32, u32> = HashMap::with_capacity(k as usize);
    let mut out = Vec::with_capacity(k as usize);
    for i in 0..k {
        let j = i + src.below((n - i) as u64)? as u32;
        let vj = *swapped.get(&j).unwrap_or(&j);
        let vi = *swapped.get(&i).unwrap_or(&i);
        swapped.insert(j, vi);
        out.push(vj);
    }
    Ok(out)
}

/// C ABI: `0..n` から `k` 個を抽選して `out[0..k]` に書く。
///
/// - `seed` が NULL なら OS 乱数、非 NULL ならその値でシード。
/// - `out` は `k` 個分の `uint32_t` 領域を指すこと。
/// - 戻り値は `SORTITION_OK` / `SORTITION_ERR_*`。
///
/// # Safety
/// `seed` は NULL か有効な `u64` へのポインタ、`out` は `k` 要素以上の書き込み可能領域であること。
#[no_mangle]
pub unsafe extern "C" fn sortition_draw(n: u32, k: u32, seed: *const u64, out: *mut u32) -> i32 {
    if out.is_null() && k > 0 {
        return SORTITION_ERR_INVALID;
    }
    let seed = if seed.is_null() { None } else { Some(*seed) };
    match draw(n, k, seed) {
        Ok(v) => {
            if k > 0 {
                std::ptr::copy_nonoverlapping(v.as_ptr(), out, v.len());
            }
            SORTITION_OK
        }
        Err(e) => e,
    }
}

/// テキスト（1 行 1 候補）から `k` 個を重複なしで抽選し、選ばれた候補を返す。
///
/// 各行は前後の空白を除き、空行は候補に含めない。同じ文字列の行は別の候補として数える。
pub fn draw_lines(text: &str, k: u32, seed: Option<u64>) -> Result<Vec<&str>, i32> {
    let items: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let n = u32::try_from(items.len()).map_err(|_| SORTITION_ERR_INVALID)?;
    Ok(draw(n, k, seed)?.into_iter().map(|i| items[i as usize]).collect())
}

/// C ABI: UTF-8 テキスト（1 行 1 候補）から `k` 個を抽選し、選ばれた候補を `\n` 区切りで `out` に書く。
///
/// - 戻り値は書いたバイト数（末尾の改行・NUL なし）。失敗時は `SORTITION_ERR_*`（負数）。
/// - 出力は入力より長くならないので、`out_cap >= len` の領域を渡せば足りる。
/// - 不正な UTF-8 は `SORTITION_ERR_UTF8`、`k` が候補数を超えると `SORTITION_ERR_INVALID`。
///
/// # Safety
/// `text` は `len` バイト読める領域、`out` は `out_cap` バイト書ける領域、
/// `seed` は NULL か有効な `u64` へのポインタであること。
#[no_mangle]
pub unsafe extern "C" fn sortition_draw_text(
    text: *const u8,
    len: usize,
    k: u32,
    seed: *const u64,
    out: *mut u8,
    out_cap: usize,
) -> i64 {
    if (text.is_null() && len > 0) || (out.is_null() && out_cap > 0) || out_cap < len {
        return SORTITION_ERR_INVALID as i64;
    }
    let bytes: &[u8] = if len == 0 { &[] } else { std::slice::from_raw_parts(text, len) };
    let Ok(text) = std::str::from_utf8(bytes) else {
        return SORTITION_ERR_UTF8 as i64;
    };
    let seed = if seed.is_null() { None } else { Some(*seed) };
    match draw_lines(text, k, seed) {
        Ok(items) => {
            let joined = items.join("\n");
            if joined.len() > out_cap {
                return SORTITION_ERR_INVALID as i64;
            }
            if !joined.is_empty() {
                std::ptr::copy_nonoverlapping(joined.as_ptr(), out, joined.len());
            }
            joined.len() as i64
        }
        Err(e) => e as i64,
    }
}

/// C ABI: 構造化データ（JSON / YAML / TOML）の配列から `k` 個を抽選し、
/// 選ばれた要素を JSON 配列の文字列として `*out` に返す。
///
/// - `format` は `SORTITION_FORMAT_*`。
/// - `path` は配列の場所（`a.b.0` のドット区切り、NUL 終端 UTF-8）。NULL か空ならルート。
///   TOML はルートが必ずテーブルなので指定が要る。
/// - 成功すると `*out` に NUL 終端の JSON 文字列を返す（`sortition_free` で解放）。失敗時は NULL。
/// - 戻り値は `SORTITION_OK` / `SORTITION_ERR_*`。失敗の詳細は `sortition_last_error` で取れる。
///
/// # Safety
/// `text` は `len` バイト読める領域、`path` は NULL か NUL 終端文字列、
/// `seed` は NULL か有効な `u64` へのポインタ、`out` は書き込み可能な `char*` 1 個分の領域であること。
#[no_mangle]
pub unsafe extern "C" fn sortition_draw_data(
    format: i32,
    text: *const u8,
    len: usize,
    path: *const c_char,
    k: u32,
    seed: *const u64,
    out: *mut *mut c_char,
) -> i32 {
    if out.is_null() {
        return SORTITION_ERR_INVALID;
    }
    *out = std::ptr::null_mut();
    if text.is_null() && len > 0 {
        set_last_error("text is NULL");
        return SORTITION_ERR_INVALID;
    }
    let bytes: &[u8] = if len == 0 { &[] } else { std::slice::from_raw_parts(text, len) };
    let path_bytes: &[u8] = if path.is_null() { b"" } else { CStr::from_ptr(path).to_bytes() };
    let seed = if seed.is_null() { None } else { Some(*seed) };

    // パーサーが panic しても、ホスト（PHP-FPM など）を巻き込んで落とさない
    let run = std::panic::catch_unwind(|| -> Result<String, DataError> {
        let text = std::str::from_utf8(bytes)
            .map_err(|_| DataError { code: SORTITION_ERR_UTF8, msg: "text is not valid UTF-8".into() })?;
        let path = std::str::from_utf8(path_bytes)
            .map_err(|_| DataError { code: SORTITION_ERR_UTF8, msg: "path is not valid UTF-8".into() })?;
        draw_data(format, text, path, k, seed)
    });
    match run {
        Ok(Ok(json)) => match CString::new(json) {
            Ok(c) => {
                *out = c.into_raw();
                SORTITION_OK
            }
            Err(_) => {
                set_last_error("internal error: NUL in output");
                SORTITION_ERR_INTERNAL
            }
        },
        Ok(Err(e)) => {
            set_last_error(&e.msg);
            e.code
        }
        Err(_) => {
            set_last_error("internal error: panic");
            SORTITION_ERR_INTERNAL
        }
    }
}

/// `sortition_draw_data` が返した文字列を解放する。NULL は無視する。
///
/// # Safety
/// `s` は `sortition_draw_data` が `*out` に返したポインタ（未解放）か NULL であること。
#[no_mangle]
pub unsafe extern "C" fn sortition_free(s: *mut c_char) {
    if !s.is_null() {
        drop(CString::from_raw(s));
    }
}

/// 同じスレッドで最後に失敗した `sortition_draw_data` の詳細メッセージ（NUL 終端 UTF-8）。
/// 次にこのスレッドで `sortition_draw_data` を呼ぶまで有効。解放不要。
#[no_mangle]
pub extern "C" fn sortition_last_error() -> *const c_char {
    LAST_ERROR.with(|e| e.borrow().as_ptr())
}

/// ライブラリのバージョン（NUL 終端、静的領域。解放不要）
#[no_mangle]
pub extern "C" fn sortition_version() -> *const std::os::raw::c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr().cast()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_and_in_range() {
        let v = draw(100, 100, None).unwrap();
        let mut s = v.clone();
        s.sort_unstable();
        assert_eq!(s, (0..100).collect::<Vec<_>>());
    }

    #[test]
    fn seed_is_reproducible() {
        assert_eq!(draw(1000, 10, Some(42)).unwrap(), draw(1000, 10, Some(42)).unwrap());
        assert_ne!(draw(1000, 10, Some(42)).unwrap(), draw(1000, 10, Some(43)).unwrap());
    }

    #[test]
    fn invalid_and_empty() {
        assert_eq!(draw(3, 4, None), Err(SORTITION_ERR_INVALID));
        assert_eq!(draw(0, 0, None).unwrap(), Vec::<u32>::new());
    }

    #[test]
    fn roughly_uniform() {
        let mut hits = [0u32; 10];
        for seed in 0..20_000u64 {
            hits[draw(10, 1, Some(seed)).unwrap()[0] as usize] += 1;
        }
        assert!(hits.iter().all(|&h| (1700..2300).contains(&h)), "{hits:?}");
    }

    #[test]
    fn lines_are_trimmed_and_blank_skipped() {
        let text = "  alice \r\n\r\nbob\n\t\ncarol\n";
        let mut v = draw_lines(text, 3, None).unwrap();
        v.sort_unstable();
        assert_eq!(v, ["alice", "bob", "carol"]);
        assert_eq!(draw_lines(text, 4, None), Err(SORTITION_ERR_INVALID));
        assert_eq!(draw_lines("", 0, None).unwrap(), Vec::<&str>::new());
    }

    #[test]
    fn lines_are_reproducible_and_keep_duplicates() {
        let text = "a\nb\nc\nd\ne";
        assert_eq!(draw_lines(text, 2, Some(7)), draw_lines(text, 2, Some(7)));
        assert_eq!(draw_lines("x\nx", 2, None).unwrap(), ["x", "x"]);
    }

    #[test]
    fn text_abi_roundtrip_and_errors() {
        let text = "あいう\nえお\nかきく";
        let mut out = vec![0u8; text.len()];
        let n = unsafe {
            sortition_draw_text(text.as_ptr(), text.len(), 3, std::ptr::null(), out.as_mut_ptr(), out.len())
        };
        assert!(n > 0 && n as usize <= text.len());
        let s = std::str::from_utf8(&out[..n as usize]).unwrap();
        let mut got: Vec<&str> = s.split('\n').collect();
        got.sort_unstable();
        assert_eq!(got, ["あいう", "えお", "かきく"]);

        let bad = [0xffu8, b'\n', b'a'];
        let r = unsafe { sortition_draw_text(bad.as_ptr(), bad.len(), 1, std::ptr::null(), out.as_mut_ptr(), out.len()) };
        assert_eq!(r, SORTITION_ERR_UTF8 as i64);
        let r = unsafe { sortition_draw_text(text.as_ptr(), text.len(), 1, std::ptr::null(), out.as_mut_ptr(), 1) };
        assert_eq!(r, SORTITION_ERR_INVALID as i64);
    }

    #[test]
    fn chacha_matches_known_keystream_and_seeks() {
        // RFC 8439 系の既知値: 鍵・ナンスとも 0 の ChaCha20 鍵流の先頭 8 バイト
        let mut r = ChaChaRng { key: [0; 32], pos: 0 };
        assert_eq!(r.next_u64().unwrap().to_le_bytes(), [0x76, 0xb8, 0xe0, 0xad, 0xa0, 0xf1, 0x3d, 0x90]);
        // 位置を指定して読んだ値は、先頭から順に読んだ値と一致する（ブロック境界をまたぐ）
        let mut seq = ChaChaRng { key: [7; 32], pos: 0 };
        let all: Vec<u64> = (0..40).map(|_| seq.next_u64().unwrap()).collect();
        for (i, want) in all.iter().enumerate() {
            let mut at = ChaChaRng { key: [7; 32], pos: 8 * i as u64 };
            assert_eq!(at.next_u64().unwrap(), *want);
        }
        let mut end = ChaChaRng { key: [7; 32], pos: CHACHA_LIMIT };
        assert_eq!(end.next_u64(), Err(SORTITION_ERR_RNG));
    }

    #[test]
    fn huge_n_small_k() {
        let v = draw(u32::MAX, 5, None).unwrap();
        assert_eq!(v.len(), 5);
    }
}
