fn main() {
    println!("cargo:rerun-if-changed=../i18n/zh.json");
    println!("cargo:rerun-if-changed=../i18n/en.json");
    tauri_build::build()
}
