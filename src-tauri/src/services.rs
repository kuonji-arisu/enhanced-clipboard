/// 业务逻辑层，按职责划分为四个子模块：
/// - `app_info`：返回前后端共享的只读应用信息与权威常量
/// - `query`：只读查询（列表、搜索、日期）
/// - `ingest`：写入新条目（由 watcher 调用）
/// - `entry`：用户发起的条目操作（删除、置顶、清空、复制回写）
/// - `prune`：存储清理（时间窗口 + 数量限制淘汰）
/// - `settings`：设置读取/保存与相关运行时副作用
/// - `persisted_state`：非设置页、best-effort 的持久化状态恢复与更新
pub mod app_info;
pub mod entry;
pub mod ingest;
pub mod persisted_state;
pub mod prune;
pub mod query;
pub mod runtime;
pub mod settings;
