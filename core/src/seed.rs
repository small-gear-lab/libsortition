//! 文字列から seed を決める。
//!
//! seed の文字列の SHA-256 が「コミット値」で、seed（`u64`）はそのダイジェストの先頭 8 バイトを
//! リトルエンディアンで読んだ値。抽選の前にコミット値だけを公開しておき、抽選の直前に文字列を明かすと、
//! あとから seed を選び直していないことを示せる。全バインディングと CLI が同じ規則を使う。

use sha2::{Digest, Sha256};

use crate::{SORTITION_ERR_INVALID, SORTITION_OK};

/// The SHA-256 of a seed string (its bytes). This is the commitment to publish before the draw.
pub fn seed_digest(text: &[u8]) -> [u8; 32] {
    Sha256::digest(text).into()
}

/// The seed (a `u64`) of a seed string: the first 8 bytes of its SHA-256, read as little-endian.
pub fn seed_from_text(text: &str) -> u64 {
    seed_from_digest(&seed_digest(text.as_bytes()))
}

fn seed_from_digest(digest: &[u8; 32]) -> u64 {
    u64::from_le_bytes(digest[..8].try_into().expect("8 bytes"))
}

/// C ABI: seed の文字列（バイト列。UTF-8 でなくてもよい）から seed を決めて `*out_seed` に書く。
/// `out_digest` が NULL でなければ、SHA-256（コミット値、32 バイト）も書く。
///
/// # Safety
/// `text` は `len` バイト読める領域、`out_seed` は書き込み可能な `u64`、
/// `out_digest` は NULL か 32 バイト書ける領域であること。
#[doc(hidden)]
#[no_mangle]
pub unsafe extern "C" fn sortition_seed_from_text(
    text: *const u8,
    len: usize,
    out_seed: *mut u64,
    out_digest: *mut u8,
) -> i32 {
    if out_seed.is_null() || (text.is_null() && len > 0) {
        return SORTITION_ERR_INVALID;
    }
    let bytes: &[u8] = if len == 0 { &[] } else { std::slice::from_raw_parts(text, len) };
    let digest = seed_digest(bytes);
    *out_seed = seed_from_digest(&digest);
    if !out_digest.is_null() {
        std::ptr::copy_nonoverlapping(digest.as_ptr(), out_digest, 32);
    }
    SORTITION_OK
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hex;

    #[test]
    fn known_values() {
        // printf sortition | sha256sum
        assert_eq!(
            hex::encode(&seed_digest(b"sortition")),
            "80ae496fd3b5d201bb96339356ff252e2b1cdc214ad567f6ebe8de4537e6b31c"
        );
        assert_eq!(seed_from_text("sortition"), 131367258856992384);
        // 空の文字列も有効（SHA-256("") の先頭 8 バイト e3b0c44298fc1c14 をリトルエンディアンで読む）
        assert_eq!(seed_from_text(""), 0x141c_fc98_42c4_b0e3);
        assert_ne!(seed_from_text("a"), seed_from_text("b"));
    }

    #[test]
    fn c_abi() {
        unsafe {
            let (mut seed, mut digest) = (0u64, [0u8; 32]);
            let t = "日本語の seed";
            assert_eq!(sortition_seed_from_text(t.as_ptr(), t.len(), &mut seed, digest.as_mut_ptr()), SORTITION_OK);
            assert_eq!(seed, seed_from_text(t));
            assert_eq!(digest, seed_digest(t.as_bytes()));
            // digest は NULL でもよい。UTF-8 でないバイト列も、そのまま扱う
            assert_eq!(sortition_seed_from_text([0xff, 0xfe].as_ptr(), 2, &mut seed, std::ptr::null_mut()), SORTITION_OK);
            assert_eq!(seed, u64::from_le_bytes(seed_digest(&[0xff, 0xfe])[..8].try_into().unwrap()));
            assert_eq!(sortition_seed_from_text(std::ptr::null(), 0, &mut seed, std::ptr::null_mut()), SORTITION_OK);
            assert_eq!(seed, seed_from_text(""));
            assert_eq!(sortition_seed_from_text(std::ptr::null(), 1, &mut seed, std::ptr::null_mut()), SORTITION_ERR_INVALID);
            assert_eq!(sortition_seed_from_text(t.as_ptr(), t.len(), std::ptr::null_mut(), std::ptr::null_mut()), SORTITION_ERR_INVALID);
        }
    }
}
