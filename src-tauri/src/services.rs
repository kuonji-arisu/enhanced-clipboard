pub(crate) mod app_info;
/// 业务逻辑层，按职责拆分为小型服务模块：
/// - `app_info`：返回前后端共享的只读应用信息与权威常量
/// - `query`：只读查询（列表、搜索、日期）
/// - `projection` / `search_preview`：列表 read model 与搜索摘要投影
/// - `ingest`：写入新条目（由 watcher 调用）
/// - `jobs`：轻量 durable deferred job claim/recovery 和 polling dedup 协调
/// - `entry`：用户发起的条目操作（删除、置顶、清空、复制回写）
/// - `prune`：存储清理（时间窗口 + 数量限制淘汰）
/// - `artifacts`：资产文件写入、路径安全、图片展示资产和维护计划
/// - `settings`：设置读取/保存与相关运行时副作用
/// - `persisted_state`：非设置页、best-effort 的持久化状态恢复与更新
pub mod artifacts;
pub mod effects;
pub mod entry;
pub(crate) mod entry_tags;
pub mod ingest;
pub mod jobs;
pub mod persisted_state;
pub mod pipeline;
pub mod projection;
pub mod prune;
pub mod query;
pub mod runtime;
pub mod search_preview;
pub mod settings;
pub mod view_events;
