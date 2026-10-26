//! 状態に封をする（認証付き暗号）。
//!
//! 信頼できない相手（ブラウザなど）に状態を預けるとき、`save()` の JSON をそのまま渡すと
//! 乱数の内部状態が見え、改ざんもできてしまう。封をすると、鍵を持つ側だけが開けられ、
//! 1 ビットでも変えれば開けなくなる。
//!
//! 形式: `"sortition1." + base64url(鍵 ID 1 バイト ‖ ナンス 24 バイト ‖ 暗号文 ‖ タグ 16 バイト)`
//! （パディングなし）。XChaCha20-Poly1305 で、認証する付加データは
//! `"libsortition/seal/v1" ‖ 鍵 ID`。ナンスは封をするたびに OS から取る。
//!
//! 鍵 ID は鍵の入れ替え用の目印（0〜255）。`token_key_id` で読んで、対応する鍵を選んでから開く。

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

use crate::session::StateError;
use crate::{set_last_error, SORTITION_ERR_INVALID, SORTITION_ERR_RNG, SORTITION_ERR_STATE, SORTITION_OK};

pub const TOKEN_PREFIX: &str = "sortition1.";
const AAD_PREFIX: &[u8] = b"libsortition/seal/v1";
const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;

pub type Key = [u8; 32];

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn b64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, &b)| n | (b as u32) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

/// パディングなしの base64url を厳密に読む（余りのビットが 0 でなければ拒否する）
fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let mut vals = Vec::with_capacity(s.len());
    for b in s.bytes() {
        vals.push(B64.iter().position(|&c| c == b)? as u32);
    }
    if vals.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(vals.len() * 3 / 4);
    for chunk in vals.chunks(4) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, &v)| n | v << (18 - 6 * i));
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        let take = chunk.len() * 6 / 8;
        out.extend_from_slice(&bytes[..take]);
        if bytes[take..].iter().any(|&b| b != 0) {
            return None;
        }
    }
    Some(out)
}

fn aad(key_id: u8) -> Vec<u8> {
    let mut a = AAD_PREFIX.to_vec();
    a.push(key_id);
    a
}

pub(crate) fn seal_with_nonce(plaintext: &[u8], key: &Key, key_id: u8, nonce: &[u8; NONCE_LEN]) -> Result<String, i32> {
    let cipher = XChaCha20Poly1305::new(key.into());
    let ct = cipher
        .encrypt(XNonce::from_slice(nonce), Payload { msg: plaintext, aad: &aad(key_id) })
        .map_err(|_| crate::SORTITION_ERR_INTERNAL)?;
    let mut raw = Vec::with_capacity(1 + NONCE_LEN + ct.len());
    raw.push(key_id);
    raw.extend_from_slice(nonce);
    raw.extend_from_slice(&ct);
    Ok(format!("{TOKEN_PREFIX}{}", b64_encode(&raw)))
}

/// `plaintext` に封をしてトークンにする。ナンスの取得に失敗すれば `SORTITION_ERR_RNG`。
pub fn seal(plaintext: &[u8], key: &Key, key_id: u8) -> Result<String, i32> {
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::fill(&mut nonce).map_err(|_| SORTITION_ERR_RNG)?;
    seal_with_nonce(plaintext, key, key_id, &nonce)
}

fn parse(token: &str) -> Result<Vec<u8>, StateError> {
    let bad = || StateError("sealed state is malformed".to_string());
    let body = token.strip_prefix(TOKEN_PREFIX).ok_or_else(bad)?;
    let raw = b64_decode(body).ok_or_else(bad)?;
    if raw.len() < 1 + NONCE_LEN + TAG_LEN {
        return Err(bad());
    }
    Ok(raw)
}

/// トークンに記された鍵 ID（開く前に、どの鍵を使うか選ぶため）。形式が不正なら `StateError`。
pub fn token_key_id(token: &str) -> Result<u8, StateError> {
    Ok(parse(token)?[0])
}

/// 封を開く。形式の不正・鍵違い・改ざんはいずれも `StateError`（理由は区別しない）。
pub fn open(token: &str, key: &Key) -> Result<Vec<u8>, StateError> {
    let raw = parse(token)?;
    let key_id = raw[0];
    XChaCha20Poly1305::new(key.into())
        .decrypt(XNonce::from_slice(&raw[1..1 + NONCE_LEN]), Payload { msg: &raw[1 + NONCE_LEN..], aad: &aad(key_id) })
        .map_err(|_| StateError("sealed state cannot be opened (wrong key or tampered)".to_string()))
}

// ---- C ABI ----

/// `key` は 32 バイト。NULL は不可。
pub(crate) unsafe fn key_ref<'a>(key: *const u8) -> Option<&'a Key> {
    if key.is_null() {
        None
    } else {
        Some(&*(key as *const Key))
    }
}

pub(crate) unsafe fn token_arg<'a>(token: *const u8, len: usize) -> Result<&'a str, i32> {
    if token.is_null() && len > 0 {
        set_last_error("token is NULL");
        return Err(SORTITION_ERR_INVALID);
    }
    let bytes: &[u8] = if len == 0 { &[] } else { std::slice::from_raw_parts(token, len) };
    std::str::from_utf8(bytes).map_err(|_| {
        set_last_error("sealed state is malformed");
        SORTITION_ERR_STATE
    })
}

/// C ABI: トークンの鍵 ID を `*out_key_id` に書く。形式が不正なら `SORTITION_ERR_STATE`。
/// 鍵を入れ替えている場合に、開く前にどの鍵を使うか選ぶのに使う。
///
/// # Safety
/// `token` は `len` バイト読める領域、`out_key_id` は書き込み可能な 1 バイトであること。
#[no_mangle]
pub unsafe extern "C" fn sortition_seal_key_id(token: *const u8, len: usize, out_key_id: *mut u8) -> i32 {
    if out_key_id.is_null() {
        return SORTITION_ERR_INVALID;
    }
    let token = match token_arg(token, len) {
        Ok(t) => t,
        Err(e) => return e,
    };
    match token_key_id(token) {
        Ok(id) => {
            *out_key_id = id;
            SORTITION_OK
        }
        Err(e) => {
            set_last_error(&e.0);
            SORTITION_ERR_STATE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: Key = [0x42; 32];

    #[test]
    fn base64url_roundtrip_and_strictness() {
        for len in 0..40usize {
            let data: Vec<u8> = (0..len as u8).map(|i| i.wrapping_mul(37).wrapping_add(250)).collect();
            let enc = b64_encode(&data);
            assert!(!enc.contains(['=', '+', '/']));
            assert_eq!(b64_decode(&enc), Some(data));
        }
        assert_eq!(b64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(b64_encode(b"fo"), "Zm8");
        assert_eq!(b64_decode("Zm8"), Some(b"fo".to_vec()));
        assert_eq!(b64_decode("Zm9="), None); // パディングは不可
        assert_eq!(b64_decode("Zm9"), None); // 余りのビットが 0 でない（"fo" の正しい形は "Zm8"）
        assert_eq!(b64_decode("Z"), None);
        assert_eq!(b64_decode("Zm+/"), None); // 標準版の文字は不可
    }

    #[test]
    fn golden_token_pins_the_format() {
        let token = seal_with_nonce(b"hello", &KEY, 7, &[1u8; NONCE_LEN]).unwrap();
        assert_eq!(
            token,
            "sortition1.BwEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQ-NSNoPVHMMS4D2JiX8zT_pkU5aTw"
        );
        assert_eq!(open(&token, &KEY).unwrap(), b"hello");
        assert_eq!(token_key_id(&token).unwrap(), 7);
    }

    #[test]
    fn roundtrip_random_nonce_and_empty_plaintext() {
        let a = seal(b"state", &KEY, 0).unwrap();
        let b = seal(b"state", &KEY, 0).unwrap();
        assert_ne!(a, b); // ナンスが毎回違う
        assert_eq!(open(&a, &KEY).unwrap(), b"state");
        assert_eq!(open(&b, &KEY).unwrap(), b"state");
        let e = seal(b"", &KEY, 255).unwrap();
        assert_eq!(open(&e, &KEY).unwrap(), b"");
        assert_eq!(token_key_id(&e).unwrap(), 255);
    }

    #[test]
    fn rejects_wrong_key_tampering_and_malformed() {
        let token = seal(b"some secret state", &KEY, 3).unwrap();
        assert!(open(&token, &[0x43; 32]).is_err());

        // どの 1 文字を変えても（別の有効な文字に）開けない
        let body = &token[TOKEN_PREFIX.len()..];
        for i in 0..body.len() {
            let mut chars: Vec<char> = body.chars().collect();
            chars[i] = if chars[i] == 'A' { 'B' } else { 'A' };
            let t = format!("{TOKEN_PREFIX}{}", chars.into_iter().collect::<String>());
            assert!(open(&t, &KEY).is_err(), "byte {i}");
        }
        // 鍵 ID だけ書き換えても（付加データに含むので）開けない
        let mut raw = b64_decode(body).unwrap();
        raw[0] = 4;
        assert!(open(&format!("{TOKEN_PREFIX}{}", b64_encode(&raw)), &KEY).is_err());

        for bad in ["", "sortition1.", "sortition1.AAAA", "sortition2.xxxx", &token[1..], &format!("{token}A")] {
            assert!(open(bad, &KEY).is_err(), "{bad}");
        }
        assert_eq!(token_key_id("nope").map(|_| ()).unwrap_err().0, "sealed state is malformed");
    }
}
