//! Shared line-scanner for the source lints (`tests/panic_budget.rs`, `tests/determinism_lint.rs`) —
//! ONE comment/literal stripper both use, so they cannot drift apart.
//!
//! Both lints used to truncate at the first `//` on the line (`line.find("//")` / `split("//")`),
//! which misreads a `//` INSIDE a string literal as a comment and silently skips the rest of the
//! line — the same misread-a-literal class as the cfg-tokenizer incident documented on
//! `determinism_lint::cfg_enables_test` (a feature *name* matched as a cfg predicate and blinded the
//! lint to half of `light.rs`). No line in `src/` trips it today; this exists so none ever can.
//!
//! Known limitation, unchanged from before: the scan is line-based, so the *continuation lines* of a
//! multi-line raw string still read as code. The RON fixtures that use those live in test modules,
//! which both lints strip before scanning.

/// The scannable code of one source line: the line up to the first `//` that sits **outside** any
/// string, raw string, or char/byte literal — with every literal's *contents* blanked to spaces
/// (delimiters kept), so a pattern like `.unwrap()` or `.sort()` quoted inside a message never
/// counts as code either. Byte offsets of everything kept are preserved.
pub fn code_portion(line: &str) -> String {
    let b = line.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'/' if i + 1 < b.len() && b[i + 1] == b'/' => break, // comment: truncate
            b'"' => {
                out.push(b'"');
                i += 1;
                while i < b.len() {
                    match b[i] {
                        b'\\' if i + 1 < b.len() => {
                            out.extend_from_slice(b"  ");
                            i += 2;
                        }
                        b'"' => {
                            out.push(b'"');
                            i += 1;
                            break;
                        }
                        _ => {
                            out.push(b' ');
                            i += 1;
                        }
                    }
                }
            }
            b'r' if raw_string_hashes(b, i).is_some() => {
                let hashes = raw_string_hashes(b, i).unwrap_or(0);
                // Emit the opener verbatim, blank the contents, keep the closer.
                let content = i + 1 + hashes + 1; // r + #s + "
                out.extend_from_slice(&b[i..content]);
                i = content;
                let closer: Vec<u8> =
                    std::iter::once(b'"').chain(std::iter::repeat_n(b'#', hashes)).collect();
                loop {
                    if i >= b.len() {
                        break; // multi-line raw string: rest of line was content (blanked)
                    }
                    if b[i..].starts_with(&closer) {
                        out.extend_from_slice(&closer);
                        i += closer.len();
                        break;
                    }
                    out.push(b' ');
                    i += 1;
                }
            }
            b'\'' => {
                // Char/byte literal vs lifetime: a literal closes within a short window ('x', '\n',
                // '\u{10FFFF}' at the longest); a lifetime (`'a`) never closes. Bounded lookahead
                // decides; a lifetime is left in place untouched.
                let close = if i + 1 < b.len() && b[i + 1] == b'\\' {
                    (i + 2..(i + 13).min(b.len())).find(|&k| b[k] == b'\'')
                } else if i + 2 < b.len() && b[i + 2] == b'\'' && b[i + 1] != b'\'' {
                    Some(i + 2)
                } else {
                    None
                };
                match close {
                    Some(k) => {
                        out.push(b'\'');
                        out.extend(std::iter::repeat_n(b' ', k - i - 1));
                        out.push(b'\'');
                        i = k + 1;
                    }
                    None => {
                        out.push(b'\''); // a lifetime tick — plain code
                        i += 1;
                    }
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    // Everything pushed is ASCII or copied whole bytes at literal boundaries, but non-ASCII code
    // bytes outside literals are copied verbatim too — the result is valid UTF-8 because literal
    // interiors (the only place bytes were REPLACED) were replaced wholesale with ASCII spaces.
    String::from_utf8(out).unwrap_or_default()
}

/// If `b[i]` starts a raw-string opener (`r"`, `r#"`, `br"`, …), the number of `#`s; else `None`.
/// `b[i]` must be the `r`. Rejects an `r` that is the tail of an identifier (`for`, `var`, …).
fn raw_string_hashes(b: &[u8], i: usize) -> Option<usize> {
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let prev_ok = i == 0
        || !ident(b[i - 1])
        || (b[i - 1] == b'b' && (i < 2 || !ident(b[i - 2]))); // the `br"…"` prefix
    if !prev_ok {
        return None;
    }
    let mut j = i + 1;
    while j < b.len() && b[j] == b'#' {
        j += 1;
    }
    (j < b.len() && b[j] == b'"').then_some(j - i - 1)
}

#[cfg(test)]
mod tests {
    use super::code_portion;

    #[test]
    fn comments_strip_and_literals_blank() {
        // Plain code passes through; trailing comments truncate.
        assert_eq!(code_portion("let x = 1;"), "let x = 1;");
        assert_eq!(code_portion("let x = 1; // .unwrap() in prose"), "let x = 1; ");
        // A `//` inside a string is NOT a comment — code after it survives.
        assert!(code_portion(r#"let u = "a//b"; v.sort();"#).contains(".sort()"));
        // String contents are blanked, so quoted panic spellings never count as code.
        assert!(!code_portion(r#"let m = "call .unwrap() here";"#).contains(".unwrap()"));
        // A char literal holding a double-quote must not open a string (the `bake.rs` shape).
        assert_eq!(code_portion("if c == b'\"' { } // tail"), "if c == b' ' { } ");
        // Lifetimes are not char literals; the comment after one still strips.
        assert_eq!(code_portion("fn f<'a>(x: &'a str) {} // c"), "fn f<'a>(x: &'a str) {} ");
        // Raw strings: an inner `"` does not close them; contents blank; the comment strips.
        let raw = code_portion(r###"let s = r#"a "quoted" //x"#; y.sort(); // c"###);
        assert!(raw.contains(".sort()"), "code after the raw string must survive: {raw}");
        assert!(!raw.contains("//x"), "raw-string contents must be blanked: {raw}");
        // Escapes inside strings cannot fake a closing quote.
        assert!(!code_portion(r#"let e = "a\" .unwrap() b";"#).contains(".unwrap()"));
    }
}
