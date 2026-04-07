# Enhanced Clipboard

一款基于 Tauri v2 + Vue 3 + TypeScript 构建的增强剪贴板管理工具，专为 Windows 设计。

## 功能特性

- **功能简洁**：无多余复杂功能，专注于剪贴板历史记录和管理
- **隐私友好**：本地存储，不连接互联网，无数据泄露风险
- **剪贴板库加密**：使用 SQLCipher 加密，密钥保存在 Windows Credential Manager
- **搜索过滤**：支持关键词搜索、按日期筛选（日历选择器）
- **置顶展示**：未筛选时首页优先显示置顶；搜索或按日期筛选时仅显示命中结果
- **快速复制**：文本条目直接回写剪贴板；图片条目回写文件列表，避免大图复制卡顿
- **全局热键**：可在设置中自定义快捷键唤起窗口（默认 `Ctrl+Shift+V`）
- **开机自启**：支持与系统自启动状态同步
- **托盘驻留**：点击关闭按钮最小化到系统托盘；左键单击托盘图标切换窗口显示；右键菜单支持"显示窗口"和"退出"
- **窗口置顶**：标题栏图钉按钮可切换窗口始终在最前
- **深色/浅色主题**：支持手动切换，设置持久化
- **多语言**：支持中文 / 英文，默认跟随系统语言
- **自动过期与清理**：支持 TTL 清理和最大历史数量限制，置顶条目不会自动删除
- **异步图片管线**：图片条目先即时入列，再后台落盘原图与缩略图
- **文件日志**：支持 `silent / error / info / debug` 日志等级，后端日志写入本地日志文件

## 技术栈

| 层 | 技术 |
|---|---|
| 框架 | Tauri v2 |
| 前端 | Vue 3 + TypeScript + Vite |
| 状态管理 | Pinia |
| 样式 | CSS 变量 + Tailwind CSS v4（仅布局） |
| 图标 | vite-plugin-svg-icons（SVG sprite） |
| 后端 | Rust |
| 数据库 | `clipboard.db`: SQLCipher + SQLite（WAL）；`settings.db`: plain SQLite |

## 数据与安全说明

- 剪贴板历史元数据与文本内容存储在加密的 `clipboard.db` 中。
- `settings.db` 保持明文，避免把普通设置也卷入密钥管理复杂度。
- 图片文件仍以文件形式存放在应用数据目录的 `images/` 和 `thumbnails/` 中，目前不做额外文件级加密。
- 当前仍处于预发布阶段；如果本地凭据异常、密钥丢失或 `clipboard.db` 损坏，应用会直接重建剪贴板数据库，不做兼容恢复。

## 开发环境搭建

### 前提条件

- [Node.js](https://nodejs.org/) ≥ 18
- [pnpm](https://pnpm.io/)
- [Rust](https://www.rust-lang.org/tools/install)（含 `cargo`）
- [Tauri CLI 前置依赖](https://v2.tauri.app/start/prerequisites/)（Windows 需要 WebView2）

### 安装依赖

```bash
pnpm install
```

### 启动开发模式

```bash
pnpm tauri dev
```

### 类型检查

```bash
pnpm exec vue-tsc --noEmit   # TypeScript
cd src-tauri && cargo check  # Rust
```

### 生产构建

```bash
pnpm tauri build
```

## License

MIT
