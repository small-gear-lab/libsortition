//! 構造化データ（JSON / YAML / TOML）の配列から抽選する。
//!
//! どの形式も `serde_json::Value` に正規化し、選ばれた要素を JSON 配列として返す。

#[cfg(any(feature = "toml", feature = "yaml"))]
use serde_json::{Map, Number};
use serde_json::Value;

use crate::{
    draw, SORTITION_ERR_FORMAT, SORTITION_ERR_INVALID, SORTITION_ERR_PARSE, SORTITION_ERR_PATH,
};

pub const SORTITION_FORMAT_JSON: i32 = 0;
pub const SORTITION_FORMAT_YAML: i32 = 1;
pub const SORTITION_FORMAT_TOML: i32 = 2;

/// 入れ子の深さの上限（スタックを守るため）
#[cfg(feature = "yaml")]
const MAX_DEPTH: usize = 128;

/// An error code (`SORTITION_ERR_*`) with a message for people to read.
#[derive(Debug)]
pub struct DataError {
    pub code: i32,
    pub msg: String,
}

fn err<T>(code: i32, msg: impl Into<String>) -> Result<T, DataError> {
    Err(DataError { code, msg: msg.into() })
}

/// NaN / Infinity は JSON の数値にできないので文字列にする
#[cfg(any(feature = "toml", feature = "yaml"))]
fn float_value(f: f64) -> Value {
    Number::from_f64(f).map(Value::Number).unwrap_or_else(|| Value::String(f.to_string()))
}

fn parse(format: i32, text: &str) -> Result<Value, DataError> {
    match format {
        SORTITION_FORMAT_JSON => {
            serde_json::from_str(text).or_else(|e| err(SORTITION_ERR_PARSE, format!("JSON: {e}")))
        }
        SORTITION_FORMAT_YAML => parse_yaml(text),
        SORTITION_FORMAT_TOML => parse_toml(text),
        _ => err(SORTITION_ERR_FORMAT, format!("unknown format {format}")),
    }
}

#[cfg(feature = "toml")]
fn parse_toml(text: &str) -> Result<Value, DataError> {
    fn convert(v: toml::Value) -> Value {
        match v {
            toml::Value::String(s) => Value::String(s),
            toml::Value::Integer(i) => Value::from(i),
            toml::Value::Float(f) => float_value(f),
            toml::Value::Boolean(b) => Value::Bool(b),
            // JSON に日時型はないので RFC 3339 の文字列にする
            toml::Value::Datetime(d) => Value::String(d.to_string()),
            toml::Value::Array(a) => Value::Array(a.into_iter().map(convert).collect()),
            toml::Value::Table(t) => {
                Value::Object(t.into_iter().map(|(k, v)| (k, convert(v))).collect::<Map<_, _>>())
            }
        }
    }
    match text.parse::<toml::Table>() {
        Ok(t) => Ok(convert(toml::Value::Table(t))),
        Err(e) => err(SORTITION_ERR_PARSE, format!("TOML: {e}")),
    }
}

#[cfg(not(feature = "toml"))]
fn parse_toml(_: &str) -> Result<Value, DataError> {
    err(SORTITION_ERR_FORMAT, "TOML support is not compiled in")
}

#[cfg(feature = "yaml")]
fn parse_yaml(text: &str) -> Result<Value, DataError> {
    use saphyr::{LoadableYamlNode, Scalar, Yaml};
    use saphyr_parser::{Event, EventReceiver, Parser};

    // saphyr のローダーはエイリアスを複製して展開するため、少ない入力から指数的に
    // 膨らむ攻撃（billion laughs）が成立する。ロード前にイベント列でエイリアスと
    // 過度な入れ子を弾く（パーサー自体は展開しないので線形時間）。
    struct Guard {
        depth: usize,
        problem: Option<&'static str>,
    }
    impl<'input> EventReceiver<'input> for Guard {
        fn on_event(&mut self, ev: Event<'input>) {
            match ev {
                Event::Alias(_) => {
                    self.problem.get_or_insert("YAML aliases are not supported");
                }
                Event::SequenceStart(..) | Event::MappingStart(..) => {
                    self.depth += 1;
                    if self.depth > MAX_DEPTH {
                        self.problem.get_or_insert("YAML nesting is too deep");
                    }
                }
                Event::SequenceEnd | Event::MappingEnd => self.depth = self.depth.saturating_sub(1),
                _ => {}
            }
        }
    }

    let mut guard = Guard { depth: 0, problem: None };
    if let Err(e) = Parser::new_from_str(text).load(&mut guard, true) {
        return err(SORTITION_ERR_PARSE, format!("YAML: {e}"));
    }
    if let Some(p) = guard.problem {
        return err(SORTITION_ERR_PARSE, format!("YAML: {p}"));
    }

    fn key_string(k: &Yaml) -> Option<String> {
        match k {
            Yaml::Value(Scalar::String(s)) => Some(s.to_string()),
            Yaml::Value(Scalar::Integer(i)) => Some(i.to_string()),
            Yaml::Value(Scalar::Boolean(b)) => Some(b.to_string()),
            Yaml::Value(Scalar::FloatingPoint(f)) => Some(f.0.to_string()),
            Yaml::Value(Scalar::Null) => Some("null".to_string()),
            _ => None,
        }
    }

    fn convert(y: &Yaml) -> Result<Value, &'static str> {
        Ok(match y {
            Yaml::Value(Scalar::Null) => Value::Null,
            Yaml::Value(Scalar::Boolean(b)) => Value::Bool(*b),
            Yaml::Value(Scalar::Integer(i)) => Value::from(*i),
            Yaml::Value(Scalar::FloatingPoint(f)) => float_value(f.0),
            Yaml::Value(Scalar::String(s)) => Value::String(s.to_string()),
            Yaml::Sequence(v) => Value::Array(v.iter().map(convert).collect::<Result<_, _>>()?),
            Yaml::Mapping(m) => {
                let mut obj = Map::new();
                for (k, v) in m {
                    let key = key_string(k).ok_or("unsupported mapping key")?;
                    obj.insert(key, convert(v)?);
                }
                Value::Object(obj)
            }
            Yaml::Tagged(..) => return Err("custom YAML tags are not supported"),
            Yaml::Alias(_) => return Err("YAML aliases are not supported"),
            _ => return Err("invalid YAML value"),
        })
    }

    let docs = match Yaml::load_from_str(text) {
        Ok(d) => d,
        Err(e) => return err(SORTITION_ERR_PARSE, format!("YAML: {e}")),
    };
    match docs.as_slice() {
        [doc] => convert(doc).or_else(|m| err(SORTITION_ERR_PARSE, format!("YAML: {m}"))),
        [] => err(SORTITION_ERR_PARSE, "YAML: empty document"),
        _ => err(SORTITION_ERR_PARSE, "YAML: multiple documents are not supported"),
    }
}

#[cfg(not(feature = "yaml"))]
fn parse_yaml(_: &str) -> Result<Value, DataError> {
    err(SORTITION_ERR_FORMAT, "YAML support is not compiled in")
}

/// `a.b.0` のようなドット区切りの path をたどって、その位置の値を取り出す。
/// 空文字列ならルート自身。要素が配列でなければエラー。
fn select(root: Value, path: &str) -> Result<Vec<Value>, DataError> {
    let mut cur = root;
    if !path.is_empty() {
        for seg in path.split('.') {
            let next = match cur {
                Value::Object(mut m) => m.swap_remove(seg),
                Value::Array(a) => seg.parse::<usize>().ok().and_then(|i| a.into_iter().nth(i)),
                _ => None,
            };
            cur = match next {
                Some(v) => v,
                None => return err(SORTITION_ERR_PATH, format!("path `{path}` not found")),
            };
        }
    }
    match cur {
        Value::Array(a) => Ok(a),
        _ if path.is_empty() => {
            err(SORTITION_ERR_PATH, "the root is not an array (specify a path to the array)")
        }
        _ => err(SORTITION_ERR_PATH, format!("the value at `{path}` is not an array")),
    }
}

/// Draws `k` elements without replacement from the array at `path` in `text` (in the given `format`) and
/// returns the chosen elements as a JSON array string.
///
/// Like [`draw`](crate::draw), this works on positions in the array.
pub fn draw_data(
    format: i32,
    text: &str,
    path: &str,
    k: u32,
    seed: Option<u64>,
) -> Result<String, DataError> {
    let items = select(parse(format, text)?, path)?;
    let n = match u32::try_from(items.len()) {
        Ok(n) => n,
        Err(_) => return err(SORTITION_ERR_INVALID, "too many candidates"),
    };
    let picked = match draw(n, k, seed) {
        Ok(p) => p,
        Err(code) => return err(code, "k exceeds the number of candidates"),
    };
    // draw の結果は重複しないので、各要素を一度だけ取り出せる
    let mut slots: Vec<Option<Value>> = items.into_iter().map(Some).collect();
    let out: Vec<Value> = picked.into_iter().filter_map(|i| slots[i as usize].take()).collect();
    Ok(Value::Array(out).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(format: i32, text: &str, path: &str, k: u32) -> Result<Vec<Value>, DataError> {
        draw_data(format, text, path, k, Some(1))
            .map(|s| serde_json::from_str::<Vec<Value>>(&s).unwrap())
    }

    fn sorted(mut v: Vec<Value>) -> Vec<String> {
        let mut s: Vec<String> = v.drain(..).map(|x| x.to_string()).collect();
        s.sort();
        s
    }

    #[test]
    fn json_root_array_keeps_structure() {
        let v = run(SORTITION_FORMAT_JSON, r#"[{"id":1,"tags":["a"]},{"id":2},null,1.5e2]"#, "", 4)
            .unwrap();
        // 数値は JSON として正規化される（1.5e2 → 150.0）
        assert_eq!(sorted(v), ["150.0", "null", r#"{"id":1,"tags":["a"]}"#, r#"{"id":2}"#]);
    }

    #[test]
    fn json_nested_path_and_index() {
        let text = r#"{"a":{"list":["x","y","z"]},"b":[["p","q"]]}"#;
        assert_eq!(run(SORTITION_FORMAT_JSON, text, "a.list", 3).unwrap().len(), 3);
        assert_eq!(run(SORTITION_FORMAT_JSON, text, "b.0", 2).unwrap().len(), 2);
        assert_eq!(run(SORTITION_FORMAT_JSON, text, "a.nope", 1).unwrap_err().code, SORTITION_ERR_PATH);
        assert_eq!(run(SORTITION_FORMAT_JSON, text, "a", 1).unwrap_err().code, SORTITION_ERR_PATH);
        assert_eq!(run(SORTITION_FORMAT_JSON, text, "", 1).unwrap_err().code, SORTITION_ERR_PATH);
    }

    #[test]
    fn errors_have_codes() {
        assert_eq!(run(SORTITION_FORMAT_JSON, "[1,2]", "", 3).unwrap_err().code, SORTITION_ERR_INVALID);
        assert_eq!(run(SORTITION_FORMAT_JSON, "[1,", "", 1).unwrap_err().code, SORTITION_ERR_PARSE);
        assert_eq!(run(99, "[]", "", 0).unwrap_err().code, SORTITION_ERR_FORMAT);
    }

    #[cfg(feature = "toml")]
    #[test]
    fn toml_array_and_array_of_tables() {
        let text = "candidates = [\"a\", \"b\", \"c\"]\nwhen = 2024-01-02T03:04:05Z\n\n[[member]]\nname = \"x\"\n\n[[member]]\nname = \"y\"\n";
        assert_eq!(sorted(run(SORTITION_FORMAT_TOML, text, "candidates", 3).unwrap()), ["\"a\"", "\"b\"", "\"c\""]);
        let m = run(SORTITION_FORMAT_TOML, text, "member", 2).unwrap();
        assert_eq!(sorted(m), [r#"{"name":"x"}"#, r#"{"name":"y"}"#]);
        // ルートはテーブルなので path 必須
        assert_eq!(run(SORTITION_FORMAT_TOML, text, "", 1).unwrap_err().code, SORTITION_ERR_PATH);
        assert_eq!(run(SORTITION_FORMAT_TOML, "a = [", "a", 1).unwrap_err().code, SORTITION_ERR_PARSE);
        let d = run(SORTITION_FORMAT_TOML, "d = [2024-01-02T03:04:05Z]", "d", 1).unwrap();
        assert_eq!(d, [Value::String("2024-01-02T03:04:05Z".into())]);
    }

    #[cfg(feature = "yaml")]
    #[test]
    fn yaml_root_sequence_and_path() {
        let text = "- name: alice\n  age: 30\n- name: bob\n  tags: [a, b]\n- plain\n- no\n- ~\n";
        let v = run(SORTITION_FORMAT_YAML, text, "", 5).unwrap();
        // YAML 1.2: `no` は bool ではなく文字列
        let mut want: Vec<String> = [
            r#""no""#,
            r#""plain""#,
            r#"{"name":"alice","age":30}"#,
            r#"{"name":"bob","tags":["a","b"]}"#,
            "null",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        want.sort();
        assert_eq!(sorted(v), want);
        let nested = "team:\n  members: [x, y, z]\n";
        assert_eq!(run(SORTITION_FORMAT_YAML, nested, "team.members", 2).unwrap().len(), 2);
    }

    #[cfg(feature = "yaml")]
    #[test]
    fn yaml_rejects_dangerous_input() {
        let alias = "a: &x [1,2]\nb: *x\n";
        assert_eq!(run(SORTITION_FORMAT_YAML, alias, "a", 1).unwrap_err().code, SORTITION_ERR_PARSE);
        // billion laughs
        let bomb = "a: &a [x,x,x,x,x,x,x,x,x]\nb: &b [*a,*a,*a,*a,*a,*a,*a,*a,*a]\nc: &c [*b,*b,*b,*b,*b,*b,*b,*b,*b]\nd: &d [*c,*c,*c,*c,*c,*c,*c,*c,*c]\ne: [*d,*d,*d,*d,*d,*d,*d,*d,*d]\n";
        assert_eq!(run(SORTITION_FORMAT_YAML, bomb, "e", 1).unwrap_err().code, SORTITION_ERR_PARSE);
        assert_eq!(run(SORTITION_FORMAT_YAML, "- !custom foo\n", "", 1).unwrap_err().code, SORTITION_ERR_PARSE);
        assert_eq!(run(SORTITION_FORMAT_YAML, "- a\n---\n- b\n", "", 1).unwrap_err().code, SORTITION_ERR_PARSE);
        let deep = format!("{}1{}", "[".repeat(1000), "]".repeat(1000));
        assert_eq!(run(SORTITION_FORMAT_YAML, &deep, "", 1).unwrap_err().code, SORTITION_ERR_PARSE);
    }

    #[test]
    fn seed_is_reproducible() {
        let t = r#"[1,2,3,4,5,6,7,8,9,10]"#;
        assert_eq!(draw_data(0, t, "", 3, Some(5)).unwrap(), draw_data(0, t, "", 3, Some(5)).unwrap());
    }
}
