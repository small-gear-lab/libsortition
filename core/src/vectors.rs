//! 全言語共通のテストベクトル（`test-vectors/vectors.json`）を、実装の出力から作る。
//!
//! ファイルは Rust・Python・Ruby・Node.js・WebAssembly・PHP のテストと、仕様書だけから書いた参照実装
//! （`test-vectors/reference.py`）が読み、同じ値になることを確かめる。結果が変わる変更をしたら、
//! このテストが落ちる。意図した変更なら `docs/SPEC.md` と `ALGORITHM` の名前を見直したうえで、
//! `SORTITION_UPDATE_VECTORS=1 cargo test -p sortition vectors` でファイルを作り直す。

use serde_json::{json, Value};

use crate::hex;
use crate::seal::seal_with_nonce;
use crate::{
    candidate_ids, draw, draw_items, seed_digest, seed_from_text, set_fingerprint, ChaChaRng, Session, Source,
    Xoshiro256, ALGORITHM,
};

fn h(bytes: &[u8]) -> String {
    hex::encode(bytes)
}

/// 64 ビットの値は、JS で丸められないよう 10 進文字列にする
fn u(v: u64) -> String {
    v.to_string()
}

fn text(s: &str) -> Vec<u8> {
    s.as_bytes().to_vec()
}

fn item_sets() -> Vec<(&'static str, Vec<Vec<u8>>)> {
    vec![
        ("names", ["alice", "bob", "carol", "dave", "erin", "frank", "grace", "heidi"].iter().map(|s| text(s)).collect()),
        (
            "duplicates-and-binary",
            vec![text("x"), text("x"), vec![], vec![], vec![0xff, 0x00, 0xfe], text("x")],
        ),
        ("unicode", vec![text("日本語"), text("日本語 "), text("é"), text("e\u{301}")]),
    ]
}

fn refs(set: &[Vec<u8>]) -> Vec<&[u8]> {
    set.iter().map(Vec::as_slice).collect()
}

fn picks_json(picks: &[crate::Pick]) -> Value {
    Value::Array(picks.iter().map(|p| json!({ "index": p.index, "id": h(&p.id) })).collect())
}

fn chacha_state(key: [u8; 32], n: u32) -> Value {
    json!({
        "version": 3,
        "id": "00112233445566778899aabbccddeeff",
        "revision": 0,
        "calls": [],
        "n": n,
        "rng": { "kind": "chacha20", "key": h(&key), "pos": "0" },
        "drawn": [],
        "swapped": [],
    })
}

pub(crate) fn build() -> Value {
    let sets = item_sets();

    let seed_texts = ["sortition", "", "stages", "日本語の seed", "a", "demo-seed-1"];
    let seed_vectors: Vec<Value> = seed_texts
        .iter()
        .map(|t| json!({ "text": t, "digest": h(&seed_digest(t.as_bytes())), "seed": u(seed_from_text(t)) }))
        .collect();

    let xoshiro: Vec<Value> = [0u64, 1, 42, 131367258856992384, u64::MAX]
        .iter()
        .map(|&seed| {
            let mut r = Xoshiro256::from_seed(seed);
            let state: Vec<String> = r.s.iter().map(|&v| u(v)).collect();
            let outputs: Vec<String> = (0..8).map(|_| u(r.next_u64())).collect();
            json!({ "seed": u(seed), "state": state, "outputs": outputs })
        })
        .collect();

    let below: Vec<Value> = [1u64, 2, 3, 10, 1_000_000_007, 4_294_967_296, 0x9000_0000_0000_0001, u64::MAX]
        .iter()
        .map(|&bound| {
            let mut src = Source::Seeded(Xoshiro256::from_seed(1));
            let values: Vec<String> = (0..16).map(|_| u(src.below(bound).unwrap())).collect();
            json!({ "seed": u(1), "bound": u(bound), "values": values })
        })
        .collect();

    let draws: Vec<Value> = [
        (10u32, 3u32, 1u64),
        (10, 10, 7),
        (1000, 10, 42),
        (1, 1, 0),
        (100, 0, 5),
        (4_294_967_295, 5, 9),
        (50, 25, u64::MAX),
        (200, 50, 9),
    ]
    .iter()
    .map(|&(n, k, seed)| json!({ "n": n, "k": k, "seed": u(seed), "result": draw(n, k, Some(seed)).unwrap() }))
    .collect();

    let item_set_vectors: Vec<Value> = sets
        .iter()
        .map(|(name, items)| {
            let ids = candidate_ids(&refs(items));
            json!({
                "name": name,
                "items_hex": items.iter().map(|i| h(i)).collect::<Vec<_>>(),
                "ids": ids.iter().map(|id| h(id)).collect::<Vec<_>>(),
                "fingerprint": h(&set_fingerprint(&ids)),
            })
        })
        .collect();

    let draw_item_cases: Vec<Value> = [(0usize, 5u32, 7u64), (0, 8, 1), (1, 4, 3), (2, 2, 11), (1, 0, 5)]
        .iter()
        .map(|&(set, k, seed)| {
            let picks = draw_items(&refs(&sets[set].1), k, Some(seed)).unwrap();
            json!({ "set": sets[set].0, "k": k, "seed": u(seed), "winners": picks_json(&picks) })
        })
        .collect();

    let stage_cases: Vec<Value> = [
        (0usize, "stages", vec![("副賞", 3u32), ("大賞", 1)]),
        (1, "dup", vec![("a", 2), ("空", 0), ("b", 2)]),
    ]
    .iter()
    .map(|(set, seed_text, stages)| {
        let total: u32 = stages.iter().map(|s| s.1).sum();
        let picks = draw_items(&refs(&sets[*set].1), total, Some(seed_from_text(seed_text))).unwrap();
        let mut at = 0usize;
        let cut: Vec<Value> = stages
            .iter()
            .map(|(name, count)| {
                let winners = picks_json(&picks[at..at + *count as usize]);
                at += *count as usize;
                json!({ "name": name, "count": count, "winners": winners })
            })
            .collect();
        json!({ "set": sets[*set].0, "seed_text": seed_text, "stages": cut })
    })
    .collect();

    let chacha_stream: Vec<Value> = [([0u8; 32], 0u64), ([7; 32], 0), ([7; 32], 56), ([7; 32], 800)]
        .iter()
        .map(|&(key, pos)| {
            let mut r = ChaChaRng { key, pos };
            let outputs: Vec<String> = (0..8).map(|_| u(r.next_u64().unwrap())).collect();
            json!({ "key": h(&key), "pos": u(pos), "outputs": outputs })
        })
        .collect();

    // ChaCha20 の状態を持つセッションを読み込んで、続けて引く
    let session_state = chacha_state([0x11; 32], 20);
    let mut session = Session::load(&session_state.to_string()).unwrap();
    let session_draws = [3u32, 4, 5];
    let session_expected: Vec<Vec<u32>> = session_draws.iter().map(|&k| session.draw(k).unwrap()).collect();
    let chacha_session = json!({ "state": session_state, "draws": session_draws, "expected": session_expected });

    // 封をしたトークン（ナンスを固定して作る）。開いて引くと、同じ結果になる
    let (key, key_id, nonce) = ([0x42u8; 32], 7u8, [1u8; 24]);
    let plaintext = Session::load(&chacha_state([0x22; 32], 20).to_string()).unwrap().save();
    let token = seal_with_nonce(plaintext.as_bytes(), &key, key_id, &nonce).unwrap();
    let mut sealed_session = Session::load(&plaintext).unwrap();
    let sealed_draws = [3u32, 4];
    let sealed_expected: Vec<Vec<u32>> = sealed_draws.iter().map(|&k| sealed_session.draw(k).unwrap()).collect();
    let seal = json!({
        "key": h(&key),
        "key_id": key_id,
        "nonce": h(&nonce),
        "plaintext": plaintext,
        "token": token,
        "draws": sealed_draws,
        "expected": sealed_expected,
    });

    json!({
        "format": "libsortition-test-vectors/1",
        "algorithm": ALGORITHM,
        "seed_from_text": seed_vectors,
        "xoshiro256": xoshiro,
        "below": below,
        "draw": draws,
        "item_sets": item_set_vectors,
        "draw_items": draw_item_cases,
        "stages": stage_cases,
        "chacha20_stream": chacha_stream,
        "chacha20_session": chacha_session,
        "seal": seal,
    })
}

#[test]
fn test_vectors_are_up_to_date() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../test-vectors/vectors.json");
    let fresh = serde_json::to_string_pretty(&build()).unwrap() + "\n";
    if std::env::var_os("SORTITION_UPDATE_VECTORS").is_some() {
        std::fs::create_dir_all(std::path::Path::new(path).parent().unwrap()).unwrap();
        std::fs::write(path, &fresh).unwrap();
        return;
    }
    match std::fs::read_to_string(path) {
        Ok(existing) => assert!(
            existing == fresh,
            "test-vectors/vectors.json differs from what the code gives. If the change is intended, review \
             docs/SPEC.md and ALGORITHM, then run: SORTITION_UPDATE_VECTORS=1 cargo test -p sortition vectors"
        ),
        // crates.io から取った crate には、リポジトリのファイルがない
        Err(_) => eprintln!("skipped: {path} is not available (not a checkout of the repository)"),
    }
}

#[test]
fn the_vectors_are_deterministic_and_cover_the_rejection_branch() {
    assert_eq!(build(), build());
    // 0x9000… を超える上限では、生の乱数の約 44% が棄却される。仕様書の書き方（zone 以下だけを採る）で
    // 手で数えた値と、`below` の出力が一致し、棄却が実際に起きていること
    let bound = 0x9000_0000_0000_0001u64;
    let zone = u64::MAX - (u64::MAX % bound + 1) % bound;
    let mut raw = Xoshiro256::from_seed(1);
    let (mut expected, mut rejected) = (Vec::new(), 0);
    while expected.len() < 16 {
        let v = raw.next_u64();
        if v <= zone {
            expected.push(v % bound);
        } else {
            rejected += 1;
        }
    }
    assert!(rejected > 0, "the rejection branch was not exercised");
    let mut src = Source::Seeded(Xoshiro256::from_seed(1));
    assert_eq!((0..16).map(|_| src.below(bound).unwrap()).collect::<Vec<_>>(), expected);
}
