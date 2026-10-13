//! IR → 纯文本（CLI 默认输出）。这是最弱的渲染器：任何块都必须有合理降级。

use vw_protocol::{inline_text, Align, Block, DocMeta, Inline};

pub fn inline_str(i: &Inline) -> String {
    inline_text(i)
}

pub fn render_blocks(blocks: &[Block]) -> String {
    let mut out = String::new();
    for b in blocks {
        render_block(b, 0, &mut out);
    }
    out
}

/// 文档头 + 块渲染。`truncated` 为 `(已渲染块数, 总块数)` 时输出提示。
pub fn render_document(meta: &DocMeta, blocks: &[Block], truncated: Option<(usize, u64)>) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "▶ {}　[viewer: {}{}]\n\n",
        meta.title.as_deref().unwrap_or(""),
        meta.viewer,
        meta.mime.as_deref().map(|m| format!(", {m}")).unwrap_or_default()
    ));
    out.push_str(&render_blocks(blocks));
    if let Some((got, total)) = truncated {
        out.push_str(&format!("\n… 已显示 {got}/{total} 块（其余未取）\n"));
    }
    out
}

fn render_block(b: &Block, depth: usize, out: &mut String) {
    let pad = "  ".repeat(depth);
    match b {
        Block::Heading { level, text } => {
            out.push_str(&pad);
            for _ in 0..(*level).clamp(1, 6) {
                out.push('#');
            }
            out.push(' ');
            out.push_str(&inline_str(text));
            out.push('\n');
        }
        Block::Para { text } => {
            out.push_str(&pad);
            out.push_str(&inline_str(text));
            out.push('\n');
        }
        Block::List { ordered, items } => {
            for (i, item) in items.iter().enumerate() {
                let marker = if *ordered { format!("{}. ", i + 1) } else { "- ".into() };
                match item {
                    Block::Para { text } => {
                        out.push_str(&pad);
                        out.push_str(&marker);
                        out.push_str(&inline_str(text));
                        out.push('\n');
                    }
                    Block::List { .. } => render_block(item, depth + 1, out),
                    other => {
                        out.push_str(&pad);
                        out.push_str(&marker);
                        out.push_str(&inline_str(&Inline::text(child_text(other))));
                        out.push('\n');
                    }
                }
            }
        }
        Block::Table { head, rows, align } => render_table(head, rows, align, out),
        Block::Code { text, lang } => {
            if let Some(l) = lang {
                out.push_str(&pad);
                out.push_str(&format!("```{l}\n"));
            }
            for line in text.lines() {
                out.push_str(&pad);
                out.push_str(line);
                out.push('\n');
            }
            if lang.is_some() {
                out.push_str(&pad);
                out.push_str("```\n");
            }
        }
        Block::KeyValue { pairs } => {
            for (k, v) in pairs {
                out.push_str(&pad);
                out.push_str(&format!("{k}: {}", inline_str(v)));
                out.push('\n');
            }
        }
        Block::Quote { blocks } => {
            let mut inner = String::new();
            for b in blocks {
                render_block(b, 0, &mut inner);
            }
            for line in inner.lines() {
                out.push_str(&pad);
                out.push_str("> ");
                out.push_str(line);
                out.push('\n');
            }
        }
        Block::Image { alt, w, h, .. } => {
            let dim = match (w, h) {
                (Some(w), Some(h)) => format!(" {w}x{h}"),
                _ => String::new(),
            };
            out.push_str(&pad);
            out.push_str(&format!("[图片 {alt}{dim}]\n"));
        }
        Block::PageBreak => out.push_str("---\n"),
        Block::Canvas { .. } => {
            out.push_str(&pad);
            out.push_str("[矢量绘图（文本渲染降级为占位）]\n");
        }
        Block::Preformatted { text } => {
            out.push_str(&pad);
            out.push_str(text);
            if !text.ends_with('\n') {
                out.push('\n');
            }
        }
        Block::Error { message } => {
            out.push_str(&pad);
            out.push_str(&format!("⚠ {message}\n"));
        }
    }
}

fn child_text(b: &Block) -> String {
    vw_protocol::block_text(b).unwrap_or_default()
}

fn pad_cell(c: &str, width: usize, align: Align) -> String {
    let len = c.chars().count();
    if len >= width {
        return c.to_string();
    }
    let fill = width - len;
    match align {
        Align::Left => format!("{c}{}", " ".repeat(fill)),
        Align::Right => format!("{}{c}", " ".repeat(fill)),
        Align::Center => {
            let l = fill / 2;
            format!("{}{c}{}", " ".repeat(l), " ".repeat(fill - l))
        }
    }
}

fn render_table(head: &Option<Vec<Inline>>, rows: &[Vec<Inline>], align: &[Align], out: &mut String) {
    let mut table: Vec<Vec<String>> = Vec::new();
    if let Some(h) = head {
        table.push(h.iter().map(inline_str).collect());
    }
    for r in rows {
        table.push(r.iter().map(inline_str).collect());
    }
    let ncols = table.iter().map(|r| r.len()).max().unwrap_or(0);
    if ncols == 0 {
        return;
    }
    let mut widths = vec![0usize; ncols];
    for r in &table {
        for (i, c) in r.iter().enumerate() {
            widths[i] = widths[i].max(c.chars().count());
        }
    }
    for (ri, r) in table.iter().enumerate() {
        let mut line = String::from("|");
        for (i, w) in widths.iter().enumerate() {
            let cell = r.get(i).map(String::as_str).unwrap_or("");
            line.push(' ');
            line.push_str(&pad_cell(cell, *w, align.get(i).copied().unwrap_or(Align::Left)));
            line.push_str(" |");
        }
        out.push_str(&line);
        out.push('\n');
        if ri == 0 && head.is_some() {
            let sep: String = widths.iter().map(|w| "-".repeat(w + 2)).collect::<Vec<_>>().join("+");
            out.push('|');
            out.push_str(&sep);
            out.push_str("|\n");
        }
    }
}
