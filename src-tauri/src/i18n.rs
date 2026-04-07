/// 编译时嵌入共享 i18n JSON，供 Rust 侧访问同一套字符串。
use std::collections::HashMap;

pub struct I18n(HashMap<String, String>);

impl I18n {
    pub fn t(&self, key: &str) -> String {
        self.0.get(key).cloned().unwrap_or_else(|| key.to_string())
    }
}

/// 检测操作系统首选语言，返回 "zh" 或 "en"。
fn detect_system_lang() -> String {
    sys_locale::get_locale()
        .filter(|l| l.starts_with("zh"))
        .map(|_| "zh".to_string())
        .unwrap_or_else(|| "en".to_string())
}

/// 根据语言代码加载对应翻译表。
/// - `"zh"` → 中文；`"en"` → 英文；`""` → 跟随系统语言。
/// JSON 在编译期嵌入二进制，无运行时 IO。
pub fn load(lang: &str) -> I18n {
    let effective = if lang.is_empty() {
        detect_system_lang()
    } else {
        lang.to_string()
    };
    let json = if effective == "zh" {
        include_str!("../../i18n/zh.json")
    } else {
        include_str!("../../i18n/en.json")
    };
    I18n(serde_json::from_str(json).unwrap_or_default())
}
