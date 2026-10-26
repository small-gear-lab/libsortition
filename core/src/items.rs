//! 任意のバイト列を候補にした抽選。
//!
//! 各候補を「並び順に依存しない一意な ID」に変換し、ID の昇順に並べた上で番号を引く。
//! 同じ候補集合とシードなら、入力の並びが違っても同じ候補が当選する。
//!
//! - ID は `SHA-256(接頭辞 ‖ バイト列 ‖ 出現番号)`。同じバイト列が複数あれば、先頭から
//!   数えた出現番号（0 始まり、8 バイトのリトルエンディアン）を混ぜて別候補にする。
//! - 保存状態には「候補集合の指紋」を入れ、復元時に渡された候補と照合する。

use std::collections::HashMap;
use std::ffi::{c_char, CString};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::hex;
use crate::session::{Session, StateError};
use crate::{
    set_last_error, SORTITION_ERR_INTERNAL, SORTITION_ERR_INVALID, SORTITION_ERR_STATE,
    SORTITION_ERR_UTF8, SORTITION_OK,
};

/// The ID of a candidate: 32 bytes of SHA-256 that do not depend on its position (see `docs/SPEC.md`).
pub type Id = [u8; 32];

const ID_PREFIX: &[u8] = b"libsortition/candidate/v1\0";
const SET_PREFIX: &[u8] = b"libsortition/set/v1\0";
const STATE_VERSION: u64 = 1;

/// A winner: `index` is its position in the list you passed, and `id` is its ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pick {
    pub index: u32,
    pub id: Id,
}

/// The ID of each candidate, in the order of the input.
///
/// An ID is `SHA-256("libsortition/candidate/v1" || 0x00 || bytes || LE64(occurrence))`, where `occurrence`
/// is how many candidates with exactly the same bytes came before it. Identical entries stay separate
/// candidates and every ID is different.
pub fn candidate_ids(items: &[&[u8]]) -> Vec<Id> {
    let mut seen: HashMap<&[u8], u64> = HashMap::with_capacity(items.len());
    items
        .iter()
        .map(|&bytes| {
            let occurrence = seen.entry(bytes).or_insert(0);
            let mut h = Sha256::new();
            h.update(ID_PREFIX);
            h.update(bytes);
            h.update(occurrence.to_le_bytes());
            *occurrence += 1;
            h.finalize().into()
        })
        .collect()
}

/// The fingerprint of a set of candidates: the SHA-256 of their IDs in sorted order, so it does not depend
/// on the order of the input. Publish it with the commitment before the draw.
pub fn set_fingerprint(ids: &[Id]) -> Id {
    let mut sorted: Vec<&Id> = ids.iter().collect();
    sorted.sort_unstable();
    fingerprint_of_sorted(sorted.into_iter())
}

fn fingerprint_of_sorted<'a>(ids: impl ExactSizeIterator<Item = &'a Id>) -> Id {
    let mut h = Sha256::new();
    h.update(SET_PREFIX);
    h.update((ids.len() as u64).to_le_bytes());
    for id in ids {
        h.update(id);
    }
    h.finalize().into()
}

fn to_hex(id: &Id) -> String {
    hex::encode(id)
}

fn from_hex(s: &str) -> Option<Id> {
    hex::decode(s)
}

/// 候補を ID 昇順に並べたときの対応表（昇順の位置 → 入力の位置）と、昇順の ID。
fn canonical_order(items: &[&[u8]]) -> Result<(Vec<u32>, Vec<Id>), i32> {
    u32::try_from(items.len()).map_err(|_| SORTITION_ERR_INVALID)?;
    let ids = candidate_ids(items);
    let mut order: Vec<u32> = (0..items.len() as u32).collect();
    order.sort_unstable_by_key(|&i| ids[i as usize]);
    let sorted: Vec<Id> = order.iter().map(|&i| ids[i as usize]).collect();
    // SHA-256 の衝突は現実的にないが、起きたら一意性が崩れるので黙って進めない
    if sorted.windows(2).any(|w| w[0] == w[1]) {
        return Err(SORTITION_ERR_INTERNAL);
    }
    Ok((order, sorted))
}

/// A session over a fixed set of candidates (arbitrary byte strings). It draws without replacement, can
/// continue where it stopped, and can be saved and restored together with the same set of candidates (in
/// any order). The winners do not depend on the order of the list.
pub struct ItemSession {
    inner: Session,
    /// 昇順の位置 → 入力の位置
    order: Vec<u32>,
    /// 昇順の ID
    ids: Vec<Id>,
    fingerprint: Id,
}

impl ItemSession {
    /// Creates a session over the candidates. Fails with `SORTITION_ERR_INVALID` for 2^32 candidates or more.
    pub fn new(items: &[&[u8]], seed: Option<u64>) -> Result<Self, i32> {
        let (order, ids) = canonical_order(items)?;
        let fingerprint = fingerprint_of_sorted(ids.iter());
        Ok(Self { inner: Session::new(ids.len() as u32, seed)?, order, ids, fingerprint })
    }

    /// How many candidates are not drawn yet.
    pub fn remaining(&self) -> u32 {
        self.inner.remaining()
    }

    /// How many candidates were drawn.
    pub fn drawn_count(&self) -> u32 {
        self.inner.drawn().len() as u32
    }

    /// The fingerprint of the set of candidates.
    pub fn fingerprint(&self) -> Id {
        self.fingerprint
    }

    fn pick(&self, pos: u32) -> Pick {
        Pick { index: self.order[pos as usize], id: self.ids[pos as usize] }
    }

    /// The winners so far, in the order drawn.
    pub fn drawn(&self) -> Vec<Pick> {
        self.inner.drawn().iter().map(|&p| self.pick(p)).collect()
    }

    /// Draws the next `k`. Fails with `SORTITION_ERR_INVALID`, without changing the state, if `k` is more
    /// than what is left.
    pub fn draw(&mut self, k: u32) -> Result<Vec<Pick>, i32> {
        Ok(self.inner.draw(k)?.into_iter().map(|p| self.pick(p)).collect())
    }

    /// Draws the next `k` with an idempotency key. Sending the same key with the same `k` again returns the
    /// earlier result without changing the state (the second value is then `true`). See
    /// [`Session::draw_keyed`].
    pub fn draw_keyed(&mut self, k: u32, key: Option<&str>) -> Result<(Vec<Pick>, bool), i32> {
        let d = self.inner.draw_keyed(k, key)?;
        Ok((d.picks.into_iter().map(|p| self.pick(p)).collect(), d.replayed))
    }

    /// The session id: 32 lowercase hex characters.
    pub fn id(&self) -> String {
        self.inner.id()
    }

    /// The number of draws that changed the state.
    pub fn revision(&self) -> u64 {
        self.inner.revision()
    }

    /// Saves the state as a JSON string, with the IDs of the winners (`drawn`) and the fingerprint of the set.
    /// It contains the internal state of the random generator, so keep it from parties you do not trust
    /// (see [`ItemSession::seal`]).
    pub fn save(&self) -> String {
        let drawn: Vec<String> = self.drawn().iter().map(|p| to_hex(&p.id)).collect();
        let session: Value = serde_json::from_str(&self.inner.save()).expect("session state is JSON");
        json!({
            "version": STATE_VERSION,
            "kind": "items",
            "fingerprint": to_hex(&self.fingerprint),
            "drawn": drawn,
            "session": session,
        })
        .to_string()
    }

    /// Seals the saved state into a token, so that it can be kept by a party that must not read or change it.
    /// The arguments are the same as for [`Session::seal`].
    pub fn seal(&self, key: &crate::seal::Key, key_id: u8) -> Result<String, i32> {
        crate::seal::seal(self.save().as_bytes(), key, key_id)
    }

    /// Restores a session from a token made by [`ItemSession::seal`] and the same set of candidates (the order
    /// may differ).
    pub fn unseal(token: &str, key: &crate::seal::Key, items: &[&[u8]]) -> Result<Self, StateError> {
        let plain = crate::seal::open(token, key)?;
        let text = String::from_utf8(plain).map_err(|_| StateError("sealed state is not UTF-8".to_string()))?;
        Self::load(&text, items)
    }

    /// Restores a session from the output of [`ItemSession::save`] and the same set of candidates (the order
    /// may differ). A different set, or a state that is inconsistent, gives a [`StateError`].
    pub fn load(state: &str, items: &[&[u8]]) -> Result<Self, StateError> {
        let bad = |m: &str| StateError(m.to_string());
        let v: Value = serde_json::from_str(state).map_err(|e| StateError(format!("invalid state: {e}")))?;
        let obj = v.as_object().ok_or_else(|| bad("state is not an object"))?;
        if obj.get("version").and_then(Value::as_u64) != Some(STATE_VERSION)
            || obj.get("kind").and_then(Value::as_str) != Some("items")
        {
            return Err(bad("unsupported state version or kind"));
        }
        let saved_fp = obj
            .get("fingerprint")
            .and_then(Value::as_str)
            .and_then(from_hex)
            .ok_or_else(|| bad("invalid fingerprint"))?;

        let (order, ids) = canonical_order(items).map_err(|_| bad("invalid candidates"))?;
        let fingerprint = fingerprint_of_sorted(ids.iter());
        if fingerprint != saved_fp {
            return Err(bad("candidates do not match the saved state"));
        }

        let inner = Session::load(&obj.get("session").ok_or_else(|| bad("missing session"))?.to_string())?;
        if inner.population() as usize != ids.len() {
            return Err(bad("state is inconsistent"));
        }
        let me = Self { inner, order, ids, fingerprint };

        let saved_drawn: Vec<&str> = obj
            .get("drawn")
            .and_then(Value::as_array)
            .ok_or_else(|| bad("invalid drawn"))?
            .iter()
            .map(Value::as_str)
            .collect::<Option<_>>()
            .ok_or_else(|| bad("invalid drawn"))?;
        let actual: Vec<String> = me.drawn().iter().map(|p| to_hex(&p.id)).collect();
        if saved_drawn != actual {
            return Err(bad("state is inconsistent"));
        }
        Ok(me)
    }
}

/// Draws `k` of the candidates without replacement. The same set of candidates and the same seed give the
/// same winners whatever the order of `items`. The result lists each winner's index in `items` and ID.
/// Fails with `SORTITION_ERR_INVALID` when `k` is more than the number of candidates.
pub fn draw_items(items: &[&[u8]], k: u32, seed: Option<u64>) -> Result<Vec<Pick>, i32> {
    ItemSession::new(items, seed)?.draw(k)
}

// ---- C ABI ----
// 候補は「連結したバッファ `data` ＋ 各候補の長さ `lens[0..count]`」で渡す。
// `data` は `lens` の合計バイト以上読める領域であること。

unsafe fn gather<'a>(data: *const u8, lens: *const usize, count: usize) -> Result<Vec<&'a [u8]>, i32> {
    if count == 0 {
        return Ok(Vec::new());
    }
    if lens.is_null() {
        return Err(SORTITION_ERR_INVALID);
    }
    let lens = std::slice::from_raw_parts(lens, count);
    let mut total = 0usize;
    for &l in lens {
        total = total.checked_add(l).ok_or(SORTITION_ERR_INVALID)?;
    }
    if data.is_null() && total > 0 {
        return Err(SORTITION_ERR_INVALID);
    }
    let mut items = Vec::with_capacity(count);
    let mut off = 0usize;
    for &l in lens {
        items.push(if l == 0 { &[][..] } else { std::slice::from_raw_parts(data.add(off), l) });
        off += l;
    }
    Ok(items)
}

unsafe fn write_picks(picks: &[Pick], out_index: *mut u32, out_id: *mut u8) {
    for (i, p) in picks.iter().enumerate() {
        if !out_index.is_null() {
            *out_index.add(i) = p.index;
        }
        if !out_id.is_null() {
            std::ptr::copy_nonoverlapping(p.id.as_ptr(), out_id.add(32 * i), 32);
        }
    }
}

/// C ABI: 各候補の ID（32 バイト）を入力と同じ並びで `out_ids[0..count*32]` に書く。
///
/// # Safety
/// `data` / `lens` は上の規約どおり、`out_ids` は `count * 32` バイト書ける領域であること。
#[doc(hidden)]
#[no_mangle]
pub unsafe extern "C" fn sortition_item_ids(data: *const u8, lens: *const usize, count: usize, out_ids: *mut u8) -> i32 {
    if out_ids.is_null() && count > 0 {
        return SORTITION_ERR_INVALID;
    }
    match gather(data, lens, count) {
        Ok(items) => {
            for (i, id) in candidate_ids(&items).iter().enumerate() {
                std::ptr::copy_nonoverlapping(id.as_ptr(), out_ids.add(32 * i), 32);
            }
            SORTITION_OK
        }
        Err(e) => e,
    }
}

/// C ABI: 候補集合の指紋（32 バイト）を `out_fingerprint` に書く。並びに依らない。
///
/// # Safety
/// `data` / `lens` は上の規約どおり、`out_fingerprint` は 32 バイト書ける領域であること。
#[doc(hidden)]
#[no_mangle]
pub unsafe extern "C" fn sortition_item_fingerprint(
    data: *const u8,
    lens: *const usize,
    count: usize,
    out_fingerprint: *mut u8,
) -> i32 {
    if out_fingerprint.is_null() {
        return SORTITION_ERR_INVALID;
    }
    match gather(data, lens, count) {
        Ok(items) => {
            let fp = set_fingerprint(&candidate_ids(&items));
            std::ptr::copy_nonoverlapping(fp.as_ptr(), out_fingerprint, 32);
            SORTITION_OK
        }
        Err(e) => e,
    }
}

/// C ABI: 候補から `k` 個を一度だけ抽選する。当選した候補の入力内の位置を `out_index[0..k]`、
/// ID を `out_ids[0..k*32]` に書く（どちらも NULL なら書かない）。
///
/// # Safety
/// `data` / `lens` / `seed` は規約どおり、`out_index` は `k` 要素、`out_ids` は `k * 32` バイト書ける領域であること。
#[doc(hidden)]
#[no_mangle]
pub unsafe extern "C" fn sortition_items_draw(
    data: *const u8,
    lens: *const usize,
    count: usize,
    k: u32,
    seed: *const u64,
    out_index: *mut u32,
    out_ids: *mut u8,
) -> i32 {
    let seed = if seed.is_null() { None } else { Some(*seed) };
    let items = match gather(data, lens, count) {
        Ok(v) => v,
        Err(e) => return e,
    };
    match draw_items(&items, k, seed) {
        Ok(picks) => {
            write_picks(&picks, out_index, out_ids);
            SORTITION_OK
        }
        Err(e) => e,
    }
}

/// C ABI: 候補集合を固定した抽選セッションを作る。`*out` は `sortition_item_session_free` で解放。
///
/// # Safety
/// `data` / `lens` / `seed` は規約どおり、`out` は書き込み可能なポインタ 1 個分の領域であること。
#[doc(hidden)]
#[no_mangle]
pub unsafe extern "C" fn sortition_item_session_new(
    data: *const u8,
    lens: *const usize,
    count: usize,
    seed: *const u64,
    out: *mut *mut ItemSession,
) -> i32 {
    if out.is_null() {
        return SORTITION_ERR_INVALID;
    }
    *out = std::ptr::null_mut();
    let seed = if seed.is_null() { None } else { Some(*seed) };
    let items = match gather(data, lens, count) {
        Ok(v) => v,
        Err(e) => return e,
    };
    match ItemSession::new(&items, seed) {
        Ok(s) => {
            *out = Box::into_raw(Box::new(s));
            SORTITION_OK
        }
        Err(e) => e,
    }
}

/// C ABI: 続きから `k` 個引く。書き込み先は `sortition_items_draw` と同じ。
/// `k` が残りを超えると `SORTITION_ERR_INVALID`（状態は変わらない）。
///
/// # Safety
/// `s` は有効なセッション、`out_index` / `out_ids` は規約どおり。同じセッションを複数スレッドから同時に使わないこと。
#[doc(hidden)]
#[no_mangle]
pub unsafe extern "C" fn sortition_item_session_draw(
    s: *mut ItemSession,
    k: u32,
    out_index: *mut u32,
    out_ids: *mut u8,
) -> i32 {
    if s.is_null() {
        return SORTITION_ERR_INVALID;
    }
    match (*s).draw(k) {
        Ok(picks) => {
            write_picks(&picks, out_index, out_ids);
            SORTITION_OK
        }
        Err(e) => e,
    }
}

/// C ABI: 羃等キー付きで続きから `k` 個引く。書き込み先は `sortition_items_draw` と同じ。
/// キーの扱いと `replayed` は `sortition_session_draw_keyed` と同じ。
///
/// # Safety
/// `s` は有効なセッション、`key` は NULL か NUL 終端文字列、`out_index` / `out_ids` は規約どおり、
/// `replayed` は NULL か書き込み可能な `int32_t` であること。
#[doc(hidden)]
#[no_mangle]
pub unsafe extern "C" fn sortition_item_session_draw_keyed(
    s: *mut ItemSession,
    k: u32,
    key: *const c_char,
    out_index: *mut u32,
    out_ids: *mut u8,
    replayed: *mut i32,
) -> i32 {
    if s.is_null() {
        return SORTITION_ERR_INVALID;
    }
    if !replayed.is_null() {
        *replayed = 0;
    }
    let key = match crate::session::key_arg(key) {
        Ok(k) => k,
        Err(e) => return e,
    };
    match (*s).draw_keyed(k, key) {
        Ok((picks, was_replayed)) => {
            write_picks(&picks, out_index, out_ids);
            if !replayed.is_null() {
                *replayed = was_replayed as i32;
            }
            SORTITION_OK
        }
        Err(e) => e,
    }
}

/// C ABI: 状態を変えた `draw` の回数。`s` が NULL なら 0。
///
/// # Safety
/// `s` は NULL か有効なセッションであること。
#[doc(hidden)]
#[no_mangle]
pub unsafe extern "C" fn sortition_item_session_revision(s: *const ItemSession) -> u64 {
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
#[doc(hidden)]
#[no_mangle]
pub unsafe extern "C" fn sortition_item_session_id(s: *const ItemSession, out: *mut u8) -> i32 {
    if s.is_null() || out.is_null() {
        return SORTITION_ERR_INVALID;
    }
    crate::session::write_id((*s).id(), out);
    SORTITION_OK
}

/// C ABI: まだ引いていない数。`s` が NULL なら 0。
///
/// # Safety
/// `s` は NULL か有効なセッションであること。
#[doc(hidden)]
#[no_mangle]
pub unsafe extern "C" fn sortition_item_session_remaining(s: *const ItemSession) -> u32 {
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
#[doc(hidden)]
#[no_mangle]
pub unsafe extern "C" fn sortition_item_session_drawn_count(s: *const ItemSession) -> u32 {
    if s.is_null() {
        0
    } else {
        (*s).drawn_count()
    }
}

/// C ABI: 状態を JSON 文字列として `*out` に返す（`sortition_free` で解放）。
///
/// # Safety
/// `s` は有効なセッション、`out` は書き込み可能な `char*` 1 個分の領域であること。
#[doc(hidden)]
#[no_mangle]
pub unsafe extern "C" fn sortition_item_session_save(s: *const ItemSession, out: *mut *mut c_char) -> i32 {
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

/// C ABI: 保存した状態を、同じ候補集合（並びは違ってよい）から復元する。
/// 候補集合が違う・状態が不正なら `SORTITION_ERR_STATE`（詳細は `sortition_last_error`）。
///
/// # Safety
/// `state` は `state_len` バイト読める領域、`data` / `lens` は規約どおり、`out` は書き込み可能なポインタ 1 個分の領域であること。
#[doc(hidden)]
#[no_mangle]
pub unsafe extern "C" fn sortition_item_session_load(
    state: *const u8,
    state_len: usize,
    data: *const u8,
    lens: *const usize,
    count: usize,
    out: *mut *mut ItemSession,
) -> i32 {
    if out.is_null() {
        return SORTITION_ERR_INVALID;
    }
    *out = std::ptr::null_mut();
    if state.is_null() && state_len > 0 {
        set_last_error("state is NULL");
        return SORTITION_ERR_INVALID;
    }
    let bytes: &[u8] = if state_len == 0 { &[] } else { std::slice::from_raw_parts(state, state_len) };
    let Ok(text) = std::str::from_utf8(bytes) else {
        set_last_error("state is not valid UTF-8");
        return SORTITION_ERR_UTF8;
    };
    let items = match gather(data, lens, count) {
        Ok(v) => v,
        Err(e) => return e,
    };
    match std::panic::catch_unwind(|| ItemSession::load(text, &items)) {
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

/// C ABI: 状態に封をしたトークンを `*out` に返す（`sortition_free` で解放）。引数は `sortition_session_seal` と同じ。
///
/// # Safety
/// `s` は有効なセッション、`key` は 32 バイト読める領域、`out` は書き込み可能な `char*` 1 個分の領域であること。
#[doc(hidden)]
#[no_mangle]
pub unsafe extern "C" fn sortition_item_session_seal(
    s: *const ItemSession,
    key: *const u8,
    key_id: u8,
    out: *mut *mut c_char,
) -> i32 {
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

/// C ABI: `sortition_item_session_seal` のトークンと、同じ候補集合（並びは違ってよい）から復元する。
/// 形式の不正・鍵違い・改ざん・候補集合の不一致は `SORTITION_ERR_STATE`（詳細は `sortition_last_error`）。
///
/// # Safety
/// `token` は `len` バイト読める領域、`key` は 32 バイト読める領域、`data` / `lens` は規約どおり、
/// `out` は書き込み可能なポインタ 1 個分の領域であること。
#[doc(hidden)]
#[no_mangle]
pub unsafe extern "C" fn sortition_item_session_unseal(
    token: *const u8,
    len: usize,
    key: *const u8,
    data: *const u8,
    lens: *const usize,
    count: usize,
    out: *mut *mut ItemSession,
) -> i32 {
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
    let items = match gather(data, lens, count) {
        Ok(v) => v,
        Err(e) => return e,
    };
    match std::panic::catch_unwind(|| ItemSession::unseal(token, key, &items)) {
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
/// `s` は `sortition_item_session_new` / `_load` が返した未解放のポインタか NULL であること。
#[doc(hidden)]
#[no_mangle]
pub unsafe extern "C" fn sortition_item_session_free(s: *mut ItemSession) {
    if !s.is_null() {
        drop(Box::from_raw(s));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Vec<&'static [u8]> {
        vec![b"alice", b"bob", b"carol", b"dave", b"erin", b"frank", b"grace", b"heidi"]
    }

    fn ids_of(picks: &[Pick]) -> Vec<Id> {
        picks.iter().map(|p| p.id).collect()
    }

    #[test]
    fn id_is_sha256_of_prefix_bytes_occurrence() {
        // printf 'libsortition/candidate/v1\0abc\0\0\0\0\0\0\0\0' | sha256sum
        assert_eq!(
            to_hex(&candidate_ids(&[b"abc"])[0]),
            "3b39139043226007aae3c88c03f4a1867e3cea04126ee893f8b395f7eb97899b"
        );
    }

    #[test]
    fn duplicates_get_distinct_ids_and_binary_is_fine() {
        let items: Vec<&[u8]> = vec![b"x", b"x", b"", b"", b"\xff\x00\xfe", b"x"];
        let ids = candidate_ids(&items);
        let mut uniq = ids.clone();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(uniq.len(), items.len());
        // 同じバイト列でも、先頭からの出現番号が違えば別 ID
        assert_ne!(ids[0], ids[1]);
        assert_ne!(ids[2], ids[3]);
    }

    #[test]
    fn result_does_not_depend_on_input_order() {
        let a = names();
        let mut b = names();
        b.reverse();
        b.swap(1, 4);
        for seed in 0..20u64 {
            let pa = draw_items(&a, 5, Some(seed)).unwrap();
            let pb = draw_items(&b, 5, Some(seed)).unwrap();
            assert_eq!(ids_of(&pa), ids_of(&pb));
            // index は各入力配列の中の位置で、指す候補は同じ
            let bytes_a: Vec<_> = pa.iter().map(|p| a[p.index as usize]).collect();
            let bytes_b: Vec<_> = pb.iter().map(|p| b[p.index as usize]).collect();
            assert_eq!(bytes_a, bytes_b);
        }
        assert_eq!(set_fingerprint(&candidate_ids(&a)), set_fingerprint(&candidate_ids(&b)));
    }

    #[test]
    fn draw_all_with_duplicates_hits_each_index_once() {
        let items: Vec<&[u8]> = vec![b"a", b"a", b"b", b"a"];
        let mut idx: Vec<u32> = draw_items(&items, 4, None).unwrap().iter().map(|p| p.index).collect();
        idx.sort_unstable();
        assert_eq!(idx, [0, 1, 2, 3]);
        assert_eq!(draw_items(&items, 5, None), Err(SORTITION_ERR_INVALID));
        assert_eq!(draw_items(&[], 0, None).unwrap(), vec![]);
    }

    #[test]
    fn split_draws_equal_one_draw() {
        let items = names();
        let whole = draw_items(&items, 6, Some(3)).unwrap();
        let mut s = ItemSession::new(&items, Some(3)).unwrap();
        let mut got = s.draw(2).unwrap();
        got.extend(s.draw(4).unwrap());
        assert_eq!(got, whole);
        assert_eq!(s.drawn(), got);
        assert_eq!(s.remaining(), 2);
    }

    #[test]
    fn save_load_resumes_even_if_order_changed() {
        let items = names();
        let mut a = ItemSession::new(&items, Some(11)).unwrap();
        let mut got = a.draw(3).unwrap();
        let state = a.save();

        let mut shuffled = names();
        shuffled.rotate_left(3);
        shuffled.swap(0, 5);
        let mut b = ItemSession::load(&state, &shuffled).unwrap();
        let rest = b.draw(5).unwrap();
        got.extend(a.draw(5).unwrap());
        assert_eq!(ids_of(&got[3..]), ids_of(&rest));
        // 復元側の index は、渡し直した配列の中の位置
        for p in &rest {
            assert_eq!(candidate_ids(&shuffled)[p.index as usize], p.id);
        }
        assert_eq!(ids_of(&got), ids_of(&draw_items(&items, 8, Some(11)).unwrap()));
    }

    #[test]
    fn load_rejects_different_candidates_and_tampering() {
        let items = names();
        let mut s = ItemSession::new(&items, Some(1)).unwrap();
        s.draw(2).unwrap();
        let state = s.save();

        let mut fewer = names();
        fewer.pop();
        let mut replaced = names();
        replaced[0] = b"mallory";
        for other in [&fewer, &replaced] {
            let e = ItemSession::load(&state, other).err().unwrap();
            assert!(e.0.contains("do not match"), "{}", e.0);
        }

        let good: Value = serde_json::from_str(&state).unwrap();
        let mutate = |f: &dyn Fn(&mut Value)| {
            let mut v = good.clone();
            f(&mut v);
            ItemSession::load(&v.to_string(), &items).is_err()
        };
        assert!(ItemSession::load(&state, &items).is_ok());
        assert!(mutate(&|v| v["kind"] = json!("numbers")));
        assert!(mutate(&|v| v["version"] = json!(2)));
        assert!(mutate(&|v| v["fingerprint"] = json!("zz")));
        assert!(mutate(&|v| v["drawn"] = json!([])));
        assert!(mutate(&|v| v["session"]["n"] = json!(3)));
        assert!(ItemSession::load("not json", &items).is_err());
        // 数値セッションの状態は別物として拒否する
        assert!(ItemSession::load(&Session::new(8, None).unwrap().save(), &items).is_err());
    }

    #[test]
    fn seal_resumes_with_reordered_candidates_and_rejects_others() {
        let key = [5u8; 32];
        let items = names();
        let mut s = ItemSession::new(&items, None).unwrap();
        let first = s.draw(3).unwrap();
        let token = s.seal(&key, 0).unwrap();

        let mut shuffled = names();
        shuffled.rotate_left(2);
        let mut r = ItemSession::unseal(&token, &key, &shuffled).unwrap();
        assert_eq!(ids_of(&r.drawn()), ids_of(&first));
        assert_eq!(ids_of(&r.draw(5).unwrap()), ids_of(&s.draw(5).unwrap()));

        assert!(ItemSession::unseal(&token, &key, &items[1..]).is_err());
        assert!(ItemSession::unseal(&token, &[6u8; 32], &items).is_err());
    }

    #[test]
    fn keyed_draw_replays_across_reorder_and_reload() {
        let items = names();
        let mut s = ItemSession::new(&items, None).unwrap();
        let (first, replayed) = s.draw_keyed(3, Some("r1")).unwrap();
        assert!(!replayed);
        assert_eq!((s.revision(), s.id().len()), (1, 32));
        let state = s.save();

        // 並びを変えて復元しても、同じキーの再送は前回の ID を返す（index は渡し直した配列の位置）
        let mut shuffled = names();
        shuffled.reverse();
        let mut r = ItemSession::load(&state, &shuffled).unwrap();
        assert_eq!(r.id(), s.id());
        let (again, replayed) = r.draw_keyed(3, Some("r1")).unwrap();
        assert!(replayed);
        assert_eq!(ids_of(&again), ids_of(&first));
        assert_eq!(r.save(), state);
        assert_eq!(r.draw_keyed(2, Some("r1")), Err(crate::SORTITION_ERR_CONFLICT));
    }

    #[test]
    fn roughly_uniform_over_items() {
        let items: Vec<&[u8]> = vec![b"a", b"b", b"c", b"d", b"e"];
        let mut hits = HashMap::new();
        for seed in 0..10_000u64 {
            let p = draw_items(&items, 1, Some(seed)).unwrap();
            *hits.entry(p[0].index).or_insert(0u32) += 1;
        }
        assert!(hits.values().all(|&h| (1700..2300).contains(&h)), "{hits:?}");
    }

    #[test]
    fn c_abi_roundtrip() {
        let items = names();
        let data: Vec<u8> = items.concat();
        let lens: Vec<usize> = items.iter().map(|i| i.len()).collect();
        unsafe {
            let mut ids = vec![0u8; 32 * items.len()];
            assert_eq!(sortition_item_ids(data.as_ptr(), lens.as_ptr(), lens.len(), ids.as_mut_ptr()), SORTITION_OK);
            let expected: Vec<u8> = candidate_ids(&items).concat();
            assert_eq!(ids, expected);

            let mut fp = [0u8; 32];
            assert_eq!(sortition_item_fingerprint(data.as_ptr(), lens.as_ptr(), lens.len(), fp.as_mut_ptr()), SORTITION_OK);
            assert_eq!(fp, set_fingerprint(&candidate_ids(&items)));

            let seed = 4u64;
            let (mut idx, mut out_ids) = ([0u32; 5], [0u8; 5 * 32]);
            assert_eq!(
                sortition_items_draw(data.as_ptr(), lens.as_ptr(), lens.len(), 5, &seed, idx.as_mut_ptr(), out_ids.as_mut_ptr()),
                SORTITION_OK
            );
            let want = draw_items(&items, 5, Some(4)).unwrap();
            assert_eq!(idx.to_vec(), want.iter().map(|p| p.index).collect::<Vec<_>>());
            assert_eq!(out_ids.to_vec(), want.iter().flat_map(|p| p.id).collect::<Vec<_>>());

            // セッション: 3 個引いて保存、復元して残りを引く
            let mut s: *mut ItemSession = std::ptr::null_mut();
            assert_eq!(sortition_item_session_new(data.as_ptr(), lens.as_ptr(), lens.len(), &seed, &mut s), SORTITION_OK);
            let mut first = [0u32; 3];
            assert_eq!(sortition_item_session_draw(s, 3, first.as_mut_ptr(), std::ptr::null_mut()), SORTITION_OK);
            assert_eq!(sortition_item_session_remaining(s), 5);
            assert_eq!(sortition_item_session_drawn_count(s), 3);
            let mut json: *mut c_char = std::ptr::null_mut();
            assert_eq!(sortition_item_session_save(s, &mut json), SORTITION_OK);
            let state = std::ffi::CStr::from_ptr(json).to_bytes().to_vec();
            crate::sortition_free(json);
            sortition_item_session_free(s);

            let mut t: *mut ItemSession = std::ptr::null_mut();
            assert_eq!(
                sortition_item_session_load(state.as_ptr(), state.len(), data.as_ptr(), lens.as_ptr(), lens.len(), &mut t),
                SORTITION_OK
            );
            let mut rest = [0u32; 5];
            assert_eq!(sortition_item_session_draw(t, 5, rest.as_mut_ptr(), std::ptr::null_mut()), SORTITION_OK);
            assert_eq!(sortition_item_session_draw(t, 1, rest.as_mut_ptr(), std::ptr::null_mut()), SORTITION_ERR_INVALID);
            let all: Vec<u32> = first.iter().chain(rest.iter()).copied().collect();
            assert_eq!(all, draw_items(&items, 8, Some(4)).unwrap().iter().map(|p| p.index).collect::<Vec<_>>());
            sortition_item_session_free(t);

            // 候補が違えば ERR_STATE、NULL ポインタは INVALID
            let other: Vec<u8> = b"x".to_vec();
            let one = [1usize];
            let mut bad: *mut ItemSession = std::ptr::null_mut();
            assert_eq!(
                sortition_item_session_load(state.as_ptr(), state.len(), other.as_ptr(), one.as_ptr(), 1, &mut bad),
                SORTITION_ERR_STATE
            );
            assert!(bad.is_null());
            assert_eq!(sortition_item_ids(std::ptr::null(), lens.as_ptr(), lens.len(), ids.as_mut_ptr()), SORTITION_ERR_INVALID);
            sortition_item_session_free(std::ptr::null_mut());
        }
    }
}
