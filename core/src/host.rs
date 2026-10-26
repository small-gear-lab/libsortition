//! ホスト環境（特に WASM）向けの補助。
//!
//! - `sortition_alloc` / `sortition_dealloc`: ホストがリニアメモリへ直接書き込むためのバッファ。
//! - `wasm_rng`: `wasm32-unknown-unknown` には OS の乱数源がないので、ホスト（JS）が import で
//!   渡す `env.sortition_random_fill` に委ねる（`getrandom` の custom backend）。

/// ホスト側から呼び出し用のバッファを確保する（WASM のようにホストがリニアメモリへ
/// 直接書き込む環境向け）。`sortition_dealloc(ptr, len)` で同じ `len` を渡して解放する。
#[doc(hidden)]
#[no_mangle]
pub extern "C" fn sortition_alloc(len: usize) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(len.max(1));
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// `sortition_alloc` で確保したバッファを解放する。NULL は無視する。
///
/// # Safety
/// `ptr` は `sortition_alloc(len)` が返した未解放のポインタ（`len` も同じ値）か NULL であること。
#[doc(hidden)]
#[no_mangle]
pub unsafe extern "C" fn sortition_dealloc(ptr: *mut u8, len: usize) {
    if !ptr.is_null() {
        drop(Vec::from_raw_parts(ptr, 0, len.max(1)));
    }
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown", getrandom_backend = "custom"))]
mod wasm_rng {
    #[link(wasm_import_module = "env")]
    extern "C" {
        /// import 名は `env.sortition_random_fill`。`ptr` から `len` バイトを乱数で埋める。
        fn sortition_random_fill(ptr: *mut u8, len: usize);
    }

    #[no_mangle]
    unsafe extern "Rust" fn __getrandom_v03_custom(
        dest: *mut u8,
        len: usize,
    ) -> Result<(), getrandom::Error> {
        sortition_random_fill(dest, len);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_roundtrip() {
        unsafe {
            let p = sortition_alloc(16);
            assert!(!p.is_null());
            std::ptr::write_bytes(p, 0xAB, 16);
            sortition_dealloc(p, 16);
            let z = sortition_alloc(0);
            assert!(!z.is_null());
            sortition_dealloc(z, 0);
            sortition_dealloc(std::ptr::null_mut(), 0);
        }
    }
}
