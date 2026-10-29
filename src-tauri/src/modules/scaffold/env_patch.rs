//! Pure `.env` editing for the New Project wizard.
//!
//! Only the keys we are asked to set are touched: an existing `KEY=...` line is
//! rewritten in place (so it keeps its position among its neighbours), keys the
//! file doesn't have are appended in one block at the end. Comments, blank
//! lines, `export` prefixes and every other key pass through byte-for-byte —
//! the same "only re-render what changed" rule the Hosts file module follows,
//! for the same reason: a hand-edited `.env` full of a developer's own notes
//! should never come back reflowed.

/// The key of an active `KEY=value` line, or `None` for comments, blanks and
/// anything that doesn't look like an assignment. `export KEY=...` counts —
/// Laravel's dotenv accepts it and it would be a surprise to append a duplicate.
pub fn key_of(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        return None;
    }
    let trimmed = trimmed.strip_prefix("export ").map(str::trim_start).unwrap_or(trimmed);
    let (key, _) = trimmed.split_once('=')?;
    let key = key.trim();
    let valid = !key.is_empty()
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !key.chars().next().is_some_and(|c| c.is_ascii_digit());
    valid.then_some(key)
}

/// Quotes a value only when dotenv would otherwise misread it (spaces, `#`,
/// quotes, `$`), so ordinary values like `mysql` or `3306` stay bare — as
/// Laravel's own `.env.example` writes them.
pub fn render_value(value: &str) -> String {
    let needs_quotes = value.is_empty()
        || value
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '#' | '"' | '\'' | '$' | '\\'));
    if needs_quotes {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        value.to_string()
    }
}

/// The current value of `key` (unquoted, trimmed), or `None` if absent.
pub fn value_of(contents: &str, key: &str) -> Option<String> {
    contents.lines().find_map(|line| {
        if key_of(line)? != key {
            return None;
        }
        let raw = line.split_once('=')?.1.trim();
        let unquoted = raw
            .strip_prefix('"')
            .and_then(|r| r.strip_suffix('"'))
            .or_else(|| raw.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')))
            .unwrap_or(raw);
        Some(unquoted.to_string())
    })
}

/// Sets each `(key, value)` in `contents`, editing in place where the key
/// exists and appending the rest. If the same key appears twice, only the
/// first occurrence is rewritten: dotenv reads the first one, and silently
/// changing both would hide a mistake the developer should see.
pub fn patch_env(contents: &str, updates: &[(String, String)]) -> String {
    let mut remaining: Vec<&(String, String)> = updates.iter().collect();
    let mut out: Vec<String> = Vec::with_capacity(contents.lines().count() + updates.len());

    for line in contents.lines() {
        let replaced = key_of(line).and_then(|k| {
            let idx = remaining.iter().position(|(key, _)| key == k)?;
            let (key, value) = remaining.remove(idx);
            Some(format!("{key}={}", render_value(value)))
        });
        out.push(replaced.unwrap_or_else(|| line.to_string()));
    }

    if !remaining.is_empty() {
        if out.last().is_some_and(|l| !l.trim().is_empty()) {
            out.push(String::new());
        }
        for (key, value) in remaining {
            out.push(format!("{key}={}", render_value(value)));
        }
    }

    // A file saved with Windows line endings keeps them: `lines()` strips the
    // `\r`, and putting back only `\n` would rewrite every line of the file.
    let eol = if contents.contains("\r\n") { "\r\n" } else { "\n" };
    let mut result = out.join(eol);
    result.push_str(eol);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kv(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn key_of_ignores_comments_blanks_and_junk() {
        assert_eq!(key_of("APP_URL=http://x"), Some("APP_URL"));
        assert_eq!(key_of("  export DB_HOST=127.0.0.1"), Some("DB_HOST"));
        assert_eq!(key_of("# APP_URL=x"), None);
        assert_eq!(key_of(""), None);
        assert_eq!(key_of("no equals here"), None);
        assert_eq!(key_of("1BAD=x"), None);
    }

    #[test]
    fn replaces_in_place_and_keeps_everything_else() {
        let input = "# app\nAPP_NAME=Laravel\nAPP_URL=http://localhost\n\n# db\nDB_CONNECTION=sqlite\n# DB_HOST=127.0.0.1\nDB_HOST=old\n";
        let out = patch_env(
            input,
            &kv(&[("APP_URL", "https://shop.test"), ("DB_CONNECTION", "mysql"), ("DB_HOST", "127.0.0.1")]),
        );
        assert_eq!(
            out,
            "# app\nAPP_NAME=Laravel\nAPP_URL=https://shop.test\n\n# db\nDB_CONNECTION=mysql\n# DB_HOST=127.0.0.1\nDB_HOST=127.0.0.1\n"
        );
    }

    #[test]
    fn appends_missing_keys_after_a_blank_line() {
        let out = patch_env("APP_NAME=Laravel\n", &kv(&[("DB_DATABASE", "shop"), ("DB_PORT", "3307")]));
        assert_eq!(out, "APP_NAME=Laravel\n\nDB_DATABASE=shop\nDB_PORT=3307\n");
    }

    #[test]
    fn appends_to_an_empty_file_without_a_leading_blank() {
        assert_eq!(patch_env("", &kv(&[("A", "1")])), "A=1\n");
    }

    #[test]
    fn only_the_first_duplicate_is_rewritten() {
        let out = patch_env("A=1\nA=2\n", &kv(&[("A", "9")]));
        assert_eq!(out, "A=9\nA=2\n");
    }

    #[test]
    fn a_commented_out_key_does_not_count_as_present() {
        let out = patch_env("# DB_PORT=1\n", &kv(&[("DB_PORT", "3306")]));
        assert_eq!(out, "# DB_PORT=1\n\nDB_PORT=3306\n");
    }

    #[test]
    fn values_needing_quotes_are_quoted_and_escaped() {
        assert_eq!(render_value("mysql"), "mysql");
        assert_eq!(render_value(""), "\"\"");
        assert_eq!(render_value("p@ss word"), "\"p@ss word\"");
        assert_eq!(render_value("a#b"), "\"a#b\"");
        assert_eq!(render_value("say \"hi\""), "\"say \\\"hi\\\"\"");
    }

    #[test]
    fn value_of_unquotes_and_skips_comments() {
        let env = "# APP_KEY=nope\nAPP_KEY=\nDB_PASSWORD=\"p w\"\nDB_HOST='h'\n";
        assert_eq!(value_of(env, "APP_KEY").as_deref(), Some(""));
        assert_eq!(value_of(env, "DB_PASSWORD").as_deref(), Some("p w"));
        assert_eq!(value_of(env, "DB_HOST").as_deref(), Some("h"));
        assert_eq!(value_of(env, "MISSING"), None);
    }

    #[test]
    fn windows_line_endings_are_kept() {
        let input = "APP_NAME=Laravel\r\nAPP_URL=http://localhost\r\n";
        let out = patch_env(input, &kv(&[("APP_URL", "https://shop.test"), ("DB_HOST", "127.0.0.1")]));
        assert_eq!(out, "APP_NAME=Laravel\r\nAPP_URL=https://shop.test\r\n\r\nDB_HOST=127.0.0.1\r\n");
    }
}
