//! 抽選セッション。
//!
//! `0..n` を母集団として、呼ぶたびに続きから非復元抽出する。途中経過は JSON に保存でき、
//! 復元すれば同じ続きから引ける（ホストが DB などに預けておく用途）。
//!
//! 乱数列の状態も保存するので、何回に分けて引いても、保存・復元をはさんでも、
//! 引く並びは変わらない（シード指定ありなら `draw(n, k1 + k2 + ..., seed)` と同じ並び）。
//! シード指定なしでは、作成時に OS から取った鍵で ChaCha20 を回し、その鍵と位置を状態に含める。
//! 同じ状態から引けば同じ結果になるので、古い状態を再送しても引き直しにならない。
//!
//! **`save()` の出力には乱数の内部状態が入る。** 持っている者は以降の当選を予測できるので、
//! 信頼できない相手には渡さないこと（封をして渡す場合は `seal` を使う）。

use std::collections::{HashMap, HashSet};
use std::ffi::c_char;
use std::ffi::CString;

use serde_json::{json, Value};

use crate::hex;
use crate::{
    set_last_error, ChaChaRng, Source, Xoshiro256, CHACHA_LIMIT, SORTITION_ERR_INTERNAL, SORTITION_ERR_INVALID,
    SORTITION_ERR_STATE, SORTITION_ERR_UTF8, SORTITION_OK,
};

const STATE_VERSION: u64 = 2;

/// 1 回分の `step` を元に戻すための記録
struct Undo {
    i: u32,
    j: u32,
    old_i: Option<u32>,
    old_j: Option<u32>,
}

fn restore(map: &mut HashMap<u32, u32>, key: u32, old: Option<u32>) {
    match old {
        Some(v) => map.insert(key, v),
        None => map.remove(&key),
    };
}

pub struct Session {
    n: u32,
    src: Source,
    /// 引いた順の結果
    drawn: Vec<u32>,
    /// 疎な Fisher-Yates の入れ替え表。キーは未抽選の位置（`drawn.len()..n`）だけ持つ
    swapped: HashMap<u32, u32>,
}

impl Session {
    /// シード指定なしなら OS から鍵を取る。取れなければ `SORTITION_ERR_RNG`。
    pub fn new(n: u32, seed: Option<u64>) -> Result<Self, i32> {
        let src = match seed {
            Some(s) => Source::Seeded(Xoshiro256::from_seed(s)),
            None => Source::Secure(ChaChaRng::from_os()?),
        };
        Ok(Self { n, src, drawn: Vec::new(), swapped: HashMap::new() })
    }

    /// 母集団の大きさ `n`
    pub fn population(&self) -> u32 {
        self.n
    }

    /// まだ引いていない数
    pub fn remaining(&self) -> u32 {
        self.n - self.drawn.len() as u32
    }

    /// ここまでに引いた結果（引いた順）
    pub fn drawn(&self) -> &[u32] {
        &self.drawn
    }

    /// 続きから `k` 個引く。`k` が残りを超えるなら何も消費せず `SORTITION_ERR_INVALID`。
    /// 途中で失敗した場合も状態は呼び出し前に戻る。
    pub fn draw(&mut self, k: u32) -> Result<Vec<u32>, i32> {
        if k > self.remaining() {
            return Err(SORTITION_ERR_INVALID);
        }
        let start = self.drawn.len();
        let mut undo: Vec<Undo> = Vec::with_capacity(k as usize);
        for _ in 0..k {
            if let Err(e) = self.step(&mut undo) {
                // 後ろから巻き戻す（OS 乱数の失敗でしか起きない）
                while let Some(u) = undo.pop() {
                    self.drawn.pop();
                    restore(&mut self.swapped, u.i, u.old_i);
                    restore(&mut self.swapped, u.j, u.old_j);
                }
                return Err(e);
            }
        }
        Ok(self.drawn[start..].to_vec())
    }

    fn step(&mut self, undo: &mut Vec<Undo>) -> Result<(), i32> {
        let i = self.drawn.len() as u32;
        let j = i + self.src.below((self.n - i) as u64)? as u32;
        let vj = *self.swapped.get(&j).unwrap_or(&j);
        let vi = *self.swapped.get(&i).unwrap_or(&i);
        let old_j = self.swapped.insert(j, vi);
        // 位置 i は確定したので、入れ替え表からは外す
        let old_i = self.swapped.remove(&i);
        self.drawn.push(vj);
        undo.push(Undo { i, j, old_i, old_j });
        Ok(())
    }

    /// 状態を JSON 文字列に保存する。
    ///
    /// 64 ビット値は JS で丸められないよう 10 進文字列にしている。
    pub fn save(&self) -> String {
        let rng = match &self.src {
            Source::Seeded(r) => json!({
                "kind": "xoshiro256",
                "s": r.s.iter().map(|v| v.to_string()).collect::<Vec<_>>(),
            }),
            Source::Secure(r) => json!({
                "kind": "chacha20",
                "key": hex::encode(&r.key),
                "pos": r.pos.to_string(),
            }),
            Source::Os => unreachable!("sessions never use the stateless OS source"),
        };
        let mut swapped: Vec<(u32, u32)> = self.swapped.iter().map(|(&k, &v)| (k, v)).collect();
        swapped.sort_unstable();
        json!({
            "version": STATE_VERSION,
            "n": self.n,
            "rng": rng,
            "drawn": self.drawn,
            "swapped": swapped,
        })
        .to_string()
    }

    /// `save` の出力から復元する。中身が矛盾していれば `SORTITION_ERR_STATE`。
    pub fn load(text: &str) -> Result<Self, StateError> {
        let bad = |m: &str| StateError(m.to_string());
        let v: Value = serde_json::from_str(text).map_err(|e| StateError(format!("invalid state: {e}")))?;
        let obj = v.as_object().ok_or_else(|| bad("state is not an object"))?;
        if obj.get("version").and_then(Value::as_u64) != Some(STATE_VERSION) {
            return Err(bad("unsupported state version"));
        }
        let to_u32 = |x: &Value| x.as_u64().and_then(|x| u32::try_from(x).ok());
        let n = obj.get("n").and_then(to_u32).ok_or_else(|| bad("invalid n"))?;

        let rng = obj.get("rng").and_then(Value::as_object).ok_or_else(|| bad("invalid rng"))?;
        let src = match rng.get("kind").and_then(Value::as_str) {
            Some("xoshiro256") => {
                let a = rng.get("s").and_then(Value::as_array).filter(|a| a.len() == 4);
                let a = a.ok_or_else(|| bad("invalid rng"))?;
                let mut s = [0u64; 4];
                for (slot, x) in s.iter_mut().zip(a) {
                    *slot = x.as_str().and_then(|x| x.parse().ok()).ok_or_else(|| bad("invalid rng"))?;
                }
                if s == [0; 4] {
                    return Err(bad("invalid rng"));
                }
                Source::Seeded(Xoshiro256 { s })
            }
            Some("chacha20") => {
                let key = rng.get("key").and_then(Value::as_str).and_then(hex::decode::<32>);
                let pos = rng.get("pos").and_then(Value::as_str).and_then(|p| p.parse::<u64>().ok());
                match (key, pos) {
                    (Some(key), Some(pos)) if pos % 8 == 0 && pos <= CHACHA_LIMIT => {
                        Source::Secure(ChaChaRng { key, pos })
                    }
                    _ => return Err(bad("invalid rng")),
                }
            }
            _ => return Err(bad("invalid rng")),
        };

        let drawn: Vec<u32> = obj
            .get("drawn")
            .and_then(Value::as_array)
            .ok_or_else(|| bad("invalid drawn"))?
            .iter()
            .map(|x| to_u32(x).filter(|&x| x < n))
            .collect::<Option<_>>()
            .ok_or_else(|| bad("invalid drawn"))?;
        if drawn.len() > n as usize {
            return Err(bad("invalid drawn"));
        }
        let d = drawn.len() as u32;

        let mut swapped = HashMap::new();
        for pair in obj.get("swapped").and_then(Value::as_array).ok_or_else(|| bad("invalid swapped"))? {
            let p = pair.as_array().filter(|p| p.len() == 2).ok_or_else(|| bad("invalid swapped"))?;
            let (Some(j), Some(vj)) = (to_u32(&p[0]), to_u32(&p[1])) else {
                return Err(bad("invalid swapped"));
            };
            if j < d || j >= n || vj >= n || swapped.insert(j, vj).is_some() {
                return Err(bad("invalid swapped"));
            }
        }

        // 触った位置 {0..d} ∪ キー と、そこに入っている値 drawn ∪ 値 は同じ集合のはず
        // （入れ替えは触った位置の中で閉じている）。重複なし・包含・個数一致で確かめる。
        let mut elems = HashSet::with_capacity(drawn.len() + swapped.len());
        for &e in drawn.iter().chain(swapped.values()) {
            if !elems.insert(e) {
                return Err(bad("state is inconsistent"));
            }
        }
        if elems.len() != drawn.len() + swapped.len() || !elems.iter().all(|&e| e < d || swapped.contains_key(&e)) {
            return Err(bad("state is inconsistent"));
        }

        Ok(Self { n, src, drawn, swapped })
    }
}

#[derive(Debug)]
pub struct StateError(pub String);

// ---- C ABI ----

/// C ABI: 新しいセッションを作る。`seed` が NULL なら OS 乱数。`*out` は `sortition_session_free` で解放。
///
/// # Safety
/// `seed` は NULL か有効な `u64` へのポインタ、`out` は書き込み可能なポインタ 1 個分の領域であること。
#[no_mangle]
pub unsafe extern "C" fn sortition_session_new(n: u32, seed: *const u64, out: *mut *mut Session) -> i32 {
    if out.is_null() {
        return SORTITION_ERR_INVALID;
    }
    let seed = if seed.is_null() { None } else { Some(*seed) };
    match Session::new(n, seed) {
        Ok(s) => {
            *out = Box::into_raw(Box::new(s));
            SORTITION_OK
        }
        Err(e) => e,
    }
}

/// C ABI: 続きから `k` 個引いて `out[0..k]` に書く。`k` が残りを超えるなら `SORTITION_ERR_INVALID`（状態は変わらない）。
///
/// # Safety
/// `s` は有効なセッション、`out` は `k` 要素以上の書き込み可能領域であること。
/// 同じセッションを複数スレッドから同時に使ってはいけない。
#[no_mangle]
pub unsafe extern "C" fn sortition_session_draw(s: *mut Session, k: u32, out: *mut u32) -> i32 {
    if s.is_null() || (out.is_null() && k > 0) {
        return SORTITION_ERR_INVALID;
    }
    match (*s).draw(k) {
        Ok(v) => {
            if k > 0 {
                std::ptr::copy_nonoverlapping(v.as_ptr(), out, v.len());
            }
            SORTITION_OK
        }
        Err(e) => e,
    }
}

/// C ABI: まだ引いていない数。`s` が NULL なら 0。
///
/// # Safety
/// `s` は NULL か有効なセッションであること。
#[no_mangle]
pub unsafe extern "C" fn sortition_session_remaining(s: *const Session) -> u32 {
    if s.is_null() {
        0
    } else {
        (*s).remaining()
    }
}

/// C ABI: ここまでに引いた数。`s` が NULL なら 0。
///
/// # Safety
/// `s` は NULL か有効なセッションであること。
#[no_mangle]
pub unsafe extern "C" fn sortition_session_drawn_count(s: *const Session) -> u32 {
    if s.is_null() {
        0
    } else {
        (*s).drawn().len() as u32
    }
}

/// C ABI: 状態を JSON 文字列に保存して `*out` に返す（`sortition_free` で解放）。
///
/// # Safety
/// `s` は有効なセッション、`out` は書き込み可能な `char*` 1 個分の領域であること。
#[no_mangle]
pub unsafe extern "C" fn sortition_session_save(s: *const Session, out: *mut *mut c_char) -> i32 {
    if out.is_null() {
        return SORTITION_ERR_INVALID;
    }
    *out = std::ptr::null_mut();
    if s.is_null() {
        return SORTITION_ERR_INVALID;
    }
    match CString::new((*s).save()) {
        Ok(c) => {
            *out = c.into_raw();
            SORTITION_OK
        }
        Err(_) => {
            set_last_error("internal error: NUL in output");
            SORTITION_ERR_INTERNAL
        }
    }
}

/// C ABI: `sortition_session_save` の出力からセッションを復元する。
/// 不正・矛盾した状態は `SORTITION_ERR_STATE`（詳細は `sortition_last_error`）。
///
/// # Safety
/// `text` は `len` バイト読める領域、`out` は書き込み可能なポインタ 1 個分の領域であること。
#[no_mangle]
pub unsafe extern "C" fn sortition_session_load(text: *const u8, len: usize, out: *mut *mut Session) -> i32 {
    if out.is_null() {
        return SORTITION_ERR_INVALID;
    }
    *out = std::ptr::null_mut();
    if text.is_null() && len > 0 {
        set_last_error("text is NULL");
        return SORTITION_ERR_INVALID;
    }
    let bytes: &[u8] = if len == 0 { &[] } else { std::slice::from_raw_parts(text, len) };
    let Ok(text) = std::str::from_utf8(bytes) else {
        set_last_error("state is not valid UTF-8");
        return SORTITION_ERR_UTF8;
    };
    match std::panic::catch_unwind(|| Session::load(text)) {
        Ok(Ok(s)) => {
            *out = Box::into_raw(Box::new(s));
            SORTITION_OK
        }
        Ok(Err(e)) => {
            set_last_error(&e.0);
            SORTITION_ERR_STATE
        }
        Err(_) => {
            set_last_error("internal error: panic");
            SORTITION_ERR_INTERNAL
        }
    }
}

/// C ABI: セッションを解放する。NULL は無視する。
///
/// # Safety
/// `s` は `sortition_session_new` / `sortition_session_load` が返した未解放のポインタか NULL であること。
#[no_mangle]
pub unsafe extern "C" fn sortition_session_free(s: *mut Session) {
    if !s.is_null() {
        drop(Box::from_raw(s));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw;

    #[test]
    fn split_draws_equal_one_draw_with_seed() {
        let mut s = Session::new(1000, Some(42)).unwrap();
        let mut got = s.draw(3).unwrap();
        got.extend(s.draw(0).unwrap());
        got.extend(s.draw(5).unwrap());
        got.extend(s.draw(2).unwrap());
        assert_eq!(got, draw(1000, 10, Some(42)).unwrap());
        assert_eq!(s.drawn(), &got[..]);
        assert_eq!(s.remaining(), 990);
    }

    #[test]
    fn exhausts_without_repeats() {
        let mut s = Session::new(50, None).unwrap();
        let mut all = Vec::new();
        while s.remaining() > 0 {
            all.extend(s.draw(7.min(s.remaining())).unwrap());
        }
        all.sort_unstable();
        assert_eq!(all, (0..50).collect::<Vec<_>>());
        assert_eq!(s.draw(1), Err(SORTITION_ERR_INVALID));
    }

    #[test]
    fn over_draw_leaves_state_untouched() {
        let mut s = Session::new(5, Some(1)).unwrap();
        s.draw(3).unwrap();
        let before = s.save();
        assert_eq!(s.draw(3), Err(SORTITION_ERR_INVALID));
        assert_eq!(s.save(), before);
    }

    #[test]
    fn save_load_resumes_seeded() {
        let mut a = Session::new(200, Some(9)).unwrap();
        let mut got = a.draw(20).unwrap();
        let mut b = Session::load(&a.save()).unwrap();
        got.extend(b.draw(30).unwrap());
        assert_eq!(got, draw(200, 50, Some(9)).unwrap());
        assert_eq!(b.save(), {
            let mut c = Session::new(200, Some(9)).unwrap();
            c.draw(50).unwrap();
            c.save()
        });
    }

    #[test]
    fn save_load_resumes_os_without_repeats() {
        let mut a = Session::new(100, None).unwrap();
        let mut all = a.draw(40).unwrap();
        let mut b = Session::load(&a.save()).unwrap();
        all.extend(b.draw(60).unwrap());
        all.sort_unstable();
        assert_eq!(all, (0..100).collect::<Vec<_>>());
    }

    #[test]
    fn huge_n_state_stays_small() {
        let mut s = Session::new(u32::MAX, None).unwrap();
        s.draw(100).unwrap();
        let mut t = Session::load(&s.save()).unwrap();
        assert!(!t.draw(100).unwrap().iter().any(|x| s.drawn().contains(x)));
    }

    #[test]
    fn rejects_tampered_state() {
        let mut s = Session::new(10, Some(3)).unwrap();
        s.draw(4).unwrap();
        let good: Value = serde_json::from_str(&s.save()).unwrap();

        let mutate = |f: &dyn Fn(&mut Value)| {
            let mut v = good.clone();
            f(&mut v);
            Session::load(&v.to_string()).err()
        };
        assert!(Session::load(&good.to_string()).is_ok());
        assert!(mutate(&|v| v["version"] = json!(1)).is_some());
        assert!(mutate(&|v| v["version"] = json!(3)).is_some());
        assert!(mutate(&|v| v["n"] = json!(3)).is_some());
        assert!(mutate(&|v| v["drawn"][0] = v["drawn"][1].clone()).is_some());
        assert!(mutate(&|v| v["drawn"][0] = json!(10)).is_some());
        assert!(mutate(&|v| v["swapped"] = json!([])).is_some());
        assert!(mutate(&|v| v["rng"] = json!(null)).is_some());
        assert!(mutate(&|v| v["rng"] = json!(["1", "2", "3", "4"])).is_some()); // 旧形式
        assert!(mutate(&|v| v["rng"]["kind"] = json!("mt19937")).is_some());
        assert!(mutate(&|v| v["rng"]["s"] = json!(["0", "0", "0", "0"])).is_some());
        assert!(mutate(&|v| v["rng"]["s"] = json!([1, 2, 3, 4])).is_some());
        assert!(Session::load("not json").is_err());
        assert!(Session::load("[]").is_err());
    }

    #[test]
    fn unseeded_state_is_chacha_and_resume_is_deterministic() {
        let mut a = Session::new(100, None).unwrap();
        a.draw(10).unwrap();
        let state: Value = serde_json::from_str(&a.save()).unwrap();
        assert_eq!(state["rng"]["kind"], "chacha20");
        assert_eq!(state["rng"]["pos"], "80"); // 8 バイト × 10 回（棄却がなければ）

        // 同じ状態から引けば何度でも同じ結果になる（古い状態を再送しても引き直しにならない）
        let saved = a.save();
        let first = a.draw(5).unwrap();
        for _ in 0..3 {
            let mut b = Session::load(&saved).unwrap();
            assert_eq!(b.draw(5).unwrap(), first);
        }
        // 別のセッションは別の鍵を持つ
        let mut c = Session::new(100, None).unwrap();
        let mut d = Session::new(100, None).unwrap();
        assert_ne!(c.draw(10).unwrap(), d.draw(10).unwrap());
    }

    #[test]
    fn rejects_bad_chacha_state() {
        let a = Session::new(10, None).unwrap();
        let good: Value = serde_json::from_str(&a.save()).unwrap();
        let mutate = |f: &dyn Fn(&mut Value)| {
            let mut v = good.clone();
            f(&mut v);
            Session::load(&v.to_string()).is_err()
        };
        assert!(!mutate(&|_| {}));
        assert!(mutate(&|v| v["rng"]["key"] = json!("abcd")));
        assert!(mutate(&|v| v["rng"]["key"] = json!("zz".repeat(32))));
        assert!(mutate(&|v| v["rng"]["pos"] = json!("7"))); // 8 の倍数でない
        assert!(mutate(&|v| v["rng"]["pos"] = json!((CHACHA_LIMIT + 8).to_string())));
        assert!(mutate(&|v| v["rng"]["pos"] = json!(8))); // 文字列でない
    }

    #[test]
    fn c_abi_roundtrip() {
        unsafe {
            let seed = 5u64;
            let mut s: *mut Session = std::ptr::null_mut();
            assert_eq!(sortition_session_new(20, &seed, &mut s), SORTITION_OK);
            let mut buf = [0u32; 8];
            assert_eq!(sortition_session_draw(s, 5, buf.as_mut_ptr()), SORTITION_OK);
            assert_eq!(sortition_session_remaining(s), 15);
            assert_eq!(sortition_session_drawn_count(s), 5);

            let mut json: *mut c_char = std::ptr::null_mut();
            assert_eq!(sortition_session_save(s, &mut json), SORTITION_OK);
            let text = std::ffi::CStr::from_ptr(json).to_bytes().to_vec();
            crate::sortition_free(json);
            sortition_session_free(s);

            let mut t: *mut Session = std::ptr::null_mut();
            assert_eq!(sortition_session_load(text.as_ptr(), text.len(), &mut t), SORTITION_OK);
            let mut rest = [0u32; 8];
            assert_eq!(sortition_session_draw(t, 8, rest.as_mut_ptr()), SORTITION_OK);
            assert_eq!(sortition_session_draw(t, 8, rest.as_mut_ptr()), SORTITION_ERR_INVALID);
            let mut all = buf[..5].to_vec();
            all.extend(rest);
            assert_eq!(all, draw(20, 13, Some(5)).unwrap());
            sortition_session_free(t);

            let mut bad: *mut Session = std::ptr::null_mut();
            assert_eq!(sortition_session_load(b"{}".as_ptr(), 2, &mut bad), SORTITION_ERR_STATE);
            assert!(bad.is_null());
            sortition_session_free(std::ptr::null_mut());
        }
    }
}
