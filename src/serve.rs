// MergeSort Pro web console (--gui) + REST API (--api): local servers for
// non-technical buyers and integrators. std only (TcpListener + threads).
// GUI routes: GET /, /api/status, /api/jobs, /api/job?id=, POST /api/sort,
//   POST /api/upload, GET /api/gen, GET /download, GET /api/history,
//   POST /api/rerun, POST /api/history/clear.
// API mode adds: GET /api/health, GET /api/job/<id>, GET /api/job/<id>/result
//   + optional Bearer token (--api-token). Bind 127.0.0.1 unless you know why not.
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn jobs() -> &'static Mutex<HashMap<String, Job>> {
    static J: OnceLock<Mutex<HashMap<String, Job>>> = OnceLock::new();
    J.get_or_init(|| Mutex::new(HashMap::new()))
}

fn counter() -> &'static Mutex<u64> {
    static C: OnceLock<Mutex<u64>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(0))
}

fn brand() -> &'static Mutex<(String, String)> {
    static B: OnceLock<Mutex<(String, String)>> = OnceLock::new();
    B.get_or_init(|| Mutex::new((String::new(), String::new())))
}

fn api_token_store() -> &'static Mutex<Option<String>> {
    static T: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(None))
}

#[derive(Clone, Debug)]
struct Job {
    id: String,
    input: String,
    output: String,
    status: String, // queued|running|done|error
    detail: String, // summary or error tail
    started: u64,
    finished: u64,
    params: String,   // raw /api/sort JSON (for re-run + history)
    duration_s: u64,
}

fn history_path() -> PathBuf {
    if let Some(dir) = std::env::var_os("APPDATA") {
        PathBuf::from(dir).join("MergeSort").join("jobs.json")
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".mergesort").join("jobs.json")
    } else {
        PathBuf::from(".mergesort").join("jobs.json")
    }
}

fn append_history(entry: &str) {
    let p = history_path();
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut items: Vec<String> = Vec::new();
    if let Ok(txt) = std::fs::read_to_string(&p) {
        let t = txt.trim();
        if t.starts_with('[') && t.ends_with(']') {
            let inner = &t[1..t.len() - 1];
            // Split top-level objects naively (entries are flat, no nested arrays).
            let mut depth = 0i32;
            let mut cur = String::new();
            for c in inner.chars() {
                if c == '{' {
                    depth += 1;
                }
                if depth > 0 {
                    cur.push(c);
                }
                if c == '}' {
                    depth -= 1;
                    if depth == 0 && !cur.trim().is_empty() {
                        items.push(cur.trim().to_string());
                        cur = String::new();
                    }
                }
            }
        }
    }
    items.push(entry.to_string());
    while items.len() > 100 {
        items.remove(0);
    }
    let _ = std::fs::write(&p, format!("[{}]", items.join(",")));
}

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn json_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}

/// Minimal JSON string-field extractor for the fixed /api/sort schema.
fn jstr(body: &str, key: &str) -> Option<String> {
    let pat = format!("\"{}\"", key);
    let i = body.find(&pat)? + pat.len();
    let rest = body[i..].trim_start();
    if !rest.starts_with(':') {
        return None;
    }
    let rest = rest[1..].trim_start();
    if rest.starts_with("true") {
        return Some("true".to_string());
    }
    if rest.starts_with("false") {
        return Some("false".to_string());
    }
    if rest.starts_with('"') {
        let bytes = rest.as_bytes();
        let mut out = String::new();
        let mut j = 1usize;
        while j < bytes.len() {
            match bytes[j] {
                b'"' => return Some(out),
                b'\\' if j + 1 < bytes.len() => {
                    match bytes[j + 1] {
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'u' if j + 5 < bytes.len() => {
                            if let Ok(s) = std::str::from_utf8(&bytes[j + 2..j + 6])
                                && let Ok(cp) = u32::from_str_radix(s, 16) {
                                    out.push(char::from_u32(cp).unwrap_or('?'));
                                }
                            j += 4;
                        }
                        other => out.push(other as char),
                    }
                    j += 1;
                }
                other => out.push(other as char),
            }
            j += 1;
        }
        return None;
    }
    // bare number
    let end = rest.find(|c: char| !(c.is_ascii_digit() || c == '-')).unwrap_or(rest.len());
    if end == 0 {
        return None;
    }
    Some(rest[..end].to_string())
}

pub fn run_gui(bind: &str, port: u16, brand_name: Option<String>, brand_color: Option<String>) -> std::io::Result<()> {
    *brand().lock().unwrap() = (
        brand_name.unwrap_or_default(),
        brand_color.unwrap_or_default(),
    );
    *api_token_store().lock().unwrap() = None;
    let listener = TcpListener::bind((bind, port))?;
    let bound = listener.local_addr().map(|a| a.port()).unwrap_or(port);
    eprintln!("MergeSort Pro console: http://{}:{}/", bind, bound);
    eprintln!("Tip: keep this window open. Press Ctrl-C to stop. Bind 127.0.0.1 only on shared machines.");
    for conn in listener.incoming() {
        match conn {
            Ok(s) => {
                std::thread::spawn(move || handle_conn(s));
            }
            Err(_) => continue,
        }
    }
    Ok(())
}

/// REST API mode for integrators (--api [host:]PORT [--api-token T]).
/// Same engine as GUI plus /api/health, /api/job/<id>, /api/job/<id>/result.
pub fn run_api(bind: &str, port: u16, token: Option<String>) -> std::io::Result<()> {
    *brand().lock().unwrap() = (String::new(), String::new());
    *api_token_store().lock().unwrap() = token.clone();
    let listener = TcpListener::bind((bind, port))?;
    let bound = listener.local_addr().map(|a| a.port()).unwrap_or(port);
    eprintln!("MergeSort Pro API: http://{}:{}/api/health", bind, bound);
    if token.is_some() {
        eprintln!("API token: enabled (send Authorization: Bearer <token> or ?token=)");
    } else {
        eprintln!("API token: none — bind 127.0.0.1 only!");
    }
    for conn in listener.incoming() {
        match conn {
            Ok(s) => {
                std::thread::spawn(move || handle_conn(s));
            }
            Err(_) => continue,
        }
    }
    Ok(())
}

fn api_authorized(target: &str, headers: &HashMap<String, String>) -> bool {
    let need = api_token_store().lock().unwrap().clone();
    let need = match need {
        Some(t) => t,
        None => return true,
    };
    if let Some(q) = query_param(target, "token")
        && q == need {
            return true;
        }
    if let Some(h) = headers.get("authorization") {
        let h = h.trim();
        if let Some(tok) = h.strip_prefix("Bearer ").or_else(|| h.strip_prefix("bearer "))
            && tok.trim() == need {
                return true;
            }
    }
    false
}

fn send(stream: &mut TcpStream, ctype: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        ctype,
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

fn send_text(stream: &mut TcpStream, code: u16, msg: &str, body: &str) {
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        code,
        msg,
        body.len(),
        body
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.flush();
}

fn read_request(stream: &mut TcpStream) -> Option<(String, String, HashMap<String, String>, Vec<u8>)> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let mut head_buf: Vec<u8> = Vec::new();
    let mut tmp = [0u8; 4096];
    // Read until end of headers.
    loop {
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                head_buf.extend_from_slice(&tmp[..n]);
                if head_buf.len() > 65536 {
                    return None;
                }
                if head_buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            Err(_) => return None,
        }
    }
    let hlen = head_buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)?;
    let head_str = String::from_utf8_lossy(&head_buf[..hlen]).into_owned();
    let mut lines = head_str.lines();
    let req_line = lines.next().unwrap_or("");
    let mut parts = req_line.split_ascii_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let mut headers = HashMap::new();
    for l in lines {
        if let Some((k, v)) = l.split_once(':') {
            headers.insert(k.trim().to_lowercase(), v.trim().to_string());
        }
    }
    let cl: usize = headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut body: Vec<u8> = Vec::new();
    if head_buf.len() > hlen {
        body.extend_from_slice(&head_buf[hlen..]);
    }
    // Uploads can be large: cap single-request body at 512MB to avoid OOM.
    if cl > 512 * 1024 * 1024 {
        return None;
    }
    while body.len() < cl {
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => body.extend_from_slice(&tmp[..n]),
            Err(_) => break,
        }
        if body.len() > 512 * 1024 * 1024 {
            break;
        }
    }
    body.truncate(cl.min(body.len()));
    Some((method, target, headers, body))
}

fn query_param(target: &str, key: &str) -> Option<String> {
    let q = target.split_once('?')?.1;
    for kv in q.split('&') {
        if let Some((k, v)) = kv.split_once('=')
            && k == key {
                return Some(url_decode(v));
            }
    }
    None
}

fn url_decode(s: &str) -> String {
    let mut out = String::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => {
                out.push(' ');
                i += 1;
            }
            b'%' if i + 2 < b.len() => {
                if let Ok(hex) = std::str::from_utf8(&b[i + 1..i + 3])
                    && let Ok(v) = u8::from_str_radix(hex, 16) {
                        out.push(v as char);
                        i += 3;
                        continue;
                    }
                out.push('%');
                i += 1;
            }
            c => {
                out.push(c as char);
                i += 1;
            }
        }
    }
    out
}

fn gui_html() -> String {
    let (bname, bcolor) = brand().lock().unwrap().clone();
    let mut html = GUI_HTML.to_string();
    if !bname.is_empty() {
        html = html.replace("MergeSort <span>Pro</span> — Console", &format!("{} — <span>powered by MergeSort Pro</span>", bname));
        html = html.replace("<title>MergeSort Pro — Console</title>", &format!("<title>{} — Console</title>", bname));
    }
    if !bcolor.is_empty() {
        html = html.replace("--acc:#56c2d6", &format!("--acc:{}", bcolor));
    }
    html
}

/// Best-effort sniff of a server-side path for the GUI's "simple mode":
/// guesses format (csv/jsonl/string), delimiter, header, and — for csv —
/// real column names (or "Kolom N" + a sample value) so a non-technical
/// user can pick "sort by City" instead of typing a 0-based column index.
fn inspect_path_json(path: String) -> String {
    let empty = "{\"format\":\"string\",\"delimiter\":\",\",\"header\":false,\"columns\":[]}";
    if path.is_empty() || !Path::new(&path).is_file() {
        return empty.to_string();
    }
    let p = Path::new(&path);
    let (enc, bom_len) = crate::inspect::sniff_bom(p);
    let sample = crate::inspect::read_sample_lines(p, &enc, bom_len, 50);
    let first_nonempty = sample.iter().find(|l| !l.trim().is_empty()).cloned().unwrap_or_default();
    if first_nonempty.trim().starts_with('{') && first_nonempty.trim().ends_with('}') {
        return "{\"format\":\"jsonl\",\"delimiter\":\",\",\"header\":false,\"columns\":[]}".to_string();
    }
    if sample.is_empty() {
        return empty.to_string();
    }
    let (delim, cols) = crate::inspect::detect_delimiter(&sample);
    let is_csv = cols > 1;
    let header = is_csv && crate::inspect::detect_header(&sample, false, 0);
    let mut columns_json = String::new();
    if is_csv {
        let ch = delim as char;
        let names: Vec<String> = if header {
            sample[0].split(ch).map(|s| s.trim().to_string()).collect()
        } else {
            (0..cols).map(|i| format!("Kolom {}", i + 1)).collect()
        };
        let sample_row = sample.iter().skip(if header { 1 } else { 0 }).find(|l| !l.trim().is_empty()).cloned().unwrap_or_default();
        let sample_fields: Vec<&str> = sample_row.split(ch).collect();
        let parts: Vec<String> = names
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let ex = sample_fields.get(i).map(|s| s.trim()).unwrap_or("");
                format!("{{\"index\":{},\"name\":\"{}\",\"example\":\"{}\"}}", i, json_escape(name), json_escape(ex))
            })
            .collect();
        columns_json = parts.join(",");
    }
    format!(
        "{{\"format\":\"{}\",\"delimiter\":\"{}\",\"header\":{},\"columns\":[{}]}}",
        if is_csv { "csv" } else { "string" },
        json_escape(&(delim as char).to_string()),
        header,
        columns_json
    )
}

fn queue_sort_from_json(body_s: &str) -> Result<String, String> {
    let input = jstr(body_s, "input").unwrap_or_default();
    let output = jstr(body_s, "output").unwrap_or_default();
    if input.is_empty() || output.is_empty() {
        return Err("input and output paths are required".to_string());
    }
    if !Path::new(&input).is_file() {
        return Err(format!("input not found: {}", input));
    }
    let max_memory = jstr(body_s, "max_memory").unwrap_or_else(|| "512MB".to_string());
    let mode = jstr(body_s, "mode").unwrap_or_else(|| "string".to_string());
    let key_column = jstr(body_s, "key_column").unwrap_or_default();
    let key_type = jstr(body_s, "key_type").unwrap_or_else(|| "string".to_string());
    let bb = |k: &str| jstr(body_s, k).as_deref() == Some("true");
    let (reverse, unique, header) = (bb("reverse"), bb("unique"), bb("header"));
    let verify = jstr(body_s, "verify").map(|v| v != "false").unwrap_or(true);
    let limit = jstr(body_s, "limit").unwrap_or_default();
    let key_field = jstr(body_s, "key_field").unwrap_or_default();
    let mut n = counter().lock().unwrap();
    *n += 1;
    let id = format!("job-{}-{}", now_unix(), *n);
    drop(n);
    jobs().lock().unwrap().insert(id.clone(), Job {
        id: id.clone(),
        input: input.clone(),
        output: output.clone(),
        status: "queued".to_string(),
        detail: "queued".to_string(),
        started: now_unix(),
        finished: 0,
        params: body_s.to_string(),
        duration_s: 0,
    });
    let log_path = job_log_path(&id);
    let id2 = id.clone();
    std::thread::spawn(move || {
        run_sort_job(id2, log_path, input, output, max_memory, mode, key_column, key_type, reverse, unique, header, verify, limit, key_field);
    });
    Ok(id)
}

fn handle_conn(mut stream: TcpStream) {
    let (method, target, headers, body) = match read_request(&mut stream) {
        Some(r) => r,
        None => return,
    };
    let path = target.split('?').next().unwrap_or("/");

    // Path-style REST: /api/job/<id>, /api/job/<id>/result
    if method == "GET" && path.starts_with("/api/job/") {
        let rest = path.trim_start_matches("/api/job/");
        if let Some(id) = rest.strip_suffix("/result") {
            if !api_authorized(&target, &headers) {
                send_text(&mut stream, 401, "Unauthorized", "{\"error\":\"api token required\"}");
                return;
            }
            let out = jobs().lock().unwrap().get(id).map(|j| j.output.clone());
            match out {
                Some(p) if Path::new(&p).is_file() => return stream_file(&mut stream, Path::new(&p)),
                _ => {
                    send_text(&mut stream, 404, "Not Found", "{\"error\":\"result not ready or job unknown\"}");
                    return;
                }
            }
        } else if !rest.contains('/') && !rest.is_empty() {
            let map = jobs().lock().unwrap();
            match map.get(rest) {
                Some(j) => {
                    let log_tail = job_log_tail(&j.id, 3000);
                    let b = format!(
                        "{{\"id\":\"{}\",\"status\":\"{}\",\"detail\":\"{}\",\"params\":\"{}\",\"duration_s\":{},\"log\":\"{}\"}}",
                        json_escape(&j.id),
                        json_escape(&j.status),
                        json_escape(&j.detail),
                        json_escape(&j.params),
                        j.duration_s,
                        json_escape(&log_tail)
                    );
                    send(&mut stream, "application/json", b.as_bytes());
                }
                None => send_text(&mut stream, 404, "Not Found", "{\"error\":\"job not found\"}"),
            }
            return;
        }
    }

    match (method.as_str(), path) {
        ("GET", "/") => {
            let h = gui_html();
            send(&mut stream, "text/html; charset=utf-8", h.as_bytes());
        }
        ("GET", "/api/health") => {
            send(&mut stream, "application/json", b"{\"status\":\"ok\"}");
        }
        ("GET", "/api/inspect") => {
            let b = inspect_path_json(query_param(&target, "path").unwrap_or_default());
            send(&mut stream, "application/json", b.as_bytes());
        }
        ("GET", "/api/history") => {
            let txt = std::fs::read_to_string(history_path()).unwrap_or_else(|_| "[]".to_string());
            send(&mut stream, "application/json", txt.as_bytes());
        }
        ("POST", "/api/history/clear") => {
            if !api_authorized(&target, &headers) {
                send_text(&mut stream, 401, "Unauthorized", "{\"error\":\"api token required\"}");
                return;
            }
            let _ = std::fs::write(history_path(), "[]");
            send(&mut stream, "application/json", b"{\"status\":\"cleared\"}");
        }
        ("POST", "/api/rerun") => {
            if !api_authorized(&target, &headers) {
                send_text(&mut stream, 401, "Unauthorized", "{\"error\":\"api token required\"}");
                return;
            }
            let body_s = String::from_utf8_lossy(&body).into_owned();
            // Accept {"id": "..."} (history id) or full sort params.
            if let Some(hid) = jstr(&body_s, "id") {
                let params = {
                    let map = jobs().lock().unwrap();
                    map.get(&hid).map(|j| j.params.clone()).or_else(|| {
                        // fall back to persistent history file
                        let txt = std::fs::read_to_string(history_path()).unwrap_or_default();
                        // naive: find object containing "id":"hid" and reuse its params
                        txt.contains(&format!("\"id\":\"{}\"", hid)).then(|| {
                            // params stored inside history entry
                            extract_history_params(&txt, &hid)
                        }).flatten()
                    })
                };
                match params {
                    Some(p) if !p.is_empty() => match queue_sort_from_json(&p) {
                        Ok(id) => {
                            let b = format!("{{\"id\":\"{}\",\"status\":\"queued\",\"reran\":\"{}\"}}", json_escape(&id), json_escape(&hid));
                            send(&mut stream, "application/json", b.as_bytes());
                        }
                        Err(e) => send_text(&mut stream, 400, "Bad Request", &format!("{{\"error\":\"{}\"}}", json_escape(&e))),
                    },
                    _ => send_text(&mut stream, 404, "Not Found", "{\"error\":\"job id unknown (history keeps last 100)\"}"),
                }
            } else {
                match queue_sort_from_json(&body_s) {
                    Ok(id) => {
                        let b = format!("{{\"id\":\"{}\",\"status\":\"queued\"}}", json_escape(&id));
                        send(&mut stream, "application/json", b.as_bytes());
                    }
                    Err(e) => send_text(&mut stream, 400, "Bad Request", &format!("{{\"error\":\"{}\"}}", json_escape(&e))),
                }
            }
        }
        ("GET", "/api/status") => {
            let n = jobs().lock().unwrap().len();
            let b = format!(
                "{{\"version\":\"{}\",\"jobs\":{}}}",
                env!("CARGO_PKG_VERSION"),
                n
            );
            send(&mut stream, "application/json", b.as_bytes());
        }
        ("GET", "/api/jobs") => {
            let map = jobs().lock().unwrap();
            let mut items: Vec<String> = Vec::new();
            for j in map.values() {
                items.push(format!(
                    "{{\"id\":\"{}\",\"input\":\"{}\",\"output\":\"{}\",\"status\":\"{}\",\"detail\":\"{}\",\"started\":{},\"finished\":{},\"duration_s\":{}}}",
                    json_escape(&j.id),
                    json_escape(&j.input),
                    json_escape(&j.output),
                    json_escape(&j.status),
                    json_escape(&j.detail),
                    j.started,
                    j.finished,
                    j.duration_s
                ));
            }
            let b = format!("{{\"jobs\":[{}]}}", items.join(","));
            send(&mut stream, "application/json", b.as_bytes());
        }
        ("GET", "/api/job") => {
            let id = query_param(&target, "id").unwrap_or_default();
            let map = jobs().lock().unwrap();
            match map.get(&id) {
                Some(j) => {
                    let log_tail = job_log_tail(&j.id, 3000);
                    let b = format!(
                        "{{\"id\":\"{}\",\"status\":\"{}\",\"detail\":\"{}\",\"params\":\"{}\",\"duration_s\":{},\"log\":\"{}\"}}",
                        json_escape(&j.id),
                        json_escape(&j.status),
                        json_escape(&j.detail),
                        json_escape(&j.params),
                        j.duration_s,
                        json_escape(&log_tail)
                    );
                    send(&mut stream, "application/json", b.as_bytes());
                }
                None => send_text(&mut stream, 404, "Not Found", "{\"error\":\"job not found\"}"),
            }
        }
        ("POST", "/api/sort") => {
            if !api_authorized(&target, &headers) {
                send_text(&mut stream, 401, "Unauthorized", "{\"error\":\"api token required\"}");
                return;
            }
            let body_s = String::from_utf8_lossy(&body).into_owned();
            match queue_sort_from_json(&body_s) {
                Ok(id) => {
                    let b = format!("{{\"id\":\"{}\",\"status\":\"queued\"}}", json_escape(&id));
                    send(&mut stream, "application/json", b.as_bytes());
                }
                Err(e) => send_text(&mut stream, 400, "Bad Request", &format!("{{\"error\":\"{}\"}}", json_escape(&e))),
            }
        }
        ("POST", "/api/upload") => {
            if !api_authorized(&target, &headers) {
                send_text(&mut stream, 401, "Unauthorized", "{\"error\":\"api token required\"}");
                return;
            }
            let filename = query_param(&target, "filename")
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| format!("upload-{}.dat", now_unix()));
            let safe: String = filename
                .chars()
                .map(|c| if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' })
                .collect();
            let dir = PathBuf::from("uploads");
            let _ = std::fs::create_dir_all(&dir);
            let dest = dir.join(safe.trim_start_matches('.'));
            match std::fs::write(&dest, &body) {
                Ok(_) => {
                    let b = format!("{{\"path\":\"{}\",\"bytes\":{}}}", json_escape(&dest.display().to_string()), body.len());
                    send(&mut stream, "application/json", b.as_bytes());
                }
                Err(e) => send_text(&mut stream, 500, "Error", &format!("{{\"error\":\"{}\"}}", json_escape(&e.to_string()))),
            }
        }
        ("GET", "/api/gen") => {
            // Demo data generator for sales demos (no CLI needed).
            let rows: usize = query_param(&target, "rows").and_then(|v| v.parse().ok()).unwrap_or(10_000);
            let mode = query_param(&target, "mode").unwrap_or_else(|| "string".to_string());
            let name = query_param(&target, "name").unwrap_or_else(|| "demo.txt".to_string());
            let rows = rows.clamp(1, 1_000_000);
            let dir = PathBuf::from("uploads");
            let _ = std::fs::create_dir_all(&dir);
            let safe: String = name.chars().map(|c| if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' }).collect();
            let dest = dir.join(safe.trim_start_matches('.'));
            match gen_demo(&dest, rows, &mode) {
                Ok(_) => {
                    let b = format!("{{\"path\":\"{}\",\"rows\":{}}}", json_escape(&dest.display().to_string()), rows);
                    send(&mut stream, "application/json", b.as_bytes());
                }
                Err(e) => send_text(&mut stream, 500, "Error", &format!("{{\"error\":\"{}\"}}", json_escape(&e))),
            }
        }
        ("GET", "/download") => {
            if !api_authorized(&target, &headers) {
                send_text(&mut stream, 401, "Unauthorized", "{\"error\":\"api token required\"}");
                return;
            }
            let p = query_param(&target, "path").unwrap_or_default();
            if p.is_empty() {
                send_text(&mut stream, 400, "Bad Request", "{\"error\":\"path required\"}");
                return;
            }
            let pb = PathBuf::from(&p);
            if !pb.is_file() {
                send_text(&mut stream, 404, "Not Found", "{\"error\":\"file not found\"}");
                return;
            }
            stream_file(&mut stream, &pb);
        }
        _ => send_text(&mut stream, 404, "Not Found", "{\"error\":\"unknown route. Try GET /\"}"),
    }
}

fn stream_file(stream: &mut TcpStream, pb: &Path) {
    let meta = match std::fs::metadata(pb) {
        Ok(m) => m,
        Err(e) => {
            send_text(stream, 500, "Error", &format!("{{\"error\":\"{}\"}}", json_escape(&e.to_string())));
            return;
        }
    };
    let fname = pb.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "download.dat".to_string());
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nContent-Disposition: attachment; filename=\"{}\"\r\nConnection: close\r\n\r\n",
        meta.len(),
        fname.replace('"', "_")
    );
    if stream.write_all(head.as_bytes()).is_err() {
        return;
    }
    let mut f = match std::fs::File::open(pb) {
        Ok(f) => f,
        Err(_) => return,
    };
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match f.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if stream.write_all(&buf[..n]).is_err() {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let _ = stream.flush();
}

/// Pulls the stored `params` blob out of a history-file entry for `hid`.
fn extract_history_params(txt: &str, hid: &str) -> Option<String> {
    let needle = format!("\"id\":\"{}\"", hid);
    let i = txt.find(&needle)?;
    let sub = &txt[i..];
    // params is JSON-escaped inside the entry; reuse jstr-like scan.
    let pat = "\"params\":\"";
    let j = sub.find(pat)? + pat.len();
    let bytes = sub.as_bytes();
    let mut out = String::new();
    let mut k = j;
    while k < bytes.len() {
        match bytes[k] {
            b'"' => return Some(out),
            b'\\' if k + 1 < bytes.len() => {
                match bytes[k + 1] {
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'u' if k + 5 < bytes.len() => {
                        if let Ok(s) = std::str::from_utf8(&bytes[k + 2..k + 6])
                            && let Ok(cp) = u32::from_str_radix(s, 16) {
                                out.push(char::from_u32(cp).unwrap_or('?'));
                            }
                        k += 4;
                    }
                    other => out.push(other as char),
                }
                k += 1;
            }
            other => out.push(other as char),
        }
        k += 1;
    }
    None
}

fn job_log_path(id: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("mergesort-gui");
    let _ = std::fs::create_dir_all(&dir);
    dir.join(format!("{}.log", id))
}

fn job_log_tail(id: &str, max: usize) -> String {
    let p = job_log_path(id);
    let data = std::fs::read(&p).unwrap_or_default();
    if data.len() <= max {
        return String::from_utf8_lossy(&data).into_owned();
    }
    format!("...[truncated]...\n{}", String::from_utf8_lossy(&data[data.len() - max..]))
}

#[allow(clippy::too_many_arguments)]
fn run_sort_job(id: String, log_path: PathBuf, input: String, output: String, max_memory: String, mode: String, key_column: String, key_type: String, reverse: bool, unique: bool, header: bool, verify: bool, limit: String, key_field: String) {
    {
        let mut map = jobs().lock().unwrap();
        if let Some(j) = map.get_mut(&id) {
            j.status = "running".to_string();
            j.detail = "sorting…".to_string();
        }
    }
    let t_start = now_unix();
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("mergesort"));
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--input").arg(&input).arg("--output").arg(&output).arg("--max-memory").arg(&max_memory);
    if mode == "csv" {
        cmd.arg("--format").arg("csv");
        if !key_column.is_empty() {
            cmd.arg("--key-column").arg(&key_column);
        } else {
            cmd.arg("--key-column").arg("0");
        }
        cmd.arg("--key-type").arg(&key_type);
    } else if mode == "numeric" {
        cmd.arg("--mode").arg("numeric");
    } else if mode == "jsonl" {
        cmd.arg("--mode").arg("jsonl");
        if !key_field.is_empty() {
            cmd.arg("--key-field").arg(&key_field);
        }
        cmd.arg("--key-type").arg(&key_type);
    } else {
        cmd.arg("--mode").arg("string");
    }
    if reverse {
        cmd.arg("--reverse");
    }
    if unique {
        cmd.arg("--unique");
    }
    if header {
        cmd.arg("--header");
    }
    if verify {
        cmd.arg("--verify");
    }
    if !limit.is_empty() && limit != "0" {
        cmd.arg("--limit").arg(&limit);
    }
    cmd.arg("--json");
    let out = cmd.output();
    let (ok, tail) = match out {
        Ok(o) => {
            let combined = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
            let _ = std::fs::write(&log_path, &combined);
            let detail = if o.status.success() {
                // --json prints a pretty multi-line object; the last line is
                // just "}" so pull the fields that matter instead.
                let records = jstr(&combined, "records").unwrap_or_else(|| "?".to_string());
                match jstr(&combined, "verified") {
                    Some(v) => format!("selesai: {} baris, verify={}", records, v),
                    None => format!("selesai: {} baris", records),
                }
            } else {
                combined.lines().last().unwrap_or("").to_string()
            };
            (o.status.success(), detail)
        }
        Err(e) => {
            let _ = std::fs::write(&log_path, e.to_string());
            (false, e.to_string())
        }
    };
    let dur = now_unix().saturating_sub(t_start);
    let mut hist_entry = String::new();
    {
        let mut map = jobs().lock().unwrap();
        if let Some(j) = map.get_mut(&id) {
            j.finished = now_unix();
            j.duration_s = dur;
            if ok {
                j.status = "done".to_string();
                j.detail = tail.chars().take(300).collect();
            } else {
                j.status = "error".to_string();
                j.detail = tail.chars().take(500).collect();
            }
            hist_entry = format!(
                "{{\"id\":\"{}\",\"time\":{},\"input\":\"{}\",\"output\":\"{}\",\"status\":\"{}\",\"duration_s\":{},\"params\":\"{}\"}}",
                json_escape(&j.id),
                j.started,
                json_escape(&j.input),
                json_escape(&j.output),
                json_escape(&j.status),
                j.duration_s,
                json_escape(&j.params),
            );
        }
    }
    if !hist_entry.is_empty() {
        append_history(&hist_entry);
    }
}

fn gen_demo(dest: &Path, rows: usize, mode: &str) -> Result<(), String> {
    use std::io::{BufWriter, Write};
    let f = std::fs::File::create(dest).map_err(|e| e.to_string())?;
    let mut w = BufWriter::with_capacity(1024 * 1024, f);
    let mut x: u64 = 0x9E3779B97F4A7C15;
    let mut rnd = move || {
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        x = x.wrapping_mul(0x2545F4914F6CDD1D);
        x
    };
    if mode == "csv" {
        w.write_all(b"id,name,city\n").map_err(|e| e.to_string())?;
        let cities = ["Jakarta", "Bandung", "Surabaya", "Medan", "Depok"];
        for i in 0..rows {
            let id = rnd();
            let city = cities[(rnd() % cities.len() as u64) as usize];
            let line = format!("{},{},user{},{}\n", id, i, i, city);
            w.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
        }
    } else {
        for _ in 0..rows {
            let len = (rnd() % 15 + 3) as usize;
            let mut s = String::with_capacity(len);
            for _ in 0..len {
                s.push((b'a' + (rnd() % 26) as u8) as char);
            }
            w.write_all(s.as_bytes()).map_err(|e| e.to_string())?;
            w.write_all(b"\n").map_err(|e| e.to_string())?;
        }
    }
    w.flush().map_err(|e| e.to_string())?;
    Ok(())
}

const GUI_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>MergeSort Pro — Console</title>
<style>
:root{--bg:#0b0f14;--card:#141b24;--line:#223041;--txt:#dbe4f0;--mut:#8fa1b5;--acc:#56c2d6;--ok:#a3be8c;--err:#bf616a}
*{box-sizing:border-box}body{font-family:ui-sans-serif,system-ui,Segoe UI,Roboto,Arial;background:var(--bg);color:var(--txt);margin:0}
header{display:flex;justify-content:space-between;align-items:center;padding:16px 24px;background:#0e141d;border-bottom:1px solid var(--line);position:sticky;top:0}
.brand{font-weight:800}.brand span{color:var(--acc)}
main{max-width:1020px;margin:0 auto;padding:20px}
.card{background:var(--card);border:1px solid var(--line);border-radius:14px;padding:18px;margin-bottom:16px}
h2{margin:0 0 12px;font-size:15px;text-transform:uppercase;letter-spacing:.6px;color:var(--mut)}
label{display:block;font-size:12px;color:var(--mut);margin:10px 0 4px}
input,select{width:100%;padding:9px 10px;border-radius:8px;border:1px solid var(--line);background:#0f1722;color:var(--txt)}
.grid2{display:grid;grid-template-columns:1fr 1fr;gap:12px}.grid3{display:grid;grid-template-columns:1fr 1fr 1fr;gap:12px}
.row{display:flex;gap:10px;align-items:center;margin-top:12px;flex-wrap:wrap}
button{background:var(--acc);border:0;color:#06222a;font-weight:800;padding:10px 18px;border-radius:10px;cursor:pointer}
button.ghost{background:transparent;color:var(--txt);border:1px solid var(--line)}
button:disabled{opacity:.5;cursor:wait}
#drop{border:2px dashed var(--line);border-radius:12px;padding:22px;text-align:center;color:var(--mut);margin-top:10px}
#drop.over{border-color:var(--acc);color:var(--txt)}
table{width:100%;border-collapse:collapse;font-size:13px}th,td{text-align:left;padding:8px;border-bottom:1px solid var(--line)}
.badge{padding:2px 10px;border-radius:999px;background:#1c2836;font-size:12px}.done{color:var(--ok)}.error{color:var(--err)}
.mut{color:var(--mut);font-size:12px}code{background:#0f1722;padding:2px 6px;border-radius:6px;border:1px solid var(--line)}
pre{background:#0b1119;border:1px solid var(--line);border-radius:10px;padding:12px;max-height:220px;overflow:auto;font-size:12px}
.hint{color:var(--mut);font-size:12px;margin-top:4px}
details{margin-top:14px;border-top:1px solid var(--line);padding-top:10px}
summary{cursor:pointer;color:var(--mut);font-size:13px;user-select:none}
.detected{display:inline-block;padding:3px 10px;border-radius:999px;background:#1c2836;font-size:12px;margin-top:6px}
@media(max-width:700px){.grid2,.grid3{grid-template-columns:1fr}}
</style>
</head>
<body>
<header><div class="brand">MergeSort <span>Pro</span> — Console</div><div class="mut" id="status">…</div></header>
<main>
<div class="card"><h2>1 · Data (drag-drop atau path server)</h2>
<div id="drop">Drop file di sini (upload ke server, cocok untuk demo ≤500MB)<br><span class="mut">atau isi path langsung di bawah untuk file besar (TB-scale)</span></div>
<div class="grid2">
<div><label>Input path (server)</label><input id="input" placeholder="uploads/demo.txt atau C:\data\big.csv"></div>
<div><label>Output path (server)</label><input id="output" placeholder="uploads/sorted.txt"></div>
</div>
<div class="row"><button class="ghost" onclick="genDemo()">Generate demo 10k</button><button class="ghost" onclick="genCsv()">Generate demo CSV</button><span class="mut" id="upmsg"></span></div>
</div>
<div class="card"><h2>2 · Cara mengurutkan</h2>
<div id="detectedBadge" class="mut" style="display:none">Terdeteksi: <span id="detectedText" class="detected"></span></div>
<div class="grid3">
<div><label>Jenis data</label><select id="mode" onchange="onModeChange()"><option value="string">Teks biasa (1 baris = 1 nilai)</option><option value="csv">Tabel / CSV (kolom-kolom)</option><option value="jsonl">JSONL (1 baris = 1 objek JSON)</option><option value="numeric">Angka biner (lanjutan)</option></select></div>
<div id="csvColWrap"><label>Urutkan berdasarkan kolom</label><select id="keycolname"></select></div>
<div id="jsonlFieldWrap" style="display:none"><label>Field JSON (titik untuk nested, mis. user.age)</label><input id="keyfield" value="" placeholder="user.age"></div>
</div>
<div class="grid3">
<div id="keytypeWrap"><label>Tipe nilai kolom</label><select id="keytype"><option value="string">Teks (urut A → Z)</option><option value="numeric">Angka (urut 1, 2, 3, ...)</option><option value="date">Tanggal (format YYYY-MM-DD)</option></select></div>
<div id="headerWrap"><label>Baris pertama judul kolom?</label><select id="header"><option value="false">Tidak, semua baris data</option><option value="true">Ya, baris 1 = judul</option></select></div>
<div><label>Urutan</label><select id="reverse"><option value="false">Kecil → Besar / A → Z</option><option value="true">Besar → Kecil / Z → A</option></select></div>
</div>
<div class="row">
<label style="display:flex;gap:6px;align-items:center"><input type="checkbox" id="unique" style="width:auto"> Buang baris yang sama persis (unique)</label>
<button onclick="startSort()">Urutkan sekarang</button>
<span class="mut">CLI setara ditampilkan di Jobs → log</span>
</div>
<details>
<summary>⚙ Pengaturan lanjutan (opsional — default sudah aman)</summary>
<div class="grid3">
<div><label>Batas memori (makin besar = makin cepat, tapi pakai lebih banyak RAM)</label><select id="mem"><option>256MB</option><option selected>512MB</option><option>1GB</option><option>2GB</option><option>8MB</option></select></div>
<div><label>Ambil hanya N baris teratas (kosong = semua)</label><input id="limit" placeholder="mis. 1000"></div>
<div><label>Cek ulang hasil setelah selesai (disarankan)</label><select id="verify"><option value="true" selected>Ya</option><option value="false">Tidak</option></select></div>
</div>
<p class="hint">Kolom terdeteksi otomatis dari isi file. Kalau salah, isi <b>Input path</b> ulang atau pilih manual di atas.</p>
</details>
</div>
<div class="card"><h2>3 · Preset (sekali setup, selamanya klik)</h2>
<div class="grid2">
<div><label>Preset tersimpan</label><select id="preset"></select></div>
<div><label>Nama preset baru</label><input id="presetname" placeholder="Laporan Penjualan Bulanan"></div>
</div>
<div class="row"><button class="ghost" onclick="applyPreset()">Pakai preset</button><button class="ghost" onclick="savePreset()">Simpan preset</button><button class="ghost" onclick="delPreset()">Hapus</button><button class="ghost" onclick="exportPresets()">Export</button><button class="ghost" onclick="importPresets()">Import</button><input type="file" id="presetfile" style="display:none" accept=".json"><span class="mut">tersimpan di browser (localStorage)</span></div>
</div>
<div class="card"><h2>4 · Jobs (live)</h2>
<div class="row"><button class="ghost" onclick="refresh()">Refresh</button><span class="mut">auto-refresh 2s</span></div>
<table><thead><tr><th>ID</th><th>Input → Output</th><th>Status</th><th>Detail</th><th>Aksi</th></tr></thead><tbody id="jobs"></tbody></table>
<pre id="log">pilih job untuk melihat log…</pre>
</div>
<div class="card"><h2>5 · Riwayat (audit + jalankan lagi)</h2>
<div class="row"><button class="ghost" onclick="loadHistory()">Muat riwayat</button><button class="ghost" onclick="clearHistory()">Hapus riwayat</button><span class="mut">maksimal 100 terakhir, tersimpan di server</span></div>
<table><thead><tr><th>Waktu</th><th>Input → Output</th><th>Status</th><th>Durasi</th><th>Aksi</th></tr></thead><tbody id="hist"></tbody></table>
</div>
<div class="card"><h2>Bantuan</h2>
<p class="mut">File besar? Isi <code>input/output</code> sebagai path server (mis. <code>C:\data\big.csv</code>), bukan upload. Upload hanya untuk demo. Hasil bisa diunduh via tombol Download. Audit: tiap run bisa <code>--log-file audit.jsonl</code>.</p>
</div>
</main>
<script>
async function api(p,o){const r=await fetch(p,o);return r.json();}
async function refreshStatus(){try{const s=await api('/api/status');document.getElementById('status').textContent='v'+s.version+' · '+s.jobs+' jobs';}catch(e){}}
async function refresh(){try{const j=await api('/api/jobs');const tb=document.getElementById('jobs');tb.innerHTML='';(j.jobs||[]).slice().reverse().forEach(job=>{const tr=document.createElement('tr');tr.innerHTML='<td><code>'+job.id+'</code></td><td>'+job.input+' → '+job.output+'</td><td><span class="badge '+job.status+'">'+job.status+'</span></td><td>'+(job.detail||'').slice(0,120)+'</td>';const td=document.createElement('td');const b1=document.createElement('button');b1.className='ghost';b1.textContent='Log';b1.onclick=()=>showLog(job.id);const b2=document.createElement('button');b2.className='ghost';b2.textContent='Download';b2.onclick=()=>{window.location='/download?path='+encodeURIComponent(job.output);};td.appendChild(b1);td.appendChild(b2);tr.appendChild(td);tb.appendChild(tr);});}catch(e){}}
async function showLog(id){const j=await api('/api/job?id='+encodeURIComponent(id));document.getElementById('log').textContent=j.log||j.detail||'-';}
function formState(){return{input:document.getElementById('input').value,output:document.getElementById('output').value,max_memory:document.getElementById('mem').value,mode:document.getElementById('mode').value,key_column:document.getElementById('keycolname').value,key_field:document.getElementById('keyfield').value,key_type:document.getElementById('keytype').value,limit:document.getElementById('limit').value,verify:document.getElementById('verify').value,reverse:document.getElementById('reverse').value,unique:document.getElementById('unique').checked?'true':'false',header:document.getElementById('header').value};}
function applyState(s){if(!s)return;for(const[k,v]of Object.entries(s)){const el=document.getElementById({input:'input',output:'output',max_memory:'mem',mode:'mode',key_column:'keycolname',key_field:'keyfield',key_type:'keytype',limit:'limit',verify:'verify'}[k]||k);if(!el)continue;if(el.type==='checkbox'){el.checked=(v==='true');}else{el.value=v;}}onModeChange();}
function getPresets(){try{return JSON.parse(localStorage.getItem('mergesort-presets')||'{}');}catch(e){return{};}}
function setPresets(p){localStorage.setItem('mergesort-presets',JSON.stringify(p));renderPresets();}
function renderPresets(){const p=getPresets();const sel=document.getElementById('preset');sel.innerHTML='';Object.keys(p).forEach(n=>{const o=document.createElement('option');o.value=n;o.textContent=n;sel.appendChild(o);});}
function savePreset(){const n=document.getElementById('presetname').value.trim();if(!n){alert('isi nama preset dulu');return;}const p=getPresets();p[n]=formState();setPresets(p);}
function applyPreset(){const n=document.getElementById('preset').value;applyState(getPresets()[n]);}
function delPreset(){const n=document.getElementById('preset').value;const p=getPresets();delete p[n];setPresets(p);}
function exportPresets(){const blob=new Blob([localStorage.getItem('mergesort-presets')||'{}'],{type:'application/json'});const a=document.createElement('a');a.href=URL.createObjectURL(blob);a.download='mergesort-presets.json';a.click();}
function importPresets(){document.getElementById('presetfile').click();}
document.getElementById('presetfile').addEventListener('change',ev=>{const f=ev.target.files[0];if(!f)return;const r=new FileReader();r.onload=()=>{try{const p=JSON.parse(r.result);if(typeof p==='object'){setPresets(p);}}catch(e){alert('file preset invalid');}};r.readAsText(f);});
async function loadHistory(){try{const h=await api('/api/history');const tb=document.getElementById('hist');tb.innerHTML='';(Array.isArray(h)?h:[].concat(h.jobs||[])).slice().reverse().forEach(e=>{const tr=document.createElement('tr');const dt=new Date((e.time||0)*1000).toLocaleString();tr.innerHTML='<td>'+dt+'</td><td>'+(e.input||'')+' → '+(e.output||'')+'</td><td><span class="badge '+(e.status||'')+'">'+(e.status||'')+'</span></td><td>'+(e.duration_s||0)+'s</td>';const td=document.createElement('td');const b=document.createElement('button');b.className='ghost';b.textContent='Jalankan lagi';b.onclick=async()=>{await api('/api/rerun',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({id:e.id})});refresh();};td.appendChild(b);tr.appendChild(td);tb.appendChild(tr);});}catch(e){}}
async function clearHistory(){if(!confirm('Hapus seluruh riwayat?'))return;await api('/api/history/clear',{method:'POST'});loadHistory();}
renderPresets();loadHistory();
async function startSort(){const body={input:document.getElementById('input').value.trim(),output:document.getElementById('output').value.trim(),max_memory:document.getElementById('mem').value,mode:document.getElementById('mode').value,key_column:document.getElementById('keycolname').value,key_field:document.getElementById('keyfield').value.trim(),key_type:document.getElementById('keytype').value,limit:document.getElementById('limit').value.trim(),verify:document.getElementById('verify').value,reverse:document.getElementById('reverse').value,unique:document.getElementById('unique').checked?'true':'false',header:document.getElementById('header').value};if(!body.input||!body.output){alert('isi input & output dulu');return;}const r=await api('/api/sort',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)});if(r.error){alert(r.error);}else{refresh();showLog(r.id);}}
function onModeChange(){const m=document.getElementById('mode').value;document.getElementById('csvColWrap').style.display=(m==='csv')?'':'none';document.getElementById('jsonlFieldWrap').style.display=(m==='jsonl')?'':'none';document.getElementById('keytypeWrap').style.display=(m==='csv'||m==='jsonl')?'':'none';document.getElementById('headerWrap').style.display=(m==='numeric')?'none':'';if(m==='numeric')document.getElementById('header').value='false';}
async function doInspect(path){if(!path)return;const q=await api('/api/inspect?path='+encodeURIComponent(path));document.getElementById('mode').value=q.format;onModeChange();document.getElementById('header').value=q.header?'true':'false';const sel=document.getElementById('keycolname');sel.innerHTML='';(q.columns||[]).forEach(c=>{const o=document.createElement('option');o.value=c.index;o.textContent=c.name+(c.example?' (contoh: '+c.example+')':'');sel.appendChild(o);});const badge=document.getElementById('detectedBadge');if(q.format==='csv'){const label=q.format.toUpperCase()+', delimiter \''+q.delimiter+'\', '+(q.columns||[]).length+' kolom, header: '+(q.header?'ya':'tidak');document.getElementById('detectedText').textContent=label;badge.style.display='';}else{badge.style.display='none';}}
async function genDemo(){const r=await api('/api/gen?rows=10000&mode=string&name=demo.txt');if(r.path){document.getElementById('input').value=r.path;document.getElementById('output').value='uploads/sorted.txt';document.getElementById('upmsg').textContent='demo: '+r.path;await doInspect(r.path);}}
async function genCsv(){const r=await api('/api/gen?rows=10000&mode=csv&name=demo.csv');if(r.path){document.getElementById('input').value=r.path;document.getElementById('output').value='uploads/sorted.csv';document.getElementById('upmsg').textContent='demo csv: '+r.path;await doInspect(r.path);}}
document.getElementById('input').addEventListener('change',ev=>doInspect(ev.target.value.trim()));
const drop=document.getElementById('drop');['dragover','dragenter'].forEach(e=>drop.addEventListener(e,ev=>{ev.preventDefault();drop.classList.add('over');}));['dragleave','drop'].forEach(e=>drop.addEventListener(e,ev=>{ev.preventDefault();drop.classList.remove('over');}));drop.addEventListener('drop',async ev=>{const f=ev.dataTransfer.files[0];if(!f)return;document.getElementById('upmsg').textContent='uploading '+f.name+'…';const buf=await f.arrayBuffer();const r=await fetch('/api/upload?filename='+encodeURIComponent(f.name),{method:'POST',body:buf});const j=await r.json();if(j.path){document.getElementById('input').value=j.path;document.getElementById('upmsg').textContent='uploaded: '+j.path;await doInspect(j.path);}});
onModeChange(); 
setInterval(()=>{refreshStatus();refresh();},2000);refreshStatus();refresh();
</script>
</body>
</html>
"#;
