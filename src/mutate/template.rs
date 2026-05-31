//! Capture-group substitution for `gw rewrite`.
//!
//! `gw` owns capture substitution (per `_/decisions.md` D3) rather than
//! delegating to rg / ast-grep — the engines either don't provide captures in
//! their JSON event stream (rg) or wouldn't get the unified `$1` / `${name}`
//! / `$$` / `$0` surface for free. Keeping it here also means the parser is a
//! single, tested chokepoint independent of the locator.

use crate::errors::GwError;

/// Expand a replacement template against a slice of captures.
///
/// Captures convention (matching `locate::Match.captures`):
/// - `("", full_match)` is `$0`.
/// - `("1", v)`, `("2", v)`, ... — numbered groups.
/// - `(name, v)` — named groups.
///
/// Grammar:
/// - `$0`, `$1`, `$2`, ... — numbered references.
/// - `${name}` — named (or numbered, via e.g. `${1}`) references.
/// - `$$` — literal `$`.
/// - Anything else after `$` is an error.
pub fn expand(replacement: &str, captures: &[(String, String)]) -> Result<String, GwError> {
    let mut out = String::with_capacity(replacement.len());
    let bytes = replacement.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];
        if b != b'$' {
            out.push(b as char);
            i += 1;
            continue;
        }

        // We saw `$`. Peek the next byte.
        let next = match bytes.get(i + 1) {
            Some(n) => *n,
            None => {
                return Err(GwError::Engine(format!("template: dangling $ at byte {i}")));
            }
        };

        match next {
            b'$' => {
                out.push('$');
                i += 2;
            }
            b'{' => {
                // ${name} or ${number}
                let start = i + 2;
                let mut j = start;
                while j < bytes.len() && bytes[j] != b'}' {
                    j += 1;
                }
                if j >= bytes.len() {
                    return Err(GwError::Engine(format!(
                        "template: unclosed '${{' at byte {i}"
                    )));
                }
                let name = &replacement[start..j];
                let value = lookup(captures, name).ok_or_else(|| {
                    GwError::Engine(format!(
                        "template: unknown reference '${{{name}}}' at byte {i}"
                    ))
                })?;
                out.push_str(value);
                i = j + 1;
            }
            c if c.is_ascii_digit() => {
                // $0, $1, ... — greedy digits.
                let start = i + 1;
                let mut j = start;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                let name = &replacement[start..j];
                let value = lookup(captures, name).ok_or_else(|| {
                    GwError::Engine(format!("template: unknown reference '${name}' at byte {i}"))
                })?;
                out.push_str(value);
                i = j;
            }
            other => {
                return Err(GwError::Engine(format!(
                    "template: unknown reference '${}' at byte {i}",
                    other as char
                )));
            }
        }
    }

    Ok(out)
}

fn lookup<'a>(captures: &'a [(String, String)], name: &str) -> Option<&'a str> {
    // `$0` is the full match, stored under the empty-string key.
    let want = if name == "0" { "" } else { name };
    captures
        .iter()
        .find(|(k, _)| k == want)
        .map(|(_, v)| v.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn empty_replacement_is_empty() {
        let c = caps(&[("", "x")]);
        assert_eq!(expand("", &c).unwrap(), "");
    }

    #[test]
    fn no_dollar_returned_verbatim() {
        let c = caps(&[("", "x")]);
        assert_eq!(expand("hello world", &c).unwrap(), "hello world");
    }

    #[test]
    fn numbered_substitution() {
        let c = caps(&[("", "foo(x)"), ("1", "x")]);
        assert_eq!(expand("bar($1)", &c).unwrap(), "bar(x)");
    }

    #[test]
    fn named_substitution() {
        let c = caps(&[("", "user=alice"), ("name", "alice")]);
        assert_eq!(expand("hello ${name}!", &c).unwrap(), "hello alice!");
    }

    #[test]
    fn dollar_zero_is_full_match() {
        let c = caps(&[("", "foo(x)"), ("1", "x")]);
        assert_eq!(expand("[$0]", &c).unwrap(), "[foo(x)]");
    }

    #[test]
    fn double_dollar_is_literal() {
        let c = caps(&[("", "x")]);
        assert_eq!(expand("price: $$5", &c).unwrap(), "price: $5");
    }

    #[test]
    fn unknown_numbered_reference_errors() {
        let c = caps(&[("", "x"), ("1", "y")]);
        let err = expand("bar$5", &c).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("$5"), "msg was: {msg}");
    }

    #[test]
    fn unclosed_brace_errors() {
        let c = caps(&[("", "x")]);
        let err = expand("${unclosed", &c).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unclosed"), "msg was: {msg}");
    }

    #[test]
    fn dangling_dollar_errors() {
        let c = caps(&[("", "x")]);
        let err = expand("trailing$", &c).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("dangling"), "msg was: {msg}");
    }

    #[test]
    fn unknown_named_reference_errors() {
        let c = caps(&[("", "x")]);
        let err = expand("hi ${who}", &c).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("${who}"), "msg was: {msg}");
    }

    #[test]
    fn multiple_substitutions() {
        let c = caps(&[("", "ab"), ("1", "a"), ("2", "b")]);
        assert_eq!(expand("$2$1$2", &c).unwrap(), "bab");
    }

    #[test]
    fn braced_numeric_reference() {
        let c = caps(&[("", "ab"), ("1", "a")]);
        assert_eq!(expand("${1}x", &c).unwrap(), "ax");
    }
}
