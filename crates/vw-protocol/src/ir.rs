//! Document IR：插件产出的唯一内容格式（设计规格 §5.2）。

use serde::{Deserialize, Serialize};

use crate::types::BlockList;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Inline {
    Text { text: String },
    Bold { inner: Box<Inline> },
    Italic { inner: Box<Inline> },
    Code { inner: Box<Inline> },
    Link { text: Box<Inline>, href: String },
    Break,
}

/// 受控的矢量绘图原语。**不含文本定位**——一旦加入文字排版，IR 就开始
/// 退化为排版引擎（设计规格 §5.2 Canvas 暂缓说明）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum DrawOp {
    Fill { points: Vec<[f32; 2]>, color: String },
    Stroke { points: Vec<[f32; 2]>, width: f32, color: String },
    Rect { x: f32, y: f32, w: f32, h: f32, color: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Block {
    Heading { level: u8, text: Inline },
    Para { text: Inline },
    List { ordered: bool, items: Vec<Block> },
    Table { head: Option<Vec<Inline>>, rows: Vec<Vec<Inline>>, align: Vec<Align> },
    Code { lang: Option<String>, text: String },
    KeyValue { pairs: Vec<(String, Inline)> },
    Quote { blocks: Vec<Block> },
    Image { blob: String, alt: String, w: Option<u32>, h: Option<u32> },
    PageBreak,
    /// 暂缓引入（见规格 §5.2）：P1–P5 的插件都不得产出该块，仅保留协议形状。
    Canvas { w: u32, h: u32, ops: Vec<DrawOp> },
    /// 逃生舱：插件不想建结构时原样输出。
    Preformatted { text: String },
    /// 局部失败，不炸整篇文档；未知块类型反序列化时也落到这里。
    Error { message: String },
}

impl Inline {
    pub fn text(s: impl Into<String>) -> Inline {
        Inline::Text { text: s.into() }
    }
}

impl Block {
    pub fn para(s: impl Into<String>) -> Block {
        Block::Para { text: Inline::text(s) }
    }

    pub fn heading(level: u8, s: impl Into<String>) -> Block {
        Block::Heading { level, text: Inline::text(s) }
    }

    pub fn preformatted(s: impl Into<String>) -> Block {
        Block::Preformatted { text: s.into() }
    }

    pub fn error(s: impl Into<String>) -> Block {
        Block::Error { message: s.into() }
    }
}

/// 从 JSON 值解析块。未知 `kind` 或非法结构一律降级为 `Block::Error`，
/// 绝不让整个文档加载失败（设计规格 §5.2 纪律 4：前向兼容）。
pub fn block_from_value(v: &serde_json::Value) -> Block {
    serde_json::from_value(v.clone()).unwrap_or_else(|e| {
        Block::error(format!("未知块类型或非法块数据（前向兼容降级）：{e}"))
    })
}

/// 从 JSON 值解析块列表，逐块应用未知类型降级：单个坏块只影响自己，
/// 不让整篇文档加载失败（设计规格 §5.2 纪律 4）。
pub fn block_list_from_value(v: &serde_json::Value) -> BlockList {
    let obj = match v.as_object() {
        Some(o) => o,
        None => {
            return BlockList {
                blocks: vec![Block::error("块列表不是 JSON 对象（前向兼容降级）")],
                total: None,
            }
        }
    };
    let mut blocks = Vec::new();
    match obj.get("blocks").and_then(|b| b.as_array()) {
        Some(arr) => {
            for item in arr {
                blocks.push(block_from_value(item));
            }
        }
        None => blocks.push(Block::error("缺少 blocks 数组（前向兼容降级）")),
    }
    let total = obj.get("total").and_then(|t| t.as_u64());
    BlockList { blocks, total }
}
