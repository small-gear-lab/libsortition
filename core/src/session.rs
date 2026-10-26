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
    set_last_error, ChaChaRng, Source, Xoshiro256, CHACHA_LIMIT, SORTITION_ERR_CONFLICT, SORTITION_ERR_RNG, SORTITION_ERR_INTERNAL, SORTITION_ERR_INVALID,
    SORTITION_ERR_STATE, SORTITION_ERR_UTF8, SORTITION_OK,
};

const STATE_VERSION: u64 = 3;

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

/// 羃等キーの長さの上限（バイト）
pub const MAX_KEY_LEN: usize = 128;

/// 羃等キー付きで引いた 1 回分の記録。`drawn[start..start + count]` がそのときの結果。
struct Call {
    key: String,
    start: u32,
    count: u32,
}

/// `draw_keyed` の結果
#[derive(Debug, PartialEq, Eq)]
pub struct Drawn {
    pub picks: Vec<u32>,
    /// 同じキーの前回の結果をそのまま返した（状態は変わっていない）
    pub replayed: bool,
}

pub struct Session {
    /// セッションの識別子（作成時に OS から取った 16 バイト）
    id: [u8; 16],
    /// 状態を変えた `draw` の回数（`k > 0` で成功した呼び出し）。保存側の比較更新に使える
    revision: u64,
    /// 羃等キー付きの呼び出しの記録（引いた順）
    calls: Vec<Call>,
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
        let mut id = [0u8; 16];
        getrandom::fill(&mut id).map_err(|_| SORTITION_ERR_RNG)?;
        Ok(Self { id, revision: 0, calls: Vec::new(), n, src, drawn: Vec::new(), swapped: HashMap::new() })
    }

    /// セッションの識別子（小文字 16 進 32 文字）
    pub fn id(&self) -> String {
        hex::encode(&self.id)
    }

    /// 状態を変えた `draw` の回数。保存側は「読んだ revision のときだけ更新する」ことで、
    /// 同じ状態から別々に進めた更新の上書きを防げる。
    pub fn revision(&self) -> u64 {
        self.revision
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
        Ok(self.draw_keyed(k, None)?.picks)
    }

    /// 羃等キー付きで続きから `k` 個引く。
    ///
    /// 同じ `key` と同じ `k` の再送は、状態を変えずに前回の結果をそのまま返す（`replayed`）。
    /// 同じ `key` で別の `k` なら `SORTITION_ERR_CONFLICT`。キーは 1〜128 バイト。
    /// `k == 0` は何もせず、キーも記録しない。失敗した呼び出しは何も残さない。
    pub fn draw_keyed(&mut self, k: u32, key: Option<&str>) -> Result<Drawn, i32> {
        if let Some(key) = key {
            if key.is_empty() || key.len() > MAX_KEY_LEN {
                return Err(SORTITION_ERR_INVALID);
            }
            if let Some(c) = self.calls.iter().find(|c| c.key == key) {
                if c.count != k {
                    return Err(SORTITION_ERR_CONFLICT);
                }
                let picks = self.drawn[c.start as usize..(c.start + c.count) as usize].to_vec();
                return Ok(Drawn { picks, replayed: true });
            }
        }
        if k == 0 {
            return Ok(Drawn { picks: Vec::new(), replayed: false });
        }
        let start = self.drawn.len() as u32;
        let picks = self.draw_raw(k)?;
        self.revision += 1;
        if let Some(key) = key {
            self.calls.push(Call { key: key.to_string(), start, count: k });
        }
        Ok(Drawn { picks, replayed: false })
    }

    fn draw_raw(&mut self, k: u32) -> Result<Vec<u32>, i32> {
        if k > self.remaining() {
            return Err(SORTITION_ERR_INVALID);
        }
        let start = self.drawn.len();
        let rng_before = self.src.clone();
        let mut undo: Vec<Undo> = Vec::with_capacity(k as usize);
        for _ in 0..k {
            if let Err(e) = self.step(&mut undo) {
                // 後ろから巻き戻す（乱数の失敗でしか起きない）。乱数の位置も戻す
                while let Some(u) = undo.pop() {
                    self.drawn.pop();
                    restore(&mut self.swapped, u.i, u.old_i);
                    restore(&mut self.swapped, u.j, u.old_j);
                }
                self.src = rng_before;
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
        let calls: Vec<(&str, u32, u32)> = self.calls.iter().map(|c| (c.key.as_str(), c.start, c.count)).collect();
        json!({
            "version": STATE_VERSION,
            "id": hex::encode(&self.id),
            "revision": self.revision,
            "calls": calls,
            "n": self.n,
            "rng": rng,
            "drawn": self.drawn,
            "swapped": swapped,
        })
        .to_string()
    }

    /// 状態に封をしてトークンにする（`save()` を `seal` で包む）。形式は `seal` のモジュールを参照。
    /// 信頼できない相手（ブラウザなど）に預けるときは、`save()` ではなくこちらを使う。
    pub fn seal(&self, key: &crate::seal::Key, key_id: u8) -> Result<String, i32> {
        crate::seal::seal(self.save().as_bytes(), key, key_id)
    }

    /// `seal` のトークンから復元する。形式の不正・鍵違い・改ざん・中身の矛盾は `StateError`。
    pub fn unseal(token: &str, key: &crate::seal::Key) -> Result<Self, StateError> {
        let plain = crate::seal::open(token, key)?;
        let text = String::from_utf8(plain).map_err(|_| StateError("sealed state is not UTF-8".to_string()))?;
        Self::load(&text)
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
        let id = obj.get("id").and_then(Value::as_str).and_then(hex::decode::<16>).ok_or_else(|| bad("invalid id"))?;
        let revision = obj.get("revision").and_then(Value::as_u64).ok_or_else(|| bad("invalid revision"))?;
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

        // 羃等キーの記録: 引いた順に重なりなく並び、キーは重複しない。
        // 各回は 1 個以上引くので、回数（revision）は引いた数を超えず、記録の数は revision 以下
        let mut calls: Vec<Call> = Vec::new();
        let mut end = 0u32;
        for c in obj.get("calls").and_then(Value::as_array).ok_or_else(|| bad("invalid calls"))? {
            let t = c.as_array().filter(|t| t.len() == 3).ok_or_else(|| bad("invalid calls"))?;
            let (Some(key), Some(start), Some(count)) = (t[0].as_str(), to_u32(&t[1]), to_u32(&t[2])) else {
                return Err(bad("invalid calls"));
            };
            let stop = start.checked_add(count);
            if key.is_empty()
                || key.len() > MAX_KEY_LEN
                || count == 0
                || start < end
                || stop.map_or(true, |s| s > d)
                || calls.iter().any(|p| p.key == key)
            {
                return Err(bad("invalid calls"));
            }
            end = start + count;
            calls.push(Call { key: key.to_string(), start, count });
        }
        if revision > drawn.len() as u64 || (calls.len() as u64) > revision || (revision == 0) != drawn.is_empty() {
            return Err(bad("invalid revision"));
        }

        Ok(Self { id, revision, calls, n, src, drawn, swapped })
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

/// 羃等キーの引数（NUL 終端、NULL ならキーなし）を取り出す。UTF-8 でなければ `SORTITION_ERR_UTF8`。
///
/// # Safety
/// `key` は NULL か NUL 終端の文字列であること。
pub(crate) unsafe fn key_arg<'a>(key: *const c_char) -> Result<Option<&'a str>, i32> {
    if key.is_null() {
        return Ok(None);
    }
    std::ffi::CStr::from_ptr(key).to_str().map(Some).map_err(|_| SORTITION_ERR_UTF8)
}

/// C ABI: 羃等キー付きで続きから `k` 個引いて `out[0..k]` に書く。
///
/// - `key` は NUL 終端 UTF-8（1〜128 バイト）。NULL ならキーなし（`sortition_session_draw` と同じ）。
/// - 同じ `key` と同じ `k` の再送は、状態を変えずに前回の結果を書き、`*replayed` を 1 にする。
///   同じ `key` で別の `k` は `SORTITION_ERR_CONFLICT`。
/// - `replayed` は NULL でもよい。
///
/// # Safety
/// `s` は有効なセッション、`key` は NULL か NUL 終端文字列、`out` は `k` 要素以上の書き込み可能領域、
/// `replayed` は NULL か書き込み可能な `int32_t` であること。
#[no_mangle]
pub unsafe extern "C" fn sortition_session_draw_keyed(
    s: *mut Session,
    k: u32,
    key: *const c_char,
    out: *mut u32,
    replayed: *mut i32,
) -> i32 {
    if s.is_null() || (out.is_null() && k > 0) {
        return SORTITION_ERR_INVALID;
    }
    if !replayed.is_null() {
        *replayed = 0;
    }
    let key = match key_arg(key) {
        Ok(k) => k,
        Err(e) => return e,
    };
    match (*s).draw_keyed(k, key) {
        Ok(d) => {
            if k > 0 {
                std::ptr::copy_nonoverlapping(d.picks.as_ptr(), out, d.picks.len());
            }
            if !replayed.is_null() {
                *replayed = d.replayed as i32;
            }
            SORTITION_OK
        }
        Err(e) => e,
    }
}

/// C ABI: 状態を変えた `draw` の回数。保存側が「読んだ revision のときだけ更新する」ために使える。`s` が NULL なら 0。
///
/// # Safety
/// `s` は NULL か有効なセッションであること。
#[no_mangle]
pub unsafe extern "C" fn sortition_session_revision(s: *const Session) -> u64 {
    if s.is_null() {
        0
    } else {
        (*s).revision()
    }
}

/// C ABI: セッションの識別子（小文字 16 進 32 文字 + NUL）を `out[0..33]` に書く。
///
/// # Safety
/// `s` は有効なセッション、`out` は 33 バイト書ける領域であること。
#[no_mangle]
pub unsafe extern "C" fn sortition_session_id(s: *const Session, out: *mut u8) -> i32 {
    if s.is_null() || out.is_null() {
        return SORTITION_ERR_INVALID;
    }
    write_id((*s).id(), out);
    SORTITION_OK
}

pub(crate) unsafe fn write_id(id: String, out: *mut u8) {
    std::ptr::copy_nonoverlapping(id.as_ptr(), out, id.len());
    *out.add(id.len()) = 0;
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

/// C ABI: 状態に封をしたトークン（NUL 終端の ASCII）を `*out` に返す（`sortition_free` で解放）。
/// 乱数の内部状態を相手に見せずに、状態を預けられる。
///
/// - `key` は 32 バイトの秘密鍵（暗号論的乱数で作る）。`key_id` は鍵の入れ替え用の目印（0〜255）。
///
/// # Safety
/// `s` は有効なセッション、`key` は 32 バイト読める領域、`out` は書き込み可能な `char*` 1 個分の領域であること。
#[no_mangle]
pub unsafe extern "C" fn sortition_session_seal(s: *const Session, key: *const u8, key_id: u8, out: *mut *mut c_char) -> i32 {
    if out.is_null() {
        return SORTITION_ERR_INVALID;
    }
    *out = std::ptr::null_mut();
    let (Some(key), false) = (crate::seal::key_ref(key), s.is_null()) else {
        return SORTITION_ERR_INVALID;
    };
    match (*s).seal(key, key_id).map(CString::new) {
        Ok(Ok(c)) => {
            *out = c.into_raw();
            SORTITION_OK
        }
        Ok(Err(_)) => {
            set_last_error("internal error: NUL in output");
            SORTITION_ERR_INTERNAL
        }
        Err(e) => e,
    }
}

/// C ABI: `sortition_session_seal` のトークンから復元する。
/// 形式の不正・鍵違い・改ざん・中身の矛盾は `SORTITION_ERR_STATE`（詳細は `sortition_last_error`）。
///
/// # Safety
/// `token` は `len` バイト読める領域、`key` は 32 バイト読める領域、`out` は書き込み可能なポインタ 1 個分の領域であること。
#[no_mangle]
pub unsafe extern "C" fn sortition_session_unseal(token: *const u8, len: usize, key: *const u8, out: *mut *mut Session) -> i32 {
    if out.is_null() {
        return SORTITION_ERR_INVALID;
    }
    *out = std::ptr::null_mut();
    let Some(key) = crate::seal::key_ref(key) else {
        return SORTITION_ERR_INVALID;
    };
    let token = match crate::seal::token_arg(token, len) {
        Ok(t) => t,
        Err(e) => return e,
    };
    match std::panic::catch_unwind(|| Session::unseal(token, key)) {
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
        // 一括で引いた状態と、id と revision（呼び出しの回数）以外は一致する
        let strip = |text: String| {
            let mut v: Value = serde_json::from_str(&text).unwrap();
            v.as_object_mut().unwrap().retain(|k, _| k != "id" && k != "revision");
            v
        };
        let mut c = Session::new(200, Some(9)).unwrap();
        c.draw(50).unwrap();
        assert_eq!(strip(b.save()), strip(c.save()));
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
        assert!(mutate(&|v| v["version"] = json!(2)).is_some());
        assert!(mutate(&|v| v["version"] = json!(4)).is_some());
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
    fn id_is_unique_and_revision_counts_state_changing_draws() {
        let mut a = Session::new(20, Some(1)).unwrap();
        let b = Session::new(20, Some(1)).unwrap();
        assert_eq!(a.id().len(), 32);
        assert_ne!(a.id(), b.id()); // シードが同じでも別のセッション
        assert_eq!(a.revision(), 0);
        a.draw(0).unwrap();
        assert_eq!(a.revision(), 0); // k == 0 は何もしない
        a.draw(3).unwrap();
        a.draw(2).unwrap();
        assert_eq!(a.revision(), 2);
        assert!(a.draw(100).is_err());
        assert_eq!(a.revision(), 2); // 失敗は数えない
        let loaded = Session::load(&a.save()).unwrap();
        assert_eq!((loaded.id(), loaded.revision()), (a.id(), 2));
    }

    #[test]
    fn idempotency_key_replays_and_detects_conflict() {
        let mut s = Session::new(50, None).unwrap();
        let first = s.draw_keyed(3, Some("req-1")).unwrap();
        assert!(!first.replayed);
        let before = s.save();
        // 同じキー・同じ k の再送は、状態を変えずに前回の結果を返す
        let again = s.draw_keyed(3, Some("req-1")).unwrap();
        assert_eq!((again.picks.clone(), again.replayed), (first.picks.clone(), true));
        assert_eq!(s.save(), before);
        // 別の k は衝突
        assert_eq!(s.draw_keyed(4, Some("req-1")), Err(SORTITION_ERR_CONFLICT));
        assert_eq!(s.draw_keyed(0, Some("req-1")), Err(SORTITION_ERR_CONFLICT));
        assert_eq!(s.save(), before);
        // 次のリクエストは続きを引く
        let second = s.draw_keyed(2, Some("req-2")).unwrap();
        assert!(!second.replayed);
        assert!(second.picks.iter().all(|p| !first.picks.contains(p)));
        // キーなしの呼び出しは記録されず、再送の対象にもならない
        s.draw_keyed(1, None).unwrap();
        assert_eq!(s.revision(), 3);
        // k == 0 はキーを記録しない（あとで同じキーを別の k で使える）
        assert!(s.draw_keyed(0, Some("req-3")).unwrap().picks.is_empty());
        assert!(!s.draw_keyed(1, Some("req-3")).unwrap().replayed);
    }

    #[test]
    fn idempotency_key_validation_and_failed_draws_leave_no_trace() {
        let mut s = Session::new(5, None).unwrap();
        assert_eq!(s.draw_keyed(1, Some("")), Err(SORTITION_ERR_INVALID));
        assert_eq!(s.draw_keyed(1, Some(&"k".repeat(MAX_KEY_LEN + 1))), Err(SORTITION_ERR_INVALID));
        assert!(s.draw_keyed(1, Some(&"k".repeat(MAX_KEY_LEN))).is_ok());
        let before = s.save();
        // 残りを超える要求は失敗し、キーも記録されない（同じキーで引き直せる）
        assert_eq!(s.draw_keyed(9, Some("big")), Err(SORTITION_ERR_INVALID));
        assert_eq!(s.save(), before);
        assert!(s.draw_keyed(2, Some("big")).is_ok());
    }

    #[test]
    fn replay_survives_save_load_and_stale_state_resend_is_identical() {
        let mut s = Session::new(100, None).unwrap();
        s.draw_keyed(4, Some("a")).unwrap();
        let stale = s.save(); // "b" を引く前の状態
        let b = s.draw_keyed(6, Some("b")).unwrap();
        let after = s.save();

        // 引いた後の状態を復元して同じキーを再送 → 前回の結果、状態は不変
        let mut r = Session::load(&after).unwrap();
        let replay = r.draw_keyed(6, Some("b")).unwrap();
        assert_eq!((replay.picks.clone(), replay.replayed), (b.picks.clone(), true));
        assert_eq!(r.save(), after);

        // 引く前の古い状態に同じリクエストを送っても、同じ結果・同じ状態になる
        let mut old = Session::load(&stale).unwrap();
        let again = old.draw_keyed(6, Some("b")).unwrap();
        assert_eq!((again.picks, again.replayed), (b.picks, false));
        assert_eq!(old.save(), after);
    }

    #[test]
    fn rejects_bad_calls_and_revision() {
        let mut s = Session::new(30, Some(2)).unwrap();
        s.draw_keyed(3, Some("a")).unwrap();
        s.draw_keyed(2, Some("b")).unwrap();
        let good: Value = serde_json::from_str(&s.save()).unwrap();
        let bad = |f: &dyn Fn(&mut Value)| {
            let mut v = good.clone();
            f(&mut v);
            Session::load(&v.to_string()).is_err()
        };
        assert!(!bad(&|_| {}));
        assert!(bad(&|v| v["id"] = json!("abcd")));
        assert!(bad(&|v| v.as_object_mut().unwrap().remove("id").map(|_| ()).unwrap()));
        assert!(bad(&|v| v["revision"] = json!(0))); // 引いているのに 0
        assert!(bad(&|v| v["revision"] = json!(1))); // 記録 2 件より少ない
        assert!(bad(&|v| v["revision"] = json!(6))); // 引いた数 5 を超える
        assert!(bad(&|v| v["calls"][1][0] = json!("a"))); // キーの重複
        assert!(bad(&|v| v["calls"][0][0] = json!(""))); // 空のキー
        assert!(bad(&|v| v["calls"][1][1] = json!(2))); // 前の記録と重なる
        assert!(bad(&|v| v["calls"][1][2] = json!(4))); // 引いた数をはみ出す
        assert!(bad(&|v| v["calls"][0][2] = json!(0))); // 0 個の記録
        assert!(bad(&|v| v["calls"] = json!("x")));
    }

    #[test]
    fn seal_roundtrip_hides_rng_and_resumes() {
        let key = [9u8; 32];
        let mut s = Session::new(40, None).unwrap();
        s.draw_keyed(5, Some("r1")).unwrap();
        let token = s.seal(&key, 1).unwrap();
        let state: Value = serde_json::from_str(&s.save()).unwrap();
        let rng_key = state["rng"]["key"].as_str().unwrap();
        assert!(!token.contains(rng_key)); // 乱数の鍵は見えない
        assert!(!token.contains("chacha20"));

        let mut r = Session::unseal(&token, &key).unwrap();
        assert_eq!(r.save(), s.save());
        // 封を開いて続きから引ける。キーの記録も引き継がれる
        assert!(r.draw_keyed(5, Some("r1")).unwrap().replayed);
        assert_eq!(r.draw(3).unwrap(), s.draw(3).unwrap());

        assert!(Session::unseal(&token, &[8u8; 32]).is_err());
        // 封はしてあるが中身が状態でない場合は、開けても復元で拒否する
        let junk = crate::seal::seal(b"{}", &key, 1).unwrap();
        assert!(Session::unseal(&junk, &key).is_err());
        let not_utf8 = crate::seal::seal(&[0xff, 0xfe], &key, 1).unwrap();
        assert!(Session::unseal(&not_utf8, &key).is_err());
    }

    #[test]
    fn c_abi_seal_unseal() {
        unsafe {
            let key = [3u8; 32];
            let mut s: *mut Session = std::ptr::null_mut();
            assert_eq!(sortition_session_new(20, std::ptr::null(), &mut s), SORTITION_OK);
            let mut buf = [0u32; 4];
            assert_eq!(sortition_session_draw(s, 4, buf.as_mut_ptr()), SORTITION_OK);

            let mut token: *mut c_char = std::ptr::null_mut();
            assert_eq!(sortition_session_seal(s, key.as_ptr(), 5, &mut token), SORTITION_OK);
            let text = std::ffi::CStr::from_ptr(token).to_bytes().to_vec();
            crate::sortition_free(token);
            let mut kid = 0u8;
            assert_eq!(crate::seal::sortition_seal_key_id(text.as_ptr(), text.len(), &mut kid), SORTITION_OK);
            assert_eq!(kid, 5);

            let mut t: *mut Session = std::ptr::null_mut();
            assert_eq!(sortition_session_unseal(text.as_ptr(), text.len(), key.as_ptr(), &mut t), SORTITION_OK);
            assert_eq!(sortition_session_revision(t), 1);
            assert_eq!((*t).save(), (*s).save());
            sortition_session_free(t);

            let wrong = [4u8; 32];
            assert_eq!(sortition_session_unseal(text.as_ptr(), text.len(), wrong.as_ptr(), &mut t), SORTITION_ERR_STATE);
            assert!(t.is_null());
            assert_eq!(sortition_session_unseal(b"junk".as_ptr(), 4, key.as_ptr(), &mut t), SORTITION_ERR_STATE);
            assert_eq!(crate::seal::sortition_seal_key_id(b"junk".as_ptr(), 4, &mut kid), SORTITION_ERR_STATE);
            assert_eq!(sortition_session_unseal(text.as_ptr(), text.len(), std::ptr::null(), &mut t), SORTITION_ERR_INVALID);
            let mut out: *mut c_char = std::ptr::null_mut();
            assert_eq!(sortition_session_seal(s, std::ptr::null(), 0, &mut out), SORTITION_ERR_INVALID);
            sortition_session_free(s);
        }
    }

    #[test]
    fn c_abi_keyed_draw_revision_and_id() {
        unsafe {
            let mut s: *mut Session = std::ptr::null_mut();
            assert_eq!(sortition_session_new(30, std::ptr::null(), &mut s), SORTITION_OK);
            let mut id = [0xffu8; 33];
            assert_eq!(sortition_session_id(s, id.as_mut_ptr()), SORTITION_OK);
            assert_eq!(id[32], 0);
            assert_eq!(std::str::from_utf8(&id[..32]).unwrap(), (*s).id());

            let (mut a, mut b) = ([0u32; 4], [0u32; 4]);
            let mut replayed = -1i32;
            let key = b"req-1\0".as_ptr() as *const c_char;
            assert_eq!(sortition_session_draw_keyed(s, 4, key, a.as_mut_ptr(), &mut replayed), SORTITION_OK);
            assert_eq!(replayed, 0);
            assert_eq!(sortition_session_revision(s), 1);
            assert_eq!(sortition_session_draw_keyed(s, 4, key, b.as_mut_ptr(), &mut replayed), SORTITION_OK);
            assert_eq!((replayed, a), (1, b));
            assert_eq!(sortition_session_revision(s), 1);
            assert_eq!(sortition_session_draw_keyed(s, 3, key, b.as_mut_ptr(), &mut replayed), SORTITION_ERR_CONFLICT);
            assert_eq!(replayed, 0);
            // キーなし（NULL）、replayed が NULL でもよい
            assert_eq!(sortition_session_draw_keyed(s, 4, std::ptr::null(), b.as_mut_ptr(), std::ptr::null_mut()), SORTITION_OK);
            assert_eq!(sortition_session_revision(s), 2);
            // UTF-8 でないキー、空のキー
            let bad_utf8 = b"\xff\0".as_ptr() as *const c_char;
            assert_eq!(sortition_session_draw_keyed(s, 1, bad_utf8, b.as_mut_ptr(), &mut replayed), SORTITION_ERR_UTF8);
            let empty = b"\0".as_ptr() as *const c_char;
            assert_eq!(sortition_session_draw_keyed(s, 1, empty, b.as_mut_ptr(), &mut replayed), SORTITION_ERR_INVALID);
            assert_eq!(sortition_session_revision(std::ptr::null()), 0);
            sortition_session_free(s);
        }
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
