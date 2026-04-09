/// 编译时嵌入共享 i18n JSON，供 Rust 侧访问同一套字符串。
use std::collections::HashMap;

use include_dir::{include_dir, Dir};

pub const DEFAULT_LOCALE: &str = "en-US";

static I18N_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../i18n");

pub struct I18n {
    primary: HashMap<String, String>,
    fallback: HashMap<String, String>,
}

impl I18n {
    pub fn t(&self, key: &str) -> String {
        self.primary
            .get(key)
            .or_else(|| self.fallback.get(key))
            .cloned()
            .unwrap_or_else(|| key.to_string())
    }

    pub fn t_fmt(&self, key: &str, params: &[(&str, String)]) -> String {
        let template = self.t(key);
        let params = params
            .iter()
            .map(|(name, value)| (*name, value.as_str()))
            .collect::<HashMap<_, _>>();
        let chars = template.chars().collect::<Vec<_>>();
        let mut output = String::with_capacity(template.len());
        let mut index = 0;

        while index < chars.len() {
            if chars[index] != '{' {
                output.push(chars[index]);
                index += 1;
                continue;
            }

            let start = index;
            index += 1;
            let name_start = index;

            while index < chars.len() && (chars[index].is_ascii_alphanumeric() || chars[index] == '_')
            {
                index += 1;
            }

            if name_start < index && index < chars.len() && chars[index] == '}' {
                let name = chars[name_start..index].iter().collect::<String>();
                if let Some(value) = params.get(name.as_str()) {
                    output.push_str(value);
                } else {
                    output.extend(chars[start..=index].iter());
                }
                index += 1;
                continue;
            }

            output.push('{');
            index = start + 1;
        }
        output
    }
}

fn locale_file_stem(path: &str) -> Option<String> {
    path.rsplit('/')
        .next()?
        .strip_suffix(".json")
        .map(|stem| stem.to_string())
}

fn available_locale_names() -> Vec<String> {
    I18N_DIR
        .files()
        .filter_map(|file| locale_file_stem(file.path().to_string_lossy().as_ref()))
        .collect()
}

fn load_supported_messages(locale: &str) -> HashMap<String, String> {
    let file_name = format!("{locale}.json");
    I18N_DIR
        .get_file(file_name)
        .and_then(|file| file.contents_utf8())
        .and_then(|json| serde_json::from_str(json).ok())
        .unwrap_or_default()
}

fn normalize_locale_tag(locale: &str) -> String {
    locale.trim().replace('_', "-").to_ascii_lowercase()
}

#[cfg(windows)]
pub fn current_locale() -> String {
    use windows_sys::Win32::Globalization::GetUserDefaultLocaleName;

    const WINDOWS_LOCALE_NAME_MAX_LENGTH: i32 = 85;

    let mut buffer = vec![0u16; WINDOWS_LOCALE_NAME_MAX_LENGTH as usize];
    let written =
        unsafe { GetUserDefaultLocaleName(buffer.as_mut_ptr(), WINDOWS_LOCALE_NAME_MAX_LENGTH) };
    if written > 0 {
        let locale = String::from_utf16_lossy(&buffer[..(written as usize - 1)]);
        if !locale.trim().is_empty() {
            return locale;
        }
    }
    sys_locale::get_locale().unwrap_or_else(|| DEFAULT_LOCALE.to_string())
}

#[cfg(not(windows))]
pub fn current_locale() -> String {
    sys_locale::get_locale().unwrap_or_else(|| DEFAULT_LOCALE.to_string())
}

pub fn resolve_app_locale(locale: Option<&str>) -> String {
    let normalized = normalize_locale_tag(locale.unwrap_or_default());
    let available = available_locale_names();

    if let Some(exact_match) = available
        .iter()
        .find(|item| normalize_locale_tag(item) == normalized)
    {
        return exact_match.clone();
    }

    DEFAULT_LOCALE.to_string()
}

/// 根据启动期 locale 加载翻译表。
/// 优先匹配当前 locale，其次回退到 en-US，最后由 `t()` 回退到 key。
pub fn load(locale: &str) -> I18n {
    let effective = resolve_app_locale(Some(locale));
    let primary = load_supported_messages(&effective);
    let fallback = if effective == DEFAULT_LOCALE {
        HashMap::new()
    } else {
        load_supported_messages(DEFAULT_LOCALE)
    };
    I18n { primary, fallback }
}
