//! `sortition` コマンドのテスト（作ったバイナリを実際に動かす）。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const NAMES: &str = "alice\nbob\ncarol\ndave\nerin\nfrank\ngrace\nheidi\n";

fn run(args: &[&str], stdin: Option<&str>) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_sortition"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary starts");
    if let Some(text) = stdin {
        child.stdin.take().unwrap().write_all(text.as_bytes()).unwrap();
    } else {
        drop(child.stdin.take());
    }
    child.wait_with_output().unwrap()
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn code(o: &Output) -> i32 {
    o.status.code().expect("exit code")
}

/// テストごとの一時ディレクトリ
fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sortition-cli-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write(dir: &Path, name: &str, text: &str) -> String {
    let p = dir.join(name);
    std::fs::write(&p, text).unwrap();
    p.to_string_lossy().into_owned()
}

#[test]
fn commit_prints_the_digest_and_the_seed_number() {
    let o = run(&["commit", "sortition"], None);
    assert_eq!(code(&o), 0);
    assert_eq!(
        out(&o),
        "commit: 80ae496fd3b5d201bb96339356ff252e2b1cdc214ad567f6ebe8de4537e6b31c\nseed: 131367258856992384\n"
    );
}

#[test]
fn fingerprint_ignores_order_blank_lines_and_spaces() {
    let d = dir("fingerprint");
    let a = write(&d, "a.txt", NAMES);
    let b = write(&d, "b.txt", "\n  heidi \ngrace\nfrank\n\nerin\ndave\ncarol\nbob\nalice");
    let (oa, ob) = (run(&["fingerprint", &a], None), run(&["fingerprint", &b], None));
    assert_eq!(code(&oa), 0);
    assert_eq!(out(&oa), out(&ob));
    assert!(out(&oa).starts_with("candidates: 8\nfingerprint: "));
    // 候補が違えば指紋も違う。標準入力からも読める
    let other = run(&["fingerprint"], Some("alice\nbob\n"));
    assert_ne!(out(&other), out(&oa));
    let ids = run(&["fingerprint", "--ids", &a], None);
    assert_eq!(out(&ids).lines().count(), 2 + 8);
}

#[test]
fn draw_matches_the_library_and_other_bindings() {
    let d = dir("draw");
    let file = write(&d, "names.txt", NAMES);
    let o = run(&["draw", &file, "--seed", "stages", "-k", "4"], None);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let winners: Vec<String> = out(&o).lines().map(str::to_string).collect();
    // Python / Ruby / Node.js / WebAssembly / PHP でも、同じ候補と seed ("stages") で同じ順に当選する
    assert_eq!(winners, ["carol", "heidi", "bob", "erin"]);
    // ライブラリを直接呼んだ結果とも一致する
    let items: Vec<&[u8]> = NAMES.lines().map(str::as_bytes).collect();
    let picks = sortition::draw_items(&items, 4, Some(sortition::seed_from_text("stages"))).unwrap();
    let lib: Vec<String> = picks.iter().map(|p| NAMES.lines().nth(p.index as usize).unwrap().to_string()).collect();
    assert_eq!(winners, lib);
    // 標準出力は結果だけで、検証に要る値は標準エラーに出る
    assert!(err(&o).contains("seed commit: 7a77c515879a3aedbe869742dd4e9a4700886ad7b3cc4f8bd86c0356e4b65499"));
    // 同じ入力なら何度でも同じ。候補の並びが違っても同じ候補が当選する
    let again = run(&["draw", "--seed", "stages", "-k", "4"], Some("heidi\ngrace\nfrank\nerin\ndave\ncarol\nbob\nalice\n"));
    assert_eq!(out(&again).lines().collect::<Vec<_>>(), winners);
}

#[test]
fn stages_are_drawn_in_order_and_do_not_overlap() {
    let d = dir("stages");
    let file = write(&d, "names.txt", NAMES);
    let o = run(&["draw", &file, "--seed", "stages", "--stage", "副賞:3", "--stage", "大賞：1"], None);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert_eq!(out(&o), "副賞\n  1. carol\n  2. heidi\n  3. bob\n大賞\n  1. erin\n");
    // 名前にコロンを含んでもよい（最後のコロンで分ける）
    let o = run(&["draw", &file, "--seed", "x", "--stage", "a:b:2"], None);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(out(&o).starts_with("a:b\n"));
}

#[test]
fn a_record_can_be_verified_and_a_changed_input_cannot() {
    let d = dir("verify");
    let file = write(&d, "names.txt", NAMES);
    let record = d.join("record.json").to_string_lossy().into_owned();
    let o = run(&["draw", &file, "--seed", "stages", "--stage", "副賞:3", "--stage", "大賞:1", "-r", &record], None);
    assert_eq!(code(&o), 0, "{}", err(&o));

    let ok = run(&["verify", &record, &file], None);
    assert_eq!(code(&ok), 0, "{}", err(&ok));
    assert!(out(&ok).starts_with("OK: the record matches (8 candidates, 4 winners in 2 stages)"));
    // 候補の並びが違っても、標準入力からでも、同じ結果になるので検証できる
    let reordered = run(&["verify", &record], Some("heidi\ngrace\nfrank\nerin\ndave\ncarol\nbob\nalice\n"));
    assert_eq!(code(&reordered), 0, "{}", err(&reordered));
    // 事前に公開したコミット値の照合
    let commit = "7a77c515879a3aedbe869742dd4e9a4700886ad7b3cc4f8bd86c0356e4b65499";
    assert_eq!(code(&run(&["verify", &record, &file, "--commit", commit], None)), 0);
    let wrong = run(&["verify", &record, &file, "--commit", "00ff"], None);
    assert_eq!(code(&wrong), 1);
    assert!(err(&wrong).contains("published beforehand"));

    // 候補が 1 つ増えた / 1 つ入れ替わった
    let more = run(&["verify", &record], Some(&format!("{NAMES}ivan\n")));
    assert_eq!(code(&more), 1);
    assert!(err(&more).contains("9") && err(&more).contains("fingerprints differ"));
    let swapped = run(&["verify", &record], Some(&NAMES.replace("alice", "mallory")));
    assert_eq!(code(&swapped), 1);
    assert!(out(&swapped).contains("NOT VERIFIED"));

    // 記録の書き換え: 当選者、seed の文字列、形式
    let text = std::fs::read_to_string(&record).unwrap();
    let mut json: serde_json::Value = serde_json::from_str(&text).unwrap();
    json["stages"][1]["winners"][0]["item"] = "alice".into();
    let forged = write(&d, "forged.json", &json.to_string());
    let o = run(&["verify", &forged, &file], None);
    assert_eq!(code(&o), 1);
    assert!(err(&o).contains("stage 2"), "{}", err(&o));

    let mut json: serde_json::Value = serde_json::from_str(&text).unwrap();
    json["seed"]["text"] = "another seed".into();
    let o = run(&["verify", &write(&d, "seed.json", &json.to_string()), &file], None);
    assert_eq!(code(&o), 1);
    assert!(err(&o).contains("does not hash to the commitment"));

    let mut json: serde_json::Value = serde_json::from_str(&text).unwrap();
    json["format"] = "something-else/9".into();
    let o = run(&["verify", &write(&d, "format.json", &json.to_string()), &file], None);
    assert_eq!(code(&o), 2);
    assert!(err(&o).contains("unsupported record format"));
    assert_eq!(code(&run(&["verify", &write(&d, "bad.json", "{not json"), &file], None)), 2);
}

#[test]
fn a_random_seed_is_kept_in_the_record() {
    let d = dir("random");
    let file = write(&d, "names.txt", NAMES);
    let o = run(&["draw", &file, "--random-seed", "-k", "2", "--json"], None);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let rec: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    assert_eq!(rec["format"], "libsortition-draw-record/1");
    let seed = rec["seed"]["text"].as_str().unwrap();
    assert_eq!(seed.len(), 24);
    assert!(err(&o).contains(seed));
    assert_eq!(rec["stages"][0]["winners"].as_array().unwrap().len(), 2);
    let path = write(&d, "record.json", &out(&o));
    assert_eq!(code(&run(&["verify", &path, &file], None)), 0);
    // 2 回続けると、別の seed になる
    let second = run(&["draw", &file, "--random-seed", "-k", "2", "--json"], None);
    let rec2: serde_json::Value = serde_json::from_str(&out(&second)).unwrap();
    assert_ne!(rec2["seed"]["text"], rec["seed"]["text"]);
}

#[test]
fn mistakes_are_reported_with_exit_status_2() {
    let d = dir("errors");
    let file = write(&d, "names.txt", NAMES);
    for (args, needle) in [
        (vec!["draw", file.as_str(), "-k", "1"], "--seed"),                                       // seed がない
        (vec!["draw", file.as_str(), "--seed", "s"], "-k N"),                                     // 人数がない
        (vec!["draw", file.as_str(), "--seed", "s", "-k", "9"], "only 8 candidates"),             // 候補より多い
        (vec!["draw", file.as_str(), "--seed", "s", "--stage", "a:5", "--stage", "b:4"], "need 9 winners"),
        (vec!["draw", file.as_str(), "--seed", "s", "--stage", "no colon"], "bad stage"),
        (vec!["draw", file.as_str(), "--seed", "s", "--stage", ":3"], "bad stage"),
        (vec!["draw", file.as_str(), "--seed", "s", "--stage", "a:-1"], "bad stage"),
        (vec!["draw", file.as_str(), "--seed", "s", "-k", "1", "--stage", "a:1"], "cannot be used with"),
        (vec!["draw", file.as_str(), "--seed", "s", "--random-seed", "-k", "1"], "cannot be used with"),
        (vec!["draw", "/nonexistent/names.txt", "--seed", "s", "-k", "1"], "cannot read"),
    ] {
        let o = run(&args, None);
        assert_eq!(code(&o), 2, "{args:?}: {}", err(&o));
        assert!(err(&o).contains(needle), "{args:?}: {}", err(&o));
    }
    // UTF-8 でない入力
    let bin = d.join("bin.txt");
    std::fs::write(&bin, [0xff, 0xfe, b'\n']).unwrap();
    let o = run(&["draw", bin.to_str().unwrap(), "--seed", "s", "-k", "1"], None);
    assert_eq!(code(&o), 2);
    assert!(err(&o).contains("not valid UTF-8"));
    // 候補が 0 件のとき、0 人なら引ける
    assert_eq!(code(&run(&["draw", "--seed", "s", "-k", "0"], Some("\n\n"))), 0);
    assert_eq!(code(&run(&["draw", "--seed", "s", "-k", "1"], Some("\n\n"))), 2);
}

#[test]
fn a_leading_bom_and_windows_line_endings_are_ignored() {
    let plain = run(&["fingerprint"], Some(NAMES));
    let windows = run(&["fingerprint"], Some(&format!("\u{feff}{}", NAMES.replace('\n', "\r\n"))));
    assert_eq!(out(&plain), out(&windows));
}

#[test]
fn help_and_version_work() {
    let o = run(&["--help"], None);
    assert_eq!(code(&o), 0);
    assert!(out(&o).contains("Draw winners reproducibly"));
    assert!(out(&o).contains("verify"));
    assert!(out(&run(&["--version"], None)).starts_with("sortition "));
}
