use super::normalize;

pub fn detect(cli_override: Option<&str>, config_locale: Option<&str>) -> &'static str {
    if let Some(l) = cli_override {
        return normalize(l);
    }
    if let Ok(l) = std::env::var("MONK_LOCALE") {
        if !l.is_empty() {
            return normalize(&l);
        }
    }
    if let Some(l) = config_locale {
        if !l.is_empty() {
            return normalize(l);
        }
    }
    if let Some(sys) = sys_locale::get_locale() {
        return normalize(&sys);
    }
    "en"
}
