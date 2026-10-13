//! IR 上的通用搜索与文本提取。所有插件的 `search` 语义由此统一：
//! 不区分大小写的子串匹配，字符偏移定位。

use crate::ir::{Block, Inline};
use crate::types::{Cursor, SearchHit, SearchHits};

/// 单次调用返回的命中上限；调用方用 `next` 游标续传。
pub const MAX_HITS: usize = 50;

const EXCERPT_CTX: usize = 24;

/// 行内元素序列的纯文本。
pub fn run_text(v: &[Inline]) -> String {
    v.iter().map(inline_text).collect()
}

pub fn inline_text(i: &Inline) -> String {
    match i {
        Inline::Text { text } => text.clone(),
        Inline::Bold { inner } | Inline::Italic { inner } => run_text(inner),
        Inline::Code { text } => text.clone(),
        Inline::Link { text, .. } => run_text(text),
        Inline::Break => "\n".into(),
    }
}

/// 块的可搜索文本。`PageBreak` / `Canvas` 无文本。
pub fn block_text(b: &Block) -> Option<String> {
    match b {
        Block::Heading { text, .. } | Block::Para { text } => Some(run_text(text)),
        Block::List { items, .. } => Some(
            items
                .iter()
                .filter_map(block_text)
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        Block::Table { head, rows, .. } => {
            let mut lines = Vec::new();
            if let Some(h) = head {
                lines.push(h.iter().map(inline_text).collect::<Vec<_>>().join("\t"));
            }
            for r in rows {
                lines.push(r.iter().map(inline_text).collect::<Vec<_>>().join("\t"));
            }
            Some(lines.join("\n"))
        }
        Block::Code { text, .. } | Block::Preformatted { text } => Some(text.clone()),
        Block::KeyValue { pairs } => Some(
            pairs
                .iter()
                .map(|(k, v)| format!("{k}: {}", inline_text(v)))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        Block::Quote { blocks } => Some(
            blocks
                .iter()
                .filter_map(block_text)
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        Block::Image { alt, .. } => Some(alt.clone()).filter(|s| !s.is_empty()),
        Block::Error { message } => Some(message.clone()),
        Block::PageBreak | Block::Canvas { .. } => None,
    }
}

/// 逐块子串搜索（不区分大小写）。`from` 之前的内容跳过。
pub fn search_blocks(blocks: &[Block], query: &str, from: Cursor) -> SearchHits {
    let qchars: Vec<char> = query.to_lowercase().chars().collect();
    let mut hits = Vec::new();
    let mut next = None;
    if qchars.is_empty() {
        return SearchHits { hits, next };
    }

    'outer: for (bi, b) in blocks.iter().enumerate().skip(from.block as usize) {
        let Some(text) = block_text(b) else { continue };
        let chars: Vec<char> = text.chars().collect();
        // 逐字符小写比较，保持原文与偏移一一对应（跳过罕见的多重映射字符）。
        let lower: Vec<char> = chars.iter().map(|c| c.to_lowercase().next().unwrap_or(*c)).collect();
        let start = if bi as u64 == from.block { (from.offset as usize).min(chars.len()) } else { 0 };
        let mut i = start;
        while i + qchars.len() <= lower.len() {
            if lower[i..i + qchars.len()] == qchars[..] {
                let s = i.saturating_sub(EXCERPT_CTX);
                let e = (i + qchars.len() + EXCERPT_CTX).min(chars.len());
                let mut excerpt: String = chars[s..e].iter().collect();
                if s > 0 {
                    excerpt.insert(0, '…');
                }
                if e < chars.len() {
                    excerpt.push('…');
                }
                hits.push(SearchHit { block: bi as u64, start: i as u64, excerpt });
                if hits.len() >= MAX_HITS {
                    next = Some(Cursor { block: bi as u64, offset: (i + qchars.len()) as u64 });
                    break 'outer;
                }
                i += qchars.len();
            } else {
                i += 1;
            }
        }
    }

    SearchHits { hits, next }
}
