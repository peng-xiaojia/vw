//! vw 插件 SDK：写一个 sidecar 插件所需的全部设施。
//!
//! 插件实现 [`RpcViewer`] / [`RpcDoc`]，然后调用 [`serve`] 接管 stdin/stdout。
//! 与传输无关：`serve` 接受任意 `Read + Write`，测试可用 [`mempipe`] 内存管道。

pub mod mempipe;
pub mod serve;

pub use mempipe::{mempipe, PipeEnd};
pub use serve::{enable_idle_exit_from_env, serve, CancelFlag, RpcDoc, RpcViewer};
