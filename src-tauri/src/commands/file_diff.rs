// ── file_diff.rs — Compact unified diff for tool cards (v1.4.4) ────────
//
// Powers the diff view inside writefile/editfile tool cards. The frontend
// reads the file BEFORE the write, calls write_file_content, then asks
// this command to produce a human-readable diff between the old and new
// content. The card shows that instead of just the new content blob —
// users instantly see what changed.
//
// Design:
//   • Line-oriented (similar::TextDiff::from_lines).
//   • Unified-diff format with 2 lines of context (compact for tool cards).
//   • Output as a single string; the frontend wraps it in <pre> and color-
//     codes via CSS class hooks "+" / "-" / "@" on first chars.
//   • Caps the diff at MAX_LINES to keep the card readable for huge files;
//     we append a "[N more lines truncated]" footer when clipped.
//   • Empty `old` → treated as a fresh-file scenario, all lines marked +.
//   • Identical inputs → returns "(no changes)" so the card doesn't render
//     an empty diff and confuse the user.

use similar::{ChangeTag, TextDiff};

const MAX_LINES: usize = 200;
const CONTEXT_LINES: usize = 2;

#[derive(Debug, Clone, serde::Serialize)]
pub struct DiffResult {
    pub text:          String,
    pub additions:     usize,
    pub deletions:     usize,
    pub truncated:     bool,
}

#[tauri::command]
pub fn compute_text_diff(old: String, new: String) -> Result<DiffResult, String> {
    if old == new {
        return Ok(DiffResult { text: "(no changes)".into(), additions: 0, deletions: 0, truncated: false });
    }
    // Fresh-file fast path — every new line is an addition.
    if old.is_empty() {
        let mut text = String::new();
        let mut additions = 0usize;
        for (i, line) in new.lines().enumerate() {
            if i >= MAX_LINES {
                let remaining = new.lines().count() - MAX_LINES;
                text.push_str(&format!("[{} more lines truncated]\n", remaining));
                return Ok(DiffResult { text, additions, deletions: 0, truncated: true });
            }
            text.push_str(&format!("+ {}\n", line));
            additions += 1;
        }
        return Ok(DiffResult { text, additions, deletions: 0, truncated: false });
    }

    let diff = TextDiff::from_lines(&old, &new);
    let mut text = String::new();
    let mut additions = 0usize;
    let mut deletions = 0usize;
    let mut lines_used = 0usize;
    let mut truncated = false;

    for group in diff.grouped_ops(CONTEXT_LINES).iter() {
        if truncated { break; }
        // Hunk header: @@ -old_start,old_len +new_start,new_len @@
        let (old_start, old_len, new_start, new_len) = hunk_extents(group);
        text.push_str(&format!("@@ -{},{} +{},{} @@\n", old_start + 1, old_len, new_start + 1, new_len));
        lines_used += 1;
        for op in group {
            // iter_changes() yields line-granular Change items. We render
            // each with a leading `+`/`-`/` ` sign and concatenate the
            // underlying value (a Cow<str>) verbatim.
            for change in diff.iter_changes(op) {
                if lines_used >= MAX_LINES {
                    truncated = true;
                    text.push_str("[diff truncated — open the file to see full changes]\n");
                    break;
                }
                let sign = match change.tag() {
                    ChangeTag::Delete => { deletions += 1; "-" }
                    ChangeTag::Insert => { additions += 1; "+" }
                    ChangeTag::Equal  => " ",
                };
                text.push_str(sign);
                text.push(' ');
                text.push_str(&change.value().to_string());
                if !text.ends_with('\n') { text.push('\n'); }
                lines_used += 1;
            }
            if truncated { break; }
        }
    }
    Ok(DiffResult { text, additions, deletions, truncated })
}

/// Compute (old_start, old_len, new_start, new_len) for a single group of
/// diff ops. Mirrors the `@@ -a,b +c,d @@` header convention.
/// `similar::DiffOp::as_tag_tuple()` returns (tag, Range<old>, Range<new>);
/// the Range fields give us start/end directly.
fn hunk_extents(group: &[similar::DiffOp]) -> (usize, usize, usize, usize) {
    let first = group.first().expect("non-empty group");
    let last  = group.last().expect("non-empty group");
    let (_, first_old_range, first_new_range) = first.as_tag_tuple();
    let (_, last_old_range,  last_new_range)  = last.as_tag_tuple();
    let old_start = first_old_range.start;
    let new_start = first_new_range.start;
    let old_end   = last_old_range.end;
    let new_end   = last_new_range.end;
    (old_start, old_end - old_start, new_start, new_end - new_start)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_inputs_return_no_changes() {
        let d = compute_text_diff("hello".into(), "hello".into()).unwrap();
        assert_eq!(d.text, "(no changes)");
        assert_eq!(d.additions + d.deletions, 0);
    }

    #[test]
    fn empty_old_returns_all_additions() {
        let d = compute_text_diff("".into(), "alpha\nbeta\n".into()).unwrap();
        assert!(d.text.contains("+ alpha"));
        assert!(d.text.contains("+ beta"));
        assert_eq!(d.additions, 2);
        assert_eq!(d.deletions, 0);
        assert!(!d.truncated);
    }

    #[test]
    fn one_line_changed_produces_minimal_diff() {
        let old = "a\nb\nc\nd\ne\n".to_string();
        let new = "a\nb\nCHANGED\nd\ne\n".to_string();
        let d = compute_text_diff(old, new).unwrap();
        // 1 deletion, 1 insertion.
        assert!(d.additions >= 1 && d.deletions >= 1, "got {} adds {} dels — text:\n{}", d.additions, d.deletions, d.text);
        assert!(d.text.contains("CHANGED"));
        assert!(!d.truncated);
    }

    #[test]
    fn huge_diff_gets_truncated_with_footer() {
        let old = (0..MAX_LINES + 50)
            .map(|i| format!("line {} old\n", i))
            .collect::<String>();
        let new = (0..MAX_LINES + 50)
            .map(|i| format!("line {} new\n", i))
            .collect::<String>();
        let d = compute_text_diff(old, new).unwrap();
        assert!(d.truncated, "should report truncation");
        assert!(d.text.contains("truncated"));
    }

    #[test]
    fn fresh_file_with_long_content_truncates() {
        let new = (0..MAX_LINES + 30)
            .map(|i| format!("ln{}\n", i))
            .collect::<String>();
        let d = compute_text_diff("".into(), new).unwrap();
        assert!(d.truncated);
        assert!(d.text.contains("more lines truncated"));
    }
}
