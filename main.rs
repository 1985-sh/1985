use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::{env, fs, process};

enum Value {
    Null,
    Bool(bool),
    Num(String),
    Str(String),
    Arr(Vec<Value>),
    Obj(Vec<(String, Value)>),
}

static NULL: Value = Value::Null;

impl std::ops::Index<&str> for Value {
    type Output = Value;
    fn index(&self, k: &str) -> &Value {
        match self {
            Value::Obj(o) => o.iter().find(|(x, _)| x == k).map(|(_, v)| v).unwrap_or(&NULL),
            _ => &NULL,
        }
    }
}

impl Value {
    fn str(&self) -> &str {
        if let Value::Str(s) = self { s } else { "" }
    }
    fn arr(&self) -> &[Value] {
        if let Value::Arr(a) = self { a } else { &[] }
    }
}

fn q(s: &str) -> String {
    let mut o = String::from('"');
    for c in s.chars() {
        match c {
            '"' => o += "\\\"",
            '\\' => o += "\\\\",
            c if (c as u32) < 0x20 => o += &format!("\\u{:04x}", c as u32),
            c => o.push(c),
        }
    }
    o + "\""
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Value::Null => write!(f, "null"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Num(n) => write!(f, "{n}"),
            Value::Str(s) => write!(f, "{}", q(s)),
            Value::Arr(a) => write!(f, "[{}]", a.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",")),
            Value::Obj(o) => write!(f, "{{{}}}", o.iter().map(|(k, v)| format!("{}:{v}", q(k))).collect::<Vec<_>>().join(",")),
        }
    }
}

fn parse(s: &str) -> Value {
    let b: Vec<char> = s.chars().collect();
    let mut i = 0;
    val(&b, &mut i).unwrap_or(Value::Null)
}

fn val(b: &[char], i: &mut usize) -> Option<Value> {
    while b.get(*i)?.is_whitespace() {
        *i += 1;
    }
    let c = *b.get(*i)?;
    *i += 1;
    Some(match c {
        '{' | '[' => {
            let (mut o, mut a) = (Vec::new(), Vec::new());
            loop {
                while b.get(*i)?.is_whitespace() || b[*i] == ',' {
                    *i += 1;
                }
                if b[*i] == '}' || b[*i] == ']' {
                    *i += 1;
                    break;
                }
                if c == '{' {
                    let Value::Str(k) = val(b, i)? else { return None };
                    while b.get(*i)? != &':' {
                        *i += 1;
                    }
                    *i += 1;
                    o.push((k, val(b, i)?));
                } else {
                    a.push(val(b, i)?);
                }
            }
            if c == '{' { Value::Obj(o) } else { Value::Arr(a) }
        }
        '"' => {
            let mut o = String::new();
            loop {
                match *b.get(*i)? {
                    '"' => break,
                    '\\' => {
                        *i += 1;
                        o.push(match b[*i] {
                            'n' => '\n',
                            't' => '\t',
                            'r' => '\r',
                            'b' => '\u{8}',
                            'f' => '\u{c}',
                            'u' => {
                                let h = |i: usize| u32::from_str_radix(&b.get(i..i + 4).map(|x| x.iter().collect::<String>()).unwrap_or_default(), 16).unwrap_or(0xfffd);
                                let mut u = h(*i + 1);
                                *i += 4;
                                if (0xd800..0xdc00).contains(&u) && b.get(*i + 1) == Some(&'\\') {
                                    u = 0x10000 + ((u - 0xd800) << 10) + (h(*i + 3) - 0xdc00);
                                    *i += 6;
                                }
                                char::from_u32(u).unwrap_or('\u{fffd}')
                            }
                            c => c,
                        })
                    }
                    c => o.push(c),
                }
                *i += 1;
            }
            *i += 1;
            Value::Str(o)
        }
        't' => { *i += 3; Value::Bool(true) }
        'f' => { *i += 4; Value::Bool(false) }
        'n' => { *i += 3; Value::Null }
        _ => {
            let st = *i - 1;
            while b.get(*i).is_some_and(|c| "+-.eE0123456789".contains(*c)) {
                *i += 1;
            }
            Value::Num(b[st..*i].iter().collect())
        }
    })
}

fn key_path() -> std::path::PathBuf {
    let home = env::var("HOME").or_else(|_| env::var("USERPROFILE")).unwrap_or_else(|_| ".".into());
    std::path::Path::new(&home).join(".config/1985/key")
}

fn save(key: &str) {
    let p = key_path();
    fs::create_dir_all(p.parent().unwrap()).ok();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&p).unwrap_or_else(|e| die(&e.to_string()));
        f.write_all(key.as_bytes()).ok();
    }
    #[cfg(not(unix))]
    fs::write(&p, key).ok();
}

fn b64u(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => continue,
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

fn expiry(key: &str) -> u64 {
    let body = key.trim_start_matches("85_").split('.').next().unwrap_or("");
    String::from_utf8(b64u(body)).ok().and_then(|s| s.split('|').nth(4).and_then(|x| x.parse().ok())).unwrap_or(0)
}

fn die(m: &str) -> ! {
    eprintln!("{m}");
    process::exit(1)
}

fn key() -> String {
    let k = env::var("NINETEEN85_KEY").ok().or_else(|| fs::read_to_string(key_path()).ok()).map(|s| s.trim().to_string());
    let Some(k) = k.filter(|k| k.starts_with("85_")) else { die("no key. run: 1985 key free  or  1985 key set <key>") };
    if expiry(&k) < std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() + 7 * 86400 && env::var("NINETEEN85_KEY").is_err() {
        if let Ok(v) = req("POST", "/keys", Some(&k), Some("{}".into())) {
            if let Some(n) = Some(v["key"].str()).filter(|s| !s.is_empty()) {
                save(n);
                return n.to_string();
            }
        }
    }
    k
}

fn curl(method: &str, path: &str, k: Option<&str>, body: Option<String>) -> process::Child {
    let mut c = Command::new("curl").args(["-sN", "--fail-with-body", "-K", "-"]).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap_or_else(|e| die(&format!("curl: {e}")));
    let mut cfg = format!("url = {}\nrequest = {method}\nuser-agent = \"1985-cli\"\nheader = \"content-type: application/json\"\n", q(&(env::var("NINETEEN85_URL").unwrap_or_else(|_| "https://api.1985.sh/v1".into()) + path)));
    if let Some(k) = k {
        cfg += &format!("header = {}\n", q(&format!("authorization: Bearer {k}")));
    }
    if let Some(b) = body {
        cfg += &format!("data-binary = {}\n", q(&b));
    }
    c.stdin.take().unwrap().write_all(cfg.as_bytes()).ok();
    c
}

fn err(c: process::Child, out: String) -> Result<String, String> {
    let st = c.wait_with_output().map_err(|e| e.to_string())?.status;
    if st.success() {
        return Ok(out);
    }
    let m = parse(&out)["error"]["message"].str().to_string();
    Err(if m.is_empty() { format!("error {}", st.code().unwrap_or(1)) } else { m })
}

fn req(method: &str, path: &str, k: Option<&str>, body: Option<String>) -> Result<Value, String> {
    let mut c = curl(method, path, k, body);
    let mut out = String::new();
    c.stdout.take().unwrap().read_to_string(&mut out).ok();
    err(c, out).map(|o| parse(&o))
}

fn stream(path: &str, k: &str, body: String, mut on: impl FnMut(&str, &Value)) -> Result<(), String> {
    let mut c = curl("POST", path, Some(k), Some(body));
    let (mut event, mut rest) = (String::new(), String::new());
    for line in BufReader::new(c.stdout.take().unwrap()).lines() {
        let line = line.map_err(|e| e.to_string())?;
        if let Some(e) = line.strip_prefix("event:") {
            event = e.trim().to_string();
        } else if let Some(d) = line.strip_prefix("data:") {
            on(&event, &parse(d.trim()));
        } else {
            rest += &line;
        }
    }
    err(c, rest).map(|_| ())
}

fn run(model: &str, prompt: &str) -> Result<(), String> {
    let mut out = std::io::stdout();
    stream("/responses", &key(), format!("{{\"model\":{},\"input\":{},\"stream\":true}}", q(model), q(prompt)), |_, v| {
        if v["type"].str() == "response.output_text.delta" {
            print!("{}", v["delta"].str());
            out.flush().ok();
        }
    })?;
    println!();
    Ok(())
}

fn main() {
    let a: Vec<String> = env::args().skip(1).collect();
    let s: Vec<&str> = a.iter().map(String::as_str).collect();
    let r = match s.as_slice() {
        ["run", model, rest @ ..] if !rest.is_empty() => run(model, &rest.join(" ")),
        ["models"] => req("GET", "/models", None, None).map(|v| {
            for m in v["data"].arr() {
                println!("{:<26}${} in  ${} out{}", m["id"].str(), m["pricing"]["input"], m["pricing"]["output"], if matches!(m["free"], Value::Bool(true)) { "  free" } else { "" });
            }
        }),
        ["search", rest @ ..] if !rest.is_empty() => req("POST", "/search", Some(&key()), Some(format!("{{\"query\":{}}}", q(&rest.join(" "))))).map(|v| {
            for x in v["results"].arr() {
                println!("{}\n{}\n", x["title"].str(), x["url"].str());
            }
        }),
        ["sandbox", "new"] => req("POST", "/sandboxes", Some(&key()), Some("{}".into())).map(|v| println!("{}", v["id"].str())),
        ["sandbox", "exec", id, rest @ ..] if !rest.is_empty() => req("POST", &format!("/sandboxes/{id}/exec"), Some(&key()), Some(format!("{{\"command\":{}}}", q(&rest.join(" "))))).map(|v| {
            print!("{}", v["output"].str());
            process::exit(if let Value::Num(n) = &v["exit_code"] { n.parse().unwrap_or(1) } else { 1 })
        }),
        ["sandbox", "rm", id] => req("DELETE", &format!("/sandboxes/{id}"), Some(&key()), None).map(|_| ()),
        ["agent", rest @ ..] if !rest.is_empty() => stream("/agents", &key(), format!("{{\"goal\":{}}}", q(&rest.join(" "))), |e, v| match e {
            "step" => eprintln!("{} {}", v["tool"].str(), v["input"]),
            "done" => println!("{}", v["output"].str()),
            "error" => eprintln!("{}", v["message"].str()),
            _ => {}
        }),
        ["key"] => Ok(println!("{}", key())),
        ["key", "set", k] => Ok(save(k)),
        ["key", "free"] => process::Command::new("ssh").args(["1985.sh", "key"]).output().map_err(|e| e.to_string()).and_then(|o| {
            let k = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if k.starts_with("85_") { save(&k); Ok(println!("saved")) } else { Err(String::from_utf8_lossy(&o.stderr).into_owned() + &k) }
        }),
        ["key", "revoke"] => req("DELETE", "/keys", Some(&key()), None).map(|_| println!("revoked")),
        ["pay"] | ["plan"] => Ok(println!("cards   https://1985.sh/plans\nwallet  https://1985.sh/docs/pay")),
        _ => Ok(println!("1985\n\nrun <model> <prompt>\nmodels\nsearch <query>\nsandbox new | exec <id> <command> | rm <id>\nagent <goal>\nkey | key free | key set <key> | key revoke\npay")),
    };
    if let Err(e) = r {
        die(&e)
    }
}
