//! Layered `.env` loading, modeled on DSH's `loadLayeredEnv`
//! (`packages/boot/app-boot`): optional `.env` files fill the gaps left by the
//! ambient process environment, so a key configured as `{ env = "X" }` does
//! not force the caller to `export` it beforehand.
//!
//! Precedence (highest first):
//!   1. inherited process environment (never overwritten)
//!   2. project layer: `<config dir>/.env` — next to `llmn.toml`
//!   3. user layer: `<llmn data dir>/.env` — the llmn home
//!
//! A missing `.env` is fine (rely on the ambient environment); unreadable
//! files and malformed lines are reported through a warn sink and skipped —
//! a convenience file must never prevent startup.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Parse one `.env` line.
///
/// Returns:
/// - `None` — ignorable line (blank or a `#` comment);
/// - `Some(Ok((key, value)))` — a valid `KEY=VALUE` entry;
/// - `Some(Err(reason))` — malformed; the caller reports the reason.
///
/// Accepted forms (Node `parseEnv` compatible subset): optional `export `
/// prefix, `#` comments, single/double-quoted values (double quotes support
/// `\n` `\r` `\t` `\"` `\\`), and inline comments after unquoted values.
fn parse_env_line(line: &str) -> Option<Result<(String, String), String>> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let body = trimmed.strip_prefix("export ").unwrap_or(trimmed);
    let Some(eq) = body.find('=') else {
        return Some(Err("missing '='".into()));
    };
    let key = body[..eq].trim();
    if key.is_empty() {
        return Some(Err("empty key".into()));
    }
    let mut chars = key.chars();
    if chars.next().is_some_and(|c| c.is_ascii_digit())
        || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Some(Err(format!("invalid key '{key}'")));
    }
    let Some(value) = parse_env_value(body[eq + 1..].trim()) else {
        return Some(Err("unterminated quote".into()));
    };
    Some(Ok((key.to_string(), value)))
}

fn parse_env_value(raw: &str) -> Option<String> {
    if raw.is_empty() {
        return Some(String::new());
    }
    if let Some(rest) = raw.strip_prefix('\'') {
        // Single quotes: literal content up to the closing quote; anything
        // after it is ignored.
        let end = rest.find('\'')?;
        return Some(rest[..end].to_string());
    }
    if let Some(rest) = raw.strip_prefix('"') {
        let mut out = String::new();
        let mut chars = rest.chars();
        loop {
            match chars.next()? {
                '"' => return Some(out),
                '\\' => match chars.next()? {
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    '\\' => out.push('\\'),
                    '"' => out.push('"'),
                    c => {
                        out.push('\\');
                        out.push(c);
                    }
                },
                c => out.push(c),
            }
        }
    }
    // Unquoted: cut an inline ` #` comment, then trim surrounding whitespace.
    let value = raw.split(" #").next().unwrap_or(raw).trim();
    Some(value.to_string())
}

/// Parse `.env` content into a map, reporting malformed lines via `warn`
/// (blank lines and comments are skipped silently).
pub fn parse_env(content: &str, warn: &dyn Fn(&str)) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for (idx, line) in content.lines().enumerate() {
        match parse_env_line(line) {
            None => {}
            Some(Ok((key, value))) => {
                out.insert(key, value);
            }
            Some(Err(reason)) => {
                warn(&format!("line {}: {reason}: '{}'", idx + 1, line.trim()));
            }
        }
    }
    out
}

/// Read `<dir>/.env` into a map. `None` when the file is absent (ENOENT);
/// other read failures and malformed lines are reported via `warn`.
fn read_env_layer(dir: &Path, warn: &dyn Fn(&str)) -> Option<HashMap<String, String>> {
    let path = dir.join(".env");
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            warn(&format!("failed to load {}: {e}", path.display()));
            return None;
        }
    };
    Some(parse_env(&content, &|line| {
        warn(&format!("{}: {line}", path.display()));
    }))
}

/// Apply layers to an environment: only names **not already set** are written
/// (`has` decides), in layer order, so earlier layers win over later ones.
/// Injected `has`/`set` keep this pure and testable without touching the real
/// process environment.
pub fn apply_layers(
    layers: impl IntoIterator<Item = HashMap<String, String>>,
    has: &dyn Fn(&str) -> bool,
    set: &dyn Fn(&str, &str),
) {
    for layer in layers {
        for (name, value) in layer {
            if !has(&name) {
                set(&name, &value);
            }
        }
    }
}

/// Load the project and user `.env` layers for a config document and apply
/// them to the process environment. Ambient variables always win; the project
/// layer (next to the config file) wins over the user layer (llmn home).
pub fn load_env_for_config(config_path: &Path, warn: &dyn Fn(&str)) {
    let project_layer = config_path
        .parent()
        .and_then(|dir| read_env_layer(dir, warn));
    let user_layer = read_env_layer(&storage::default_data_dir(), warn);
    apply_layers(
        [project_layer, user_layer].into_iter().flatten(),
        &|name| std::env::var_os(name).is_some(),
        &|name, value| {
            // SAFETY: this runs once during startup config loading, before any
            // request or concurrent `getenv`; mutating the process environment
            // later (e.g. from a hot-reload) writes the same values, and only
            // names that are still unset. Same constraint as the dotenv-crate
            // startup pattern; `set_var` is `unsafe` in edition 2024 because
            // POSIX `setenv` is not atomic with concurrent `getenv`.
            unsafe { std::env::set_var(name, value) }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_and_export_prefix() {
        let mut map = HashMap::new();
        map.insert("A".into(), "1".into());
        map.insert("GEMINI_API_KEY".into(), "sk-123".into());
        assert_eq!(
            parse_env("A=1\nexport GEMINI_API_KEY=sk-123\n", &|_| panic!(
                "no warn"
            )),
            map
        );
    }

    #[test]
    fn parse_ignores_blank_and_comments() {
        let map = parse_env("# comment\n\n  \nKEY=value # trailing\n", &|_| {
            panic!("no warn")
        });
        assert_eq!(map.get("KEY").map(String::as_str), Some("value"));
    }

    #[test]
    fn parse_quoted_values() {
        let map = parse_env(
            "SINGLE='a b'\nDOUBLE=\"x\\ny\"\nESCAPED=\"q\\\"z\"\nEMPTY=\n",
            &|_| panic!("no warn"),
        );
        assert_eq!(map.get("SINGLE").map(String::as_str), Some("a b"));
        assert_eq!(map.get("DOUBLE").map(String::as_str), Some("x\ny"));
        assert_eq!(map.get("ESCAPED").map(String::as_str), Some("q\"z"));
        assert_eq!(map.get("EMPTY").map(String::as_str), Some(""));
    }

    #[test]
    fn parse_reports_malformed_and_skips() {
        let warns = std::sync::Mutex::new(Vec::new());
        let map = parse_env("GOOD=1\nNOEQUALS\n1BAD=2\nUNCLOSED='abc\n", &|w| {
            warns.lock().unwrap().push(w.to_string())
        });
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("GOOD").map(String::as_str), Some("1"));
        assert_eq!(warns.lock().unwrap().len(), 3);
    }

    #[test]
    fn read_layer_missing_file_is_none() {
        let tmp = std::env::temp_dir().join(format!("llmn-env-none-{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        assert!(read_env_layer(&tmp, &|_| panic!("no warn")).is_none());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn read_layer_parses_file_and_reports_bad_lines() {
        let tmp = std::env::temp_dir().join(format!("llmn-env-file-{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join(".env"), "K1=v1\nK2 = v2 # c\nbadline\n").unwrap();
        let warns = std::sync::Mutex::new(Vec::new());
        let map = read_env_layer(&tmp, &|w| warns.lock().unwrap().push(w.to_string())).unwrap();
        assert_eq!(map.get("K1").map(String::as_str), Some("v1"));
        assert_eq!(map.get("K2").map(String::as_str), Some("v2"));
        assert_eq!(warns.lock().unwrap().len(), 1);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn apply_layers_never_overrides_ambient_and_project_wins() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let env: Rc<RefCell<HashMap<String, String>>> = Rc::new(RefCell::new(
            [("AMBIENT".to_string(), "ambient".to_string())].into(),
        ));
        let project: HashMap<String, String> = [
            ("AMBIENT".to_string(), "from-project".to_string()),
            ("P".to_string(), "p".to_string()),
        ]
        .into();
        let user: HashMap<String, String> = [
            ("P".to_string(), "from-user".to_string()),
            ("U".to_string(), "u".to_string()),
        ]
        .into();

        let env_has = env.clone();
        let env_set = env.clone();
        apply_layers(
            [project, user],
            &|name| env_has.borrow().contains_key(name),
            &|name, value| {
                env_set
                    .borrow_mut()
                    .insert(name.to_string(), value.to_string());
            },
        );

        // ambient wins; project wins over user; only unset names are filled
        assert_eq!(
            env.borrow().get("AMBIENT").map(String::as_str),
            Some("ambient")
        );
        assert_eq!(env.borrow().get("P").map(String::as_str), Some("p"));
        assert_eq!(env.borrow().get("U").map(String::as_str), Some("u"));
    }
}
