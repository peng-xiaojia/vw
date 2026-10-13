//! IR → JSON（CLI `--json`、MCP、GUI 共用同一份序列化，设计规格 §4.3）。

use serde::Serialize;

use vw_protocol::{Block, DocMeta};

#[derive(Serialize)]
struct DocJson<'a> {
    meta: &'a DocMeta,
    #[serde(skip_serializing_if = "Option::is_none")]
    total: Option<u64>,
    blocks: &'a [Block],
}

pub fn render_json(meta: &DocMeta, blocks: &[Block], total: Option<u64>) -> String {
    let doc = DocJson { meta, total, blocks };
    serde_json::to_string_pretty(&doc).unwrap_or_else(|e| {
        format!("{{\"error\":{{\"kind\":\"internal\",\"message\":\"IR 序列化失败：{e}\"}}}}")
    })
}
