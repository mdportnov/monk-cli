use std::{collections::BTreeSet, fs, path::Path};

fn walk(dir: &Path, out: &mut Vec<String>) {
    for entry in fs::read_dir(dir).unwrap() {
        let e = entry.unwrap();
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(fs::read_to_string(&p).unwrap());
        }
    }
}

fn extract_keys(src: &str) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    let patterns = ["i18n::t!(\"", "monk_t!(\"", "i18n::lookup(\"", "i18n::render(\""];
    for pat in patterns {
        let mut rest = src;
        while let Some(i) = rest.find(pat) {
            let start = i + pat.len();
            let tail = &rest[start..];
            if let Some(end) = tail.find('"') {
                let key = &tail[..end];
                if key.contains('.') && !key.contains('{') {
                    keys.insert(key.to_string());
                }
                rest = &tail[end + 1..];
            } else {
                break;
            }
        }
    }
    keys
}

fn dynamic_prefixes() -> &'static [&'static str] {
    &["tui.menu."]
}

#[test]
fn every_used_i18n_key_exists_in_all_locales() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut sources = Vec::new();
    walk(&root.join("src"), &mut sources);

    let mut used: BTreeSet<String> = BTreeSet::new();
    for s in &sources {
        used.extend(extract_keys(s));
    }
    assert!(!used.is_empty(), "no i18n keys discovered — regex likely broken");

    let en = fs::read_to_string(root.join("locales/en.yml")).unwrap();
    let ru = fs::read_to_string(root.join("locales/ru.yml")).unwrap();
    let en_keys = parse_locale(&en);
    let ru_keys = parse_locale(&ru);

    let mut missing: Vec<String> = Vec::new();
    for k in &used {
        if !en_keys.contains(k) {
            missing.push(format!("en: {k}"));
        }
        if !ru_keys.contains(k) {
            missing.push(format!("ru: {k}"));
        }
    }
    assert!(missing.is_empty(), "missing i18n keys:\n  {}", missing.join("\n  "));

    let dyn_prefixes = dynamic_prefixes();
    let mut unused: Vec<String> = Vec::new();
    for k in &en_keys {
        if used.contains(k) {
            continue;
        }
        if dyn_prefixes.iter().any(|p| k.starts_with(p)) {
            continue;
        }
        if k.starts_with("onboarding.")
            || k.starts_with("errors.")
            || k.starts_with("status.")
            || k.starts_with("hard.")
            || k.starts_with("panic.")
            || k.starts_with("common.")
            || k.starts_with("tui.help.")
        {
            continue;
        }
        unused.push(k.clone());
    }
    assert!(
        unused.is_empty(),
        "unused i18n keys in en.yml (add to ignore list or remove):\n  {}",
        unused.join("\n  ")
    );

    let only_en: Vec<_> = en_keys.difference(&ru_keys).cloned().collect();
    let only_ru: Vec<_> = ru_keys.difference(&en_keys).cloned().collect();
    assert!(
        only_en.is_empty() && only_ru.is_empty(),
        "locale key drift — en-only: {only_en:?}, ru-only: {only_ru:?}"
    );
}

fn parse_locale(yaml: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut stack: Vec<(usize, String)> = Vec::new();
    for raw in yaml.lines() {
        if raw.trim().is_empty() || raw.trim_start().starts_with('#') {
            continue;
        }
        let indent = raw.len() - raw.trim_start().len();
        while stack.last().map(|(i, _)| *i >= indent).unwrap_or(false) {
            stack.pop();
        }
        let line = raw.trim();
        let Some(idx) = line.find(':') else { continue };
        let k = line[..idx].trim();
        let v = line[idx + 1..].trim();
        if k.starts_with('_') {
            continue;
        }
        if v.is_empty() {
            stack.push((indent, k.to_string()));
        } else {
            let prefix = stack.iter().map(|(_, s)| s.as_str()).collect::<Vec<_>>().join(".");
            let full = if prefix.is_empty() { k.to_string() } else { format!("{prefix}.{k}") };
            out.insert(full);
        }
    }
    out
}
