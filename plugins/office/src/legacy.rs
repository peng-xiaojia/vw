//! Legacy parsing is delegated to office_oxide (MIT OR Apache-2.0).
//! Only content is converted to our IR; no macros or external links run.
use crate::{damaged, OfficeLimits};
use office_oxide::ir::{inline_to_text, Element};
use vw_protocol::{Align, Block, Inline, PluginError};

pub fn parse(path: &std::path::Path, limits: OfficeLimits) -> Result<Vec<Block>, PluginError> {
    use std::io::Read;
    let file = std::fs::File::open(path)?;
    if file.metadata()?.len() > limits.xml_bytes { return Err(damaged("旧版 Office 文件超过 32 MiB")); }
    let mut bytes = Vec::new();
    file.take(limits.xml_bytes + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limits.xml_bytes { return Err(damaged("读取期间文件超出限制")); }
    let format = match path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "doc" => office_oxide::DocumentFormat::Doc,
        "xls" => office_oxide::DocumentFormat::Xls,
        _ => office_oxide::DocumentFormat::Ppt,
    };
    let document = office_oxide::Document::from_reader(std::io::Cursor::new(bytes), format).map_err(damaged)?;
    let ir = document.to_ir();
    let mut blocks = Vec::new();
    let mut budget = limits.xml_bytes as usize;
    for section in &ir.sections {
        if let Some(title) = &section.title { push_text(&mut blocks, Block::Heading { level: 1, text: vec![Inline::text(title)] }, &mut budget)?; }
        for element in &section.elements { convert(element, &mut blocks, &mut budget, 0)?; }
    }
    if blocks.is_empty() { blocks.push(Block::para("（空文档）")); }
    Ok(blocks)
}

fn push_text(blocks: &mut Vec<Block>, block: Block, budget: &mut usize) -> Result<(), PluginError> {
    let size = serde_json::to_vec(&block).map_err(PluginError::from)?.len();
    *budget = budget.checked_sub(size).ok_or_else(|| damaged("正文超过预览限制"))?;
    if blocks.len() >= 65000 { return Err(damaged("文档块超过预览限制")); }
    blocks.push(block); Ok(())
}

fn convert(element: &Element, blocks: &mut Vec<Block>, budget: &mut usize, depth: usize) -> Result<(), PluginError> {
    if depth > 32 { return Err(damaged("嵌套结构过深")); }
    match element {
        Element::Paragraph(p) => push_text(blocks, Block::para(inline_to_text(&p.content)), budget)?,
        Element::Heading(h) => push_text(blocks, Block::Heading { level: h.clamped_level(), text: vec![Inline::text(inline_to_text(&h.content))] }, budget)?,
        Element::Table(table) => {
            let mut rows = Vec::new(); let mut width = 0;
            for row in table.rows.iter().take(500) {
                let mut cells = Vec::new();
                for cell in row.cells.iter().take(128) {
                    let mut nested = Vec::new();
                    for child in &cell.content { convert(child, &mut nested, budget, depth + 1)?; }
                    let text = nested.iter().filter_map(vw_protocol::block_text).collect::<Vec<_>>().join("\n");
                    cells.push(Inline::text(text));
                }
                width = width.max(cells.len()); rows.push(cells);
            }
            push_text(blocks, Block::Table { head: None, align: vec![Align::Left; width], rows }, budget)?;
        }
        Element::List(list) => convert_list(list, blocks, budget, depth + 1)?,
        Element::TextBox(container) => {
            for child in &container.content { convert(child, blocks, budget, depth + 1)?; }
        }
        Element::Footnote(note) | Element::Endnote(note) => {
            for child in &note.content { convert(child, blocks, budget, depth + 1)?; }
        }
        Element::CodeBlock(code) => push_text(blocks, Block::preformatted(&code.content), budget)?,
        Element::Image(_) => push_text(blocks, Block::para("（嵌入图片）"), budget)?,
        _ => {},
    }
    Ok(())
}

fn convert_list(list: &office_oxide::ir::List, blocks: &mut Vec<Block>, budget: &mut usize, depth: usize) -> Result<(), PluginError> {
    if depth > 32 { return Err(damaged("列表嵌套过深")); }
    let mut items = Vec::new();
    for item in &list.items {
        for child in &item.content { convert(child, &mut items, budget, depth + 1)?; }
        if let Some(nested) = &item.nested { convert_list(nested, &mut items, budget, depth + 1)?; }
    }
    push_text(blocks, Block::List { ordered: list.ordered, items }, budget)
}
