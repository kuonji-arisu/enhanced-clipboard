// 在 release 模式下禁止弹出额外的控制台窗口（Windows），请勿删除
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    enhanced_clipboard_lib::run()
}
