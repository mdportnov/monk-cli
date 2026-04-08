mod detect;

pub use detect::detect;

use std::{
    borrow::Cow,
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

pub const SUPPORTED: &[&str] = &["en", "ru"];

const EN_YAML: &str = include_str!("../../locales/en.yml");
const RU_YAML: &str = include_str!("../../locales/ru.yml");

fn bundles() -> &'static HashMap<&'static str, HashMap<String, String>> {
    static B: OnceLock<HashMap<&'static str, HashMap<String, String>>> = OnceLock::new();
    B.get_or_init(|| {
        let mut m = HashMap::new();
        m.insert("en", parse(EN_YAML));
        m.insert("ru", parse(RU_YAML));
        m
    })
}

fn current_locale() -> &'static Mutex<&'static str> {
    static L: OnceLock<Mutex<&'static str>> = OnceLock::new();
    L.get_or_init(|| Mutex::new("en"))
}

pub fn set(locale: &str) {
    *current_locale().lock().unwrap() = normalize(locale);
}

pub fn current() -> String {
    current_locale().lock().unwrap().to_string()
}

pub fn normalize(raw: &str) -> &'static str {
    if raw.to_lowercase().starts_with("ru") {
        "ru"
    } else {
        "en"
    }
}

pub fn init(config_locale: Option<&str>, cli_override: Option<&str>) {
    let locale = detect(cli_override, config_locale);
    set(locale);
}

pub fn lookup(key: &str) -> Cow<'static, str> {
    let locale: &str = &current_locale().lock().unwrap();
    let b = bundles();
    if let Some(v) = b.get(locale).and_then(|m| m.get(key)) {
        return Cow::Owned(v.clone());
    }
    if let Some(v) = b.get("en").and_then(|m| m.get(key)) {
        return Cow::Owned(v.clone());
    }
    Cow::Owned(key.to_string())
}

pub fn render(key: &str, args: &[(&str, String)]) -> String {
    let mut s = lookup(key).into_owned();
    for (k, v) in args {
        let needle = format!("{{{k}}}");
        s = s.replace(&needle, v);
        let pct = format!("%{{{k}}}");
        s = s.replace(&pct, v);
    }
    s
}

fn parse(yaml: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut section: Option<String> = None;
    for raw in yaml.lines() {
        if raw.trim().is_empty() || raw.trim_start().starts_with('#') {
            continue;
        }
        if !raw.starts_with(' ') && !raw.starts_with('\t') {
            if let Some((k, v)) = split_kv(raw.trim()) {
                if v.is_empty() {
                    section = Some(k.to_string());
                } else if !k.starts_with('_') {
                    out.insert(k.to_string(), unquote(v));
                }
            }
            continue;
        }
        let trimmed = raw.trim_start();
        if let Some((k, v)) = split_kv(trimmed) {
            let full = match &section {
                Some(s) => format!("{s}.{k}"),
                None => k.to_string(),
            };
            out.insert(full, unquote(v));
        }
    }
    out
}

fn split_kv(line: &str) -> Option<(&str, &str)> {
    let idx = line.find(':')?;
    let k = line[..idx].trim();
    let v = line[idx + 1..].trim();
    Some((k, v))
}

fn unquote(v: &str) -> String {
    let s = if (v.starts_with('"') && v.ends_with('"') && v.len() >= 2)
        || (v.starts_with('\'') && v.ends_with('\'') && v.len() >= 2)
    {
        &v[1..v.len() - 1]
    } else {
        v
    };
    s.replace("\\n", "\n").replace("\\t", "\t").replace("\\\"", "\"")
}

#[macro_export]
macro_rules! monk_t {
    ($key:expr $(,)?) => {
        $crate::i18n::lookup($key)
    };
    ($key:expr, $($name:ident = $value:expr),+ $(,)?) => {
        ::std::borrow::Cow::<'static, str>::Owned($crate::i18n::render(
            $key,
            &[$((stringify!($name), ($value).to_string())),+],
        ))
    };
}

pub use crate::monk_t as t;
