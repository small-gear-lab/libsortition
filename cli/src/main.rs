//! `sortition`: 候補のリストから再現できる抽選をして、記録した抽選を検証するコマンド。
//!
//! 抽選は core の `draw_items`（候補をバイト列の ID にして、ID の昇順に並べてから引く）をそのまま使うので、
//! 各言語のバインディングと同じ結果になり、候補の並び順に依らない。

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use serde_json::{json, Value};

/// 記録（`draw --record` の出力）の形式の名前。形式を変えるときは番号を上げる。
const RECORD_FORMAT: &str = "libsortition-draw-record/1";

#[derive(Parser)]
#[command(
    name = "sortition",
    version,
    about = "Draw winners reproducibly from a list of candidates, and verify a recorded draw",
    after_help = "Exit status: 0 on success, 1 when a verification fails, 2 on a usage or input error."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Draw winners from a list of candidates
    Draw(DrawArgs),
    /// Check a recorded draw by drawing again from the same candidates
    Verify(VerifyArgs),
    /// Show the fingerprint of a set of candidates (it does not depend on their order)
    Fingerprint(FingerprintArgs),
    /// Show the commitment (SHA-256) and the seed number of a seed string
    Commit(CommitArgs),
}

#[derive(Args)]
struct DrawArgs {
    /// Candidates, one per line; '-' or no file reads standard input. Surrounding spaces and a leading BOM are
    /// ignored, blank lines are skipped, and identical lines are separate candidates.
    file: Option<PathBuf>,

    /// The seed string. Its SHA-256 is the commitment; publish that before the draw and reveal the string at the draw
    #[arg(long, value_name = "TEXT", required_unless_present = "random_seed")]
    seed: Option<String>,

    /// Make a random seed string (it is printed to standard error and kept in the record)
    #[arg(long, conflicts_with = "seed")]
    random_seed: bool,

    /// Draw this many winners in one stage
    #[arg(short = 'k', long, value_name = "N", conflicts_with = "stages")]
    count: Option<u32>,

    /// A prize stage, in drawing order, as NAME:COUNT (repeat for more stages). A winner of an earlier stage cannot
    /// win a later one
    #[arg(long = "stage", value_name = "NAME:COUNT")]
    stages: Vec<String>,

    /// Write a record that `sortition verify` can check
    #[arg(short, long, value_name = "FILE")]
    record: Option<PathBuf>,

    /// Print the record as JSON instead of the winners
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct VerifyArgs {
    /// The record written by `sortition draw --record`
    record: PathBuf,

    /// The candidates, in any order ('-' or no file reads standard input)
    file: Option<PathBuf>,

    /// The commitment that was published before the draw; the seed string in the record must hash to it
    #[arg(long, value_name = "HEX")]
    commit: Option<String>,
}

#[derive(Args)]
struct FingerprintArgs {
    /// Candidates, one per line ('-' or no file reads standard input)
    file: Option<PathBuf>,

    /// Also print the ID of every candidate
    #[arg(long)]
    ids: bool,
}

#[derive(Args)]
struct CommitArgs {
    /// The seed string
    seed: String,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::from(2)
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode, String> {
    match cli.command {
        Command::Draw(args) => draw(args),
        Command::Verify(args) => verify(args),
        Command::Fingerprint(args) => fingerprint(args),
        Command::Commit(args) => {
            println!("commit: {}", hex(&sortition::seed_digest(args.seed.as_bytes())));
            println!("seed: {}", sortition::seed_from_text(&args.seed));
            Ok(ExitCode::SUCCESS)
        }
    }
}

// ---- 入力 ----

/// 1 行 1 候補のテキストを読む。前後の空白を除き、空行は捨てる（同じ文字列の行は別の候補）。
fn read_candidates(file: Option<&Path>) -> Result<Vec<String>, String> {
    let mut bytes = Vec::new();
    match file {
        Some(path) if path != Path::new("-") => {
            bytes = fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        }
        _ => {
            io::stdin().lock().read_to_end(&mut bytes).map_err(|e| format!("cannot read standard input: {e}"))?;
        }
    }
    let text = String::from_utf8(bytes).map_err(|_| "the candidates are not valid UTF-8".to_string())?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    Ok(text.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect())
}

struct Stage {
    name: String,
    count: u32,
}

/// `NAME:COUNT`（区切りは半角か全角のコロン。名前にコロンを含むときは最後のコロンで分ける）を読む。
fn parse_stage(spec: &str) -> Result<Stage, String> {
    let bad = || format!("bad stage '{spec}': write it as NAME:COUNT, for example 'Grand prize:1'");
    let (at, sep) = spec.char_indices().rfind(|&(_, c)| c == ':' || c == '：').ok_or_else(bad)?;
    let name = spec[..at].trim();
    let count = spec[at + sep.len_utf8()..].trim().parse::<u32>().map_err(|_| bad())?;
    if name.is_empty() {
        return Err(bad());
    }
    Ok(Stage { name: name.to_string(), count })
}

// ---- 抽選 ----

struct Winner {
    id: [u8; 32],
    item: String,
}

struct StageResult {
    name: String,
    count: u32,
    winners: Vec<Winner>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn as_bytes(items: &[String]) -> Vec<&[u8]> {
    items.iter().map(|s| s.as_bytes()).collect()
}

/// 候補の数と、候補集合の指紋（並び順に依らない）。
fn candidate_summary(items: &[String]) -> (usize, String) {
    let ids = sortition::candidate_ids(&as_bytes(items));
    (items.len(), hex(&sortition::set_fingerprint(&ids)))
}

/// 合計人数を一度に引いて、先頭から賞ごとに切り分ける（`draw` と `verify` で同じ手順を使う）。
fn compute(items: &[String], seed_text: &str, stages: &[Stage]) -> Result<Vec<StageResult>, String> {
    let total: u64 = stages.iter().map(|s| u64::from(s.count)).sum();
    if total > items.len() as u64 {
        return Err(format!("the stages need {total} winners but there are only {} candidates", items.len()));
    }
    let seed = sortition::seed_from_text(seed_text);
    let picks = sortition::draw_items(&as_bytes(items), total as u32, Some(seed))
        .map_err(|code| format!("the draw failed (code {code})"))?;
    let mut at = 0usize;
    Ok(stages
        .iter()
        .map(|s| {
            let winners = picks[at..at + s.count as usize]
                .iter()
                .map(|p| Winner { id: p.id, item: items[p.index as usize].clone() })
                .collect();
            at += s.count as usize;
            StageResult { name: s.name.clone(), count: s.count, winners }
        })
        .collect())
}

fn record(seed_text: &str, items: &[String], results: &[StageResult]) -> Value {
    let (count, fingerprint) = candidate_summary(items);
    json!({
        "format": RECORD_FORMAT,
        "libsortition": sortition::VERSION,
        "seed": {
            "text": seed_text,
            "commit": hex(&sortition::seed_digest(seed_text.as_bytes())),
            "u64": sortition::seed_from_text(seed_text).to_string(),
        },
        "candidates": { "count": count, "fingerprint": fingerprint },
        "stages": results.iter().map(|r| json!({
            "name": r.name,
            "count": r.count,
            "winners": r.winners.iter().map(|w| json!({ "id": hex(&w.id), "item": w.item })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

fn random_seed_text() -> Result<String, String> {
    let mut bytes = [0u8; 12];
    getrandom::fill(&mut bytes).map_err(|e| format!("cannot get random bytes: {e}"))?;
    Ok(hex(&bytes))
}

fn draw(args: DrawArgs) -> Result<ExitCode, String> {
    let stages = match (args.count, args.stages.is_empty()) {
        (Some(k), _) => vec![Stage { name: "winners".to_string(), count: k }],
        (None, false) => args.stages.iter().map(|s| parse_stage(s)).collect::<Result<_, _>>()?,
        (None, true) => return Err("give -k N, or at least one --stage NAME:COUNT".to_string()),
    };
    let seed_text = match args.seed {
        Some(text) => text,
        None => random_seed_text()?,
    };
    let items = read_candidates(args.file.as_deref())?;
    let results = compute(&items, &seed_text, &stages)?;
    let rec = record(&seed_text, &items, &results);
    let pretty = serde_json::to_string_pretty(&rec).expect("a record is JSON") + "\n";
    if let Some(path) = &args.record {
        fs::write(path, &pretty).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    }

    // 検証に要る値は標準エラーに出す（標準出力は結果だけにして、パイプしやすくする）
    eprintln!("seed text:   {seed_text}");
    eprintln!("seed commit: {}", rec["seed"]["commit"].as_str().unwrap_or_default());
    eprintln!(
        "candidates:  {} (fingerprint {})",
        rec["candidates"]["count"],
        rec["candidates"]["fingerprint"].as_str().unwrap_or_default()
    );

    if args.json {
        print!("{pretty}");
    } else if args.count.is_some() {
        for w in &results[0].winners {
            println!("{}", w.item);
        }
    } else {
        for r in &results {
            println!("{}", r.name);
            for (i, w) in r.winners.iter().enumerate() {
                println!("  {}. {}", i + 1, w.item);
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

// ---- 検証 ----

fn at<'a>(v: &'a Value, path: &[&str]) -> Result<&'a Value, String> {
    path.iter().try_fold(v, |v, key| v.get(key)).ok_or_else(|| format!("the record has no \"{}\"", path.join(".")))
}

fn str_at<'a>(v: &'a Value, path: &[&str]) -> Result<&'a str, String> {
    at(v, path)?.as_str().ok_or_else(|| format!("\"{}\" in the record must be a string", path.join(".")))
}

fn u64_at(v: &Value, path: &[&str]) -> Result<u64, String> {
    at(v, path)?.as_u64().ok_or_else(|| format!("\"{}\" in the record must be a non-negative integer", path.join(".")))
}

fn verify(args: VerifyArgs) -> Result<ExitCode, String> {
    let text = fs::read_to_string(&args.record).map_err(|e| format!("cannot read {}: {e}", args.record.display()))?;
    let rec: Value = serde_json::from_str(&text).map_err(|e| format!("the record is not valid JSON: {e}"))?;
    if str_at(&rec, &["format"])? != RECORD_FORMAT {
        return Err(format!("unsupported record format (this tool reads {RECORD_FORMAT})"));
    }
    let seed_text = str_at(&rec, &["seed", "text"])?;
    let recorded_commit = str_at(&rec, &["seed", "commit"])?;
    let recorded_seed = str_at(&rec, &["seed", "u64"])?;
    let recorded_count = u64_at(&rec, &["candidates", "count"])?;
    let recorded_fingerprint = str_at(&rec, &["candidates", "fingerprint"])?;
    let recorded_stages = at(&rec, &["stages"])?.as_array().ok_or("\"stages\" in the record must be an array")?;
    let mut plan = Vec::new();
    for (i, s) in recorded_stages.iter().enumerate() {
        let count = u32::try_from(u64_at(s, &["count"])?).map_err(|_| format!("stages[{i}].count is too large"))?;
        plan.push(Stage { name: str_at(s, &["name"])?.to_string(), count });
    }

    let items = read_candidates(args.file.as_deref())?;
    let mut problems: Vec<String> = Vec::new();

    let digest = hex(&sortition::seed_digest(seed_text.as_bytes()));
    if digest != recorded_commit {
        problems.push("the seed text does not hash to the commitment in the record".to_string());
    }
    if sortition::seed_from_text(seed_text).to_string() != recorded_seed {
        problems.push("the seed number in the record does not follow from the seed text".to_string());
    }
    if let Some(expected) = &args.commit {
        if !expected.trim().eq_ignore_ascii_case(&digest) {
            problems.push("the commitment published beforehand does not match the seed text".to_string());
        }
    }

    let (count, fingerprint) = candidate_summary(&items);
    let mut same_candidates = true;
    if count as u64 != recorded_count {
        problems.push(format!("the record has {recorded_count} candidates but the list has {count}"));
        same_candidates = false;
    }
    if fingerprint != recorded_fingerprint {
        problems.push("the candidates are not the ones in the record (the fingerprints differ)".to_string());
        same_candidates = false;
    }

    // 候補が違えば結果が違うのは当然なので、同じ候補のときだけ引き直して比べる
    if same_candidates {
        match compute(&items, seed_text, &plan) {
            Err(e) => problems.push(e),
            Ok(results) => {
                for (i, (mine, theirs)) in results.iter().zip(recorded_stages).enumerate() {
                    let recorded: Vec<(&str, &str)> = theirs
                        .get("winners")
                        .and_then(Value::as_array)
                        .map(|ws| {
                            ws.iter()
                                .map(|w| (w["id"].as_str().unwrap_or(""), w["item"].as_str().unwrap_or("")))
                                .collect()
                        })
                        .unwrap_or_default();
                    let ids: Vec<String> = mine.winners.iter().map(|w| hex(&w.id)).collect();
                    let same = recorded.len() == mine.winners.len()
                        && recorded.iter().zip(&mine.winners).zip(&ids).all(|(((rid, ritem), w), id)| {
                            *rid == id.as_str() && *ritem == w.item
                        });
                    if !same {
                        problems.push(format!("the winners of stage {} ('{}') differ from the record", i + 1, mine.name));
                    }
                }
            }
        }
    }

    if problems.is_empty() {
        let winners: u64 = plan.iter().map(|s| u64::from(s.count)).sum();
        println!("OK: the record matches ({count} candidates, {winners} winners in {} stages)", plan.len());
        Ok(ExitCode::SUCCESS)
    } else {
        for p in &problems {
            eprintln!("MISMATCH: {p}");
        }
        println!("NOT VERIFIED");
        Ok(ExitCode::from(1))
    }
}

fn fingerprint(args: FingerprintArgs) -> Result<ExitCode, String> {
    let items = read_candidates(args.file.as_deref())?;
    let (count, fingerprint) = candidate_summary(&items);
    println!("candidates: {count}");
    println!("fingerprint: {fingerprint}");
    if args.ids {
        for (id, item) in sortition::candidate_ids(&as_bytes(&items)).iter().zip(&items) {
            println!("{}  {item}", hex(id));
        }
    }
    Ok(ExitCode::SUCCESS)
}
