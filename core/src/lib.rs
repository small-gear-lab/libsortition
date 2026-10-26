//! Fair, repeatable and verifiable random lotteries: draw `k` of `n` candidates without replacement.
//!
//! The same candidates and the same seed give the same winners, in every language libsortition has
//! bindings for and in its `sortition` command. [`docs/SPEC.md`](https://github.com/small-gear-lab/libsortition/blob/main/docs/SPEC.md)
//! writes the algorithm down exactly, with test vectors and a reference implementation.
//!
//! # Drawing candidates
//!
//! A candidate is any byte string. Each one gets an ID that does not depend on its position, and the
//! draw works on the IDs in sorted order, so **the winners do not depend on the order of your list**.
//! A seed string gives a seed (and the commitment you can publish before the draw: its SHA-256).
//!
//! ```
//! use sortition::{draw_items, seed_from_text};
//!
//! let people: Vec<&[u8]> = vec![b"alice", b"bob", b"carol", b"dave"];
//! let seed = seed_from_text("my seed");
//! let picks = draw_items(&people, 2, Some(seed)).unwrap();
//!
//! let winners: Vec<&[u8]> = picks.iter().map(|p| people[p.index as usize]).collect();
//! assert_eq!(winners, [b"dave" as &[u8], b"bob"]);
//! ```
//!
//! [`candidate_ids`] and [`set_fingerprint`] give the IDs and the fingerprint of a set of candidates;
//! publish the fingerprint with the commitment, and anyone can check afterwards that the candidates
//! were the same.
//!
//! # Sessions
//!
//! [`Session`] and [`ItemSession`] draw in steps and continue where they stopped. Their state can be
//! saved as JSON and restored, a retried request with an idempotency key never draws twice, and
//! [`Session::seal`] turns a state into a token that a party you do not trust can keep without being
//! able to read or change it.
//!
//! ```
//! use sortition::Session;
//!
//! let mut session = Session::new(100, Some(1)).unwrap();
//! let first = session.draw(3).unwrap();
//!
//! // Save it, and continue later (even in another process): the draw goes on as if never interrupted.
//! let mut restored = Session::load(&session.save()).unwrap();
//! let next = restored.draw(2).unwrap();
//! assert_eq!([first, next].concat(), sortition::draw(100, 5, Some(1)).unwrap());
//! ```
//!
//! # Two ways to draw
//!
//! [`draw`], [`draw_lines`] and [`draw_data`] return **positions** in the list as you gave it, so a
//! reordered list gives different winners. Use the candidate functions above when that matters.
//!
//! The `yaml` and `toml` features (on by default) add drawing from YAML and TOML data. The functions
//! marked `extern "C"` are the C ABI; it is documented in `sortition.h`.

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
pub use seal::Key;
pub use session::{Drawn, Session, StateError, MAX_KEY_LEN};
pub use data::{
    draw_data, DataError, SORTITION_FORMAT_JSON, SORTITION_FORMAT_TOML, SORTITION_FORMAT_YAML,
};

/// The version of the library.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The name of the version of the draw algorithm (see `docs/SPEC.md`). The seed rule, the random streams,
/// candidate IDs and their order, the way numbers are drawn and the way stages are cut promise to give the
/// same result for the same input. A change that could give a different result gets a new name.
/// Put it in the record of a draw, so that the record is never checked with the wrong algorithm later.
pub const ALGORITHM: &str = "libsortition-draw/1";

/// Success.
pub const SORTITION_OK: i32 = 0;
/// An invalid argument (`k > n`, a NULL `out`, ...).
pub const SORTITION_ERR_INVALID: i32 = -1;
/// The operating system's randomness could not be read.
pub const SORTITION_ERR_RNG: i32 = -2;
/// The input text is not valid UTF-8.
pub const SORTITION_ERR_UTF8: i32 = -3;
/// A syntax error in structured data (this includes YAML aliases, custom tags and multiple documents).
pub const SORTITION_ERR_PARSE: i32 = -4;
/// The `path` was not found, or what it points to is not an array.
pub const SORTITION_ERR_PATH: i32 = -5;
/// An unknown format, or one that is disabled in this build.
pub const SORTITION_ERR_FORMAT: i32 = -6;
/// An internal error (a panic was caught at the FFI boundary).
pub const SORTITION_ERR_INTERNAL: i32 = -7;

/// An invalid saved session state or sealed token (the format, the version or an inconsistency).
pub const SORTITION_ERR_STATE: i32 = -8;
/// The idempotency key was already used with a different request (another `k`).
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

/// Draws `k` distinct numbers from `0..n`, in the order drawn. It needs O(k) memory.
///
/// With a seed the result is repeatable (xoshiro256\*\*); without one the operating system's randomness is used.
/// The numbers are **positions**: a reordered list of candidates gives different winners.
/// Fails with [`SORTITION_ERR_INVALID`] when `k > n`.
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
#[doc(hidden)]
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

/// Draws `k` lines from text (one candidate per line) without replacement and returns the chosen lines.
///
/// Each line is trimmed of surrounding white space, and empty lines are not candidates. Identical lines
/// are separate candidates. Like [`draw`], this works on positions in the text.
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
#[doc(hidden)]
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
#[doc(hidden)]
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
#[doc(hidden)]
#[no_mangle]
pub unsafe extern "C" fn sortition_free(s: *mut c_char) {
    if !s.is_null() {
        drop(CString::from_raw(s));
    }
}

/// 同じスレッドで最後に失敗した `sortition_draw_data` の詳細メッセージ（NUL 終端 UTF-8）。
/// 次にこのスレッドで `sortition_draw_data` を呼ぶまで有効。解放不要。
#[doc(hidden)]
#[no_mangle]
pub extern "C" fn sortition_last_error() -> *const c_char {
    LAST_ERROR.with(|e| e.borrow().as_ptr())
}

/// ライブラリのバージョン（NUL 終端、静的領域。解放不要）
#[doc(hidden)]
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
