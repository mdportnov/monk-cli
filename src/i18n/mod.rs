mod detect;

pub use detect::detect;
pub use rust_i18n::t;

pub const SUPPORTED: &[&str] = &["en", "ru"];

pub fn set(locale: &str) {
    let normalized = normalize(locale);
    rust_i18n::set_locale(normalized);
}

pub fn current() -> String {
    rust_i18n::locale().to_string()
}

pub fn normalize(raw: &str) -> &'static str {
    let lower = raw.to_lowercase();
    if lower.starts_with("ru") {
        "ru"
    } else {
        "en"
    }
}

pub fn init(config_locale: Option<&str>, cli_override: Option<&str>) {
    let locale = detect(cli_override, config_locale);
    set(locale);
}
