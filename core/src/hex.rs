//! 小文字 16 進の変換（ID・鍵・指紋の表記に使う）。

pub fn encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 長さがちょうど `N` バイト分で、16 進（大小どちらも可）として正しいときだけ `Some`。
pub fn decode<const N: usize>(s: &str) -> Option<[u8; N]> {
    if s.len() != 2 * N || !s.is_ascii() {
        return None;
    }
    let mut out = [0u8; N];
    for (slot, pair) in out.iter_mut().zip(s.as_bytes().chunks(2)) {
        *slot = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_rejects() {
        assert_eq!(encode(&[0x00, 0xab, 0xff]), "00abff");
        assert_eq!(decode::<3>("00abff"), Some([0x00, 0xab, 0xff]));
        assert_eq!(decode::<3>("00ABFF"), Some([0x00, 0xab, 0xff]));
        assert_eq!(decode::<3>("00abf"), None);
        assert_eq!(decode::<3>("00abfg"), None);
        assert_eq!(decode::<3>("00ab\u{e9}f"), None);
    }
}
