//! JSON → schema+CSV tabular compression for large, homogeneous tool outputs.
//!
//! v1.7.235 — ported from the Headroom pilot (2026-07-09). A verbose JSON array
//! of uniform objects — Lucy's classic tool output (`inventory`, process lists,
//! `cve_match` rows) — repeats every key on every element. This transform hoists
//! the keys into a one-time schema header and emits each object as a CSV row:
//!
//! ```text
//! [JSON-TABLE v1] {"key":"installed_software","extra":{"count":160},"cols":["name","version",...],"n":160}
//! App000,1.4.31,...
//! App001,2.0.11,...
//! [/JSON-TABLE]
//! ```
//!
//! Measured ~43% token reduction on a 160-row inventory, LOSSLESS. Unlike the
//! upstream Python tool it needs no ML, no sidecar, and fires deterministically
//! on a threshold we control — so it's most valuable exactly where Lucy needs it:
//! small-context local models (e.g. qwen3:8b @ 40k) in long agent loops.
//!
//! # Design invariants
//! - **Lossless**: `decompress(compress(x)) == x` (semantic JSON equality). Guarded
//!   by round-trip tests. Type fidelity is preserved: the string `"5"` never
//!   collapses into the number `5`, `null` never collapses into `""`.
//! - **Safe passthrough**: anything that isn't a big, homogeneous, scalar-valued
//!   array of objects returns `None` — the caller keeps the original untouched.
//! - **No data invention**: V1 requires every object to share the exact same key
//!   set (ragged rows → passthrough), so decompression rebuilds byte-for-byte keys.

use serde_json::{Map, Number, Value};

const MARKER_START: &str = "[JSON-TABLE v1]";
const MARKER_END: &str = "[/JSON-TABLE]";

#[derive(Clone, Copy)]
pub struct TabularOpts {
    /// Don't bother compressing an array with fewer rows than this.
    pub min_rows: usize,
    /// Don't bother if the whole input is smaller than this (chars).
    pub min_input_chars: usize,
    /// Only return the compressed form if it saves at least this fraction.
    pub min_savings: f64,
}

impl Default for TabularOpts {
    fn default() -> Self {
        TabularOpts { min_rows: 8, min_input_chars: 800, min_savings: 0.12 }
    }
}

/// True if `s` would round-trip as a JSON number (so a String that looks like a
/// number must be quoted to stay a String).
fn is_number_like(s: &str) -> bool {
    serde_json::from_str::<Number>(s).is_ok()
}

/// Render one scalar JSON value as a CSV cell (RFC 4180). Strings are quoted only
/// when necessary to (a) survive CSV or (b) avoid being re-read as number/bool/null.
fn encode_cell(v: &Value, out: &mut String) {
    match v {
        Value::Null => { /* empty unquoted field == null */ }
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => {
            let must_quote = s.is_empty()
                || s == "true"
                || s == "false"
                || is_number_like(s)
                || s.contains(',')
                || s.contains('"')
                || s.contains('\n')
                || s.contains('\r');
            if must_quote {
                out.push('"');
                out.push_str(&s.replace('"', "\"\""));
                out.push('"');
            } else {
                out.push_str(s);
            }
        }
        // Non-scalar cells never reach here (validated before compression).
        _ => {}
    }
}

/// Inverse of `encode_cell`. `quoted` marks whether the source field was wrapped
/// in double quotes (→ always a String); otherwise infer null/bool/number/string.
fn decode_cell(quoted: bool, text: &str) -> Value {
    if quoted {
        return Value::String(text.to_string());
    }
    if text.is_empty() {
        return Value::Null;
    }
    if text == "true" {
        return Value::Bool(true);
    }
    if text == "false" {
        return Value::Bool(false);
    }
    if let Ok(n) = serde_json::from_str::<Number>(text) {
        return Value::Number(n);
    }
    Value::String(text.to_string())
}

/// Is this a compressible array — a run of objects that all share the exact same
/// set of scalar-valued keys? Returns the sorted column list if so.
fn tabular_columns(arr: &[Value], min_rows: usize) -> Option<Vec<String>> {
    if arr.len() < min_rows {
        return None;
    }
    let first = arr.first()?.as_object()?;
    // All values in the first object must be scalars.
    if first.values().any(|v| v.is_array() || v.is_object()) {
        return None;
    }
    let cols: Vec<String> = first.keys().cloned().collect(); // serde_json Map is sorted
    if cols.is_empty() {
        return None;
    }
    // v1.7.236 (audit): refuse SINGLE-column arrays. A null in a one-column
    // table encodes as an empty CSV line; a first/last empty line is
    // indistinguishable from the leading/trailing newline trimming the
    // decompress side does, so a boundary null would silently DROP a row on
    // round-trip. Multi-column rows always contain a comma, so they are
    // unaffected — and a one-column array barely benefits from tabular packing
    // anyway (CSV of one column ≈ the JSON minus braces).
    if cols.len() == 1 {
        return None;
    }
    for el in arr {
        let obj = el.as_object()?;
        if obj.len() != cols.len() {
            return None; // ragged → passthrough (V1)
        }
        for k in &cols {
            match obj.get(k) {
                Some(v) if !(v.is_array() || v.is_object()) => {}
                _ => return None, // missing key or non-scalar value
            }
        }
    }
    Some(cols)
}

/// Compress `input` (a JSON string) to the tabular form, or `None` to pass through
/// unchanged. `key` is the array's field name when the root is an object wrapping
/// a single array; `None` when the root *is* the array.
pub fn compress_tool_json(input: &str, opts: &TabularOpts) -> Option<String> {
    if input.len() < opts.min_input_chars {
        return None;
    }
    let root: Value = serde_json::from_str(input).ok()?;

    let (key, arr, extra): (Option<String>, &Vec<Value>, Option<Value>) = match &root {
        Value::Array(a) => (None, a, None),
        Value::Object(map) => {
            // Pick the single largest array-of-objects field; everything else is `extra`.
            let mut best: Option<(&String, &Vec<Value>)> = None;
            for (k, v) in map {
                if let Value::Array(a) = v {
                    if a.first().map(|e| e.is_object()).unwrap_or(false)
                        && best.map(|(_, b)| a.len() > b.len()).unwrap_or(true) {
                            best = Some((k, a));
                        }
                }
            }
            let (k, a) = best?;
            let mut rest = map.clone();
            rest.remove(k);
            let extra = if rest.is_empty() { None } else { Some(Value::Object(rest)) };
            (Some(k.clone()), a, extra)
        }
        _ => return None,
    };

    let cols = tabular_columns(arr, opts.min_rows)?;

    // Header line: marker + machine-readable metadata (single line).
    let meta = serde_json::json!({
        "key": key,
        "extra": extra,
        "cols": cols,
        "n": arr.len(),
    });
    let mut out = String::with_capacity(input.len() / 2);
    out.push_str(MARKER_START);
    out.push(' ');
    out.push_str(&serde_json::to_string(&meta).ok()?);
    out.push('\n');
    for el in arr {
        let obj = el.as_object()?;
        for (i, c) in cols.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            encode_cell(obj.get(c).unwrap_or(&Value::Null), &mut out);
        }
        out.push('\n');
    }
    out.push_str(MARKER_END);

    // Only worth it if we actually saved enough.
    if (out.len() as f64) <= (input.len() as f64) * (1.0 - opts.min_savings) {
        Some(out)
    } else {
        None
    }
}

/// Parse a CSV body (RFC 4180) into up to `n` records of `(quoted, text)` cells.
/// Handles embedded commas/quotes/newlines inside quoted fields.
fn parse_csv(body: &str, n: usize) -> Vec<Vec<(bool, String)>> {
    let mut rows: Vec<Vec<(bool, String)>> = Vec::with_capacity(n);
    let mut row: Vec<(bool, String)> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut was_quoted = false;
    let mut field_started = false;
    let mut chars = body.chars().peekable();
    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(ch);
            }
        } else {
            match ch {
                '"' if !field_started => {
                    in_quotes = true;
                    was_quoted = true;
                    field_started = true;
                }
                ',' => {
                    row.push((was_quoted, std::mem::take(&mut field)));
                    was_quoted = false;
                    field_started = false;
                }
                '\r' => { /* ignore CR; LF ends the record */ }
                '\n' => {
                    row.push((was_quoted, std::mem::take(&mut field)));
                    rows.push(std::mem::take(&mut row));
                    was_quoted = false;
                    field_started = false;
                    if rows.len() >= n {
                        return rows;
                    }
                }
                _ => {
                    field.push(ch);
                    field_started = true;
                }
            }
        }
    }
    // Trailing record without a final newline.
    if field_started || was_quoted || !row.is_empty() {
        row.push((was_quoted, field));
        rows.push(row);
    }
    rows.truncate(n);
    rows
}

/// Inverse of `compress_tool_json`. Returns `None` if `input` isn't a JSON-TABLE
/// block (so the caller can treat it as already-plain content).
pub fn decompress_tool_json(input: &str) -> Option<String> {
    let trimmed = input.trim_start();
    if !trimmed.starts_with(MARKER_START) {
        return None;
    }
    let after_marker = &trimmed[MARKER_START.len()..];
    let nl = after_marker.find('\n')?;
    let header = after_marker[..nl].trim();
    let mut body = &after_marker[nl + 1..];
    // Strip the end marker (and anything after it) if present.
    if let Some(pos) = body.rfind(MARKER_END) {
        body = &body[..pos];
    }
    let meta: Value = serde_json::from_str(header).ok()?;
    let cols: Vec<String> = meta.get("cols")?.as_array()?
        .iter().map(|c| c.as_str().unwrap_or("").to_string()).collect();
    let n = meta.get("n")?.as_u64()? as usize;

    let records = parse_csv(body.trim_matches('\n'), n);
    // v1.7.236 (audit): hard guard against SILENT row loss. `n` is the row count
    // the compressor recorded; if parsing yields a different number (e.g. a
    // boundary empty line eaten by trim on an older single-column blob), refuse
    // rather than hand back a short array that looks complete.
    if records.len() != n {
        return None;
    }
    let mut arr: Vec<Value> = Vec::with_capacity(records.len());
    for rec in &records {
        if rec.len() != cols.len() {
            return None; // corrupt / mismatched — refuse rather than guess
        }
        let mut obj = Map::new();
        for (c, (quoted, text)) in cols.iter().zip(rec.iter()) {
            obj.insert(c.clone(), decode_cell(*quoted, text));
        }
        arr.push(Value::Object(obj));
    }

    let array_val = Value::Array(arr);
    let root = match meta.get("key") {
        Some(Value::String(k)) => {
            let mut map = match meta.get("extra") {
                Some(Value::Object(m)) => m.clone(),
                _ => Map::new(),
            };
            map.insert(k.clone(), array_val);
            Value::Object(map)
        }
        _ => array_val,
    };
    serde_json::to_string(&root).ok()
}

// ── Tauri commands ────────────────────────────────────────────────────────────

/// Max length of a leading label (e.g. `[INVENTORY]\n`) we'll preserve while
/// compressing the JSON body after it. Longer prefixes are treated as prose and
/// left alone.
const MAX_LABEL_PREFIX: usize = 200;

/// Compress a tool output if it's a big homogeneous JSON array; else return it
/// unchanged. Tolerates a short leading label before the JSON body. Safe by
/// construction — worst case is a no-op passthrough.
#[tauri::command]
pub fn compress_tool_output(text: String) -> String {
    let opts = TabularOpts::default();
    // Fast path: the whole string is JSON.
    if let Some(c) = compress_tool_json(&text, &opts) {
        return c;
    }
    // Tolerate a short label like "[INVENTORY]\n{...}". Candidate body starts:
    // after the first newline (label on its own line — the common case), then the
    // first '{'. We deliberately do NOT treat the label's own '[' as a body start.
    let mut starts: Vec<usize> = Vec::new();
    if let Some(nl) = text.find('\n') {
        starts.push(nl + 1);
    }
    if let Some(br) = text.find('{') {
        starts.push(br);
    }
    for start in starts {
        if start == 0 || start > MAX_LABEL_PREFIX {
            continue;
        }
        let (prefix, body) = text.split_at(start);
        if let Some(c) = compress_tool_json(body, &opts) {
            return format!("{prefix}{c}");
        }
    }
    text
}

/// Expand a JSON-TABLE block back to JSON; if `text` isn't such a block, return it
/// unchanged (so it's safe to call on any string). Handles a short leading label.
#[tauri::command]
pub fn expand_tool_output(text: String) -> String {
    if let Some(j) = decompress_tool_json(&text) {
        return j;
    }
    if let Some(idx) = text.find(MARKER_START) {
        if idx <= MAX_LABEL_PREFIX {
            let (prefix, body) = text.split_at(idx);
            if let Some(j) = decompress_tool_json(body) {
                return format!("{prefix}{j}");
            }
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn inventory(n: usize) -> String {
        let pubs = ["Microsoft Corporation", "Google LLC", "Adobe Inc.", "NVIDIA", "Oracle"];
        let mut apps = Vec::new();
        for i in 0..n {
            apps.push(json!({
                "name": format!("App{:03}-Pro", i),
                "version": format!("{}.{}.{}", i % 20, i % 10, i % 100),
                "publisher": pubs[i % pubs.len()],
                "size_mb": (i as f64) * 3.5 + 2.0,
                "install_location": format!("C:\\Program Files\\App{:03}", i),
                "uninstall_string": format!("\"C:\\Program Files\\App{:03}\\uninstall.exe\" /S", i),
            }));
        }
        serde_json::to_string_pretty(&json!({"installed_software": apps, "count": n})).unwrap()
    }

    fn semantically_equal(a: &str, b: &str) -> bool {
        let va: Value = serde_json::from_str(a).unwrap();
        let vb: Value = serde_json::from_str(b).unwrap();
        va == vb
    }

    #[test]
    fn roundtrip_inventory_is_lossless() {
        let orig = inventory(160);
        let comp = compress_tool_json(&orig, &TabularOpts::default()).expect("should compress");
        assert!(comp.starts_with(MARKER_START));
        let back = decompress_tool_json(&comp).expect("should decompress");
        assert!(semantically_equal(&orig, &back), "round-trip must be lossless");
    }

    #[test]
    fn inventory_saves_meaningful_tokens() {
        let orig = inventory(160);
        let comp = compress_tool_json(&orig, &TabularOpts::default()).unwrap();
        let ratio = 1.0 - (comp.len() as f64 / orig.len() as f64);
        assert!(ratio > 0.30, "expected >30% char reduction, got {:.1}%", ratio * 100.0);
    }

    #[test]
    fn root_array_without_wrapper_roundtrips() {
        let arr = json!([
            {"a": 1, "b": "x"}, {"a": 2, "b": "y"}, {"a": 3, "b": "z"},
            {"a": 4, "b": "w"}, {"a": 5, "b": "v"}, {"a": 6, "b": "u"},
            {"a": 7, "b": "t"}, {"a": 8, "b": "s"}, {"a": 9, "b": "r"},
        ]);
        let orig = serde_json::to_string(&arr).unwrap();
        let opts = TabularOpts { min_rows: 8, min_input_chars: 0, min_savings: 0.0 };
        let comp = compress_tool_json(&orig, &opts).unwrap();
        let back = decompress_tool_json(&comp).unwrap();
        assert!(semantically_equal(&orig, &back));
    }

    #[test]
    fn type_fidelity_preserved() {
        // string "5" must stay a string; number 5 must stay a number; null vs "".
        let arr = json!([
            {"s": "5", "n": 5, "b": "true", "flag": true, "nul": null, "empty": ""},
            {"s": "10", "n": 10, "b": "false", "flag": false, "nul": null, "empty": ""},
            {"s": "007", "n": 7, "b": "x", "flag": true, "nul": null, "empty": ""},
            {"s": "1.2.3", "n": 3.5, "b": "y", "flag": false, "nul": null, "empty": ""},
            {"s": "6", "n": 6, "b": "z", "flag": true, "nul": null, "empty": ""},
            {"s": "8", "n": 8, "b": "w", "flag": false, "nul": null, "empty": ""},
            {"s": "9", "n": 9, "b": "v", "flag": true, "nul": null, "empty": ""},
            {"s": "2", "n": 2, "b": "u", "flag": false, "nul": null, "empty": ""},
        ]);
        let orig = serde_json::to_string(&arr).unwrap();
        let opts = TabularOpts { min_rows: 8, min_input_chars: 0, min_savings: 0.0 };
        let comp = compress_tool_json(&orig, &opts).unwrap();
        let back = decompress_tool_json(&comp).unwrap();
        assert!(semantically_equal(&orig, &back), "types must survive; got {}", back);
    }

    #[test]
    fn values_with_commas_quotes_newlines_roundtrip() {
        let arr = json!([
            {"k": "a,b,c", "q": "she said \"hi\"", "m": "line1\nline2"},
            {"k": "plain", "q": "no special", "m": "x"},
            {"k": ",leading", "q": "trailing,", "m": "a\r\nb"},
            {"k": "\"quoted\"", "q": "", "m": "z"},
            {"k": "e", "q": "f", "m": "g"},
            {"k": "h", "q": "i", "m": "j"},
            {"k": "kk", "q": "ll", "m": "mm"},
            {"k": "nn", "q": "oo", "m": "pp"},
        ]);
        let orig = serde_json::to_string(&arr).unwrap();
        let opts = TabularOpts { min_rows: 8, min_input_chars: 0, min_savings: 0.0 };
        let comp = compress_tool_json(&orig, &opts).unwrap();
        let back = decompress_tool_json(&comp).unwrap();
        assert!(semantically_equal(&orig, &back), "special chars must survive; got {}", back);
    }

    #[test]
    fn passthrough_small_input() {
        assert!(compress_tool_json("{\"a\":[1,2,3]}", &TabularOpts::default()).is_none());
    }

    #[test]
    fn passthrough_non_json() {
        let opts = TabularOpts { min_rows: 2, min_input_chars: 0, min_savings: 0.0 };
        assert!(compress_tool_json("PS C:\\> Get-Service\nStatus  Name", &opts).is_none());
    }

    #[test]
    fn passthrough_ragged_objects() {
        // Different key sets → not tabular → passthrough.
        let arr = json!([
            {"a": 1, "b": 2}, {"a": 3}, {"a": 4, "b": 5}, {"a": 6, "b": 7},
            {"a": 8, "b": 9}, {"a": 10, "b": 11}, {"a": 12, "b": 13}, {"a": 14, "b": 15},
        ]);
        let orig = serde_json::to_string(&arr).unwrap();
        let opts = TabularOpts { min_rows: 2, min_input_chars: 0, min_savings: 0.0 };
        assert!(compress_tool_json(&orig, &opts).is_none());
    }

    #[test]
    fn passthrough_nested_values() {
        // Objects contain nested arrays/objects → not scalar-tabular → passthrough.
        let arr = json!([
            {"a": 1, "b": [1, 2]}, {"a": 2, "b": [3]}, {"a": 3, "b": []},
            {"a": 4, "b": [4]}, {"a": 5, "b": [5]}, {"a": 6, "b": [6]},
            {"a": 7, "b": [7]}, {"a": 8, "b": [8]},
        ]);
        let orig = serde_json::to_string(&arr).unwrap();
        let opts = TabularOpts { min_rows: 2, min_input_chars: 0, min_savings: 0.0 };
        assert!(compress_tool_json(&orig, &opts).is_none());
    }

    #[test]
    fn passthrough_single_column_with_boundary_null() {
        // v1.7.236 (audit): a single-column array whose FIRST (or last) element
        // is null used to round-trip lossily — the null encodes as an empty CSV
        // line and the decompress-side newline trim ate it, dropping the row.
        // A one-column array must now PASS THROUGH uncompressed, so the data is
        // preserved verbatim.
        let arr = json!([
            {"v": null}, {"v": "a"}, {"v": "b"}, {"v": "c"},
            {"v": "d"}, {"v": "e"}, {"v": "f"}, {"v": null},
        ]);
        let orig = serde_json::to_string(&arr).unwrap();
        let opts = TabularOpts { min_rows: 2, min_input_chars: 0, min_savings: 0.0 };
        assert!(
            compress_tool_json(&orig, &opts).is_none(),
            "single-column arrays must pass through, not compress"
        );
        // And the public entry point must return the input unchanged (no loss).
        let out = expand_tool_output(compress_tool_output_or_self(&orig, &opts));
        let back: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(back, arr, "single-column data must survive verbatim");
    }

    /// Small helper mirroring the command's "compress or return self" behaviour
    /// without needing the Tauri command wrapper.
    fn compress_tool_output_or_self(s: &str, opts: &TabularOpts) -> String {
        compress_tool_json(s, opts).unwrap_or_else(|| s.to_string())
    }

    #[test]
    fn expand_passthrough_on_plain_text() {
        // expand_tool_output must be a no-op on non-table content.
        assert_eq!(expand_tool_output("just some text".to_string()), "just some text");
    }

    #[test]
    fn command_wrappers_roundtrip() {
        let orig = inventory(40);
        let comp = compress_tool_output(orig.clone());
        assert!(comp.starts_with(MARKER_START), "should have compressed");
        let back = expand_tool_output(comp);
        assert!(semantically_equal(&orig, &back));
    }

    #[test]
    fn command_wrappers_handle_leading_label() {
        // Tool results often carry a "[LABEL]\n" prefix before the JSON body.
        let json = inventory(40);
        let labeled = format!("[INVENTORY RESULT]\n{json}");
        let comp = compress_tool_output(labeled.clone());
        assert!(comp.starts_with("[INVENTORY RESULT]\n"), "label must be preserved");
        assert!(comp.contains(MARKER_START), "body must be compressed");
        assert!(comp.len() < labeled.len(), "must be smaller");
        let back = expand_tool_output(comp);
        assert!(back.starts_with("[INVENTORY RESULT]\n"), "label preserved on expand");
        let back_json = back.strip_prefix("[INVENTORY RESULT]\n").unwrap();
        assert!(semantically_equal(&json, back_json), "labeled body must round-trip");
    }
}
