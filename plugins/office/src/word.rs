use crate::{
    damaged,
    package::Package,
    xml::{runs, Node},
};
use vw_protocol::{Align, Block, Inline, PluginError};

const MAX_TABLE_ROWS: usize = 500;
const MAX_TABLE_COLUMNS: usize = 128;
pub(crate) fn parse(package: &mut Package) -> Result<Vec<Block>, PluginError> {
    let root = package.xml("word/document.xml")?;
    if root.name != "document" {
        return Err(damaged("Word 根节点无效"));
    }
    let body = root
        .child("body")
        .ok_or_else(|| damaged("缺少 Word 正文"))?;
    let mut blocks = Vec::new();
    body_blocks(body, &mut blocks);
    if blocks.is_empty() {
        blocks.push(Block::para("（空文档）"));
    }
    Ok(blocks)
}
fn paragraph(node: &Node) -> Block {
    let text = runs(node);
    let properties = node.child("pPr");
    let style = properties
        .and_then(|p| p.child("pStyle"))
        .and_then(|p| p.attr("val"))
        .unwrap_or("")
        .to_ascii_lowercase();
    let level = style
        .strip_prefix("heading")
        .and_then(|n| n.parse::<u8>().ok())
        .filter(|n| (1..=6).contains(n))
        .or_else(|| {
            properties
                .and_then(|p| p.child("outlineLvl"))
                .and_then(|p| p.attr("val"))
                .and_then(|n| n.parse::<u8>().ok())
                .filter(|n| *n < 6)
                .map(|n| n + 1)
        });
    if let Some(level) = level {
        Block::heading(level, text)
    } else {
        Block::para(text)
    }
}
fn body_blocks(node: &Node, blocks: &mut Vec<Block>) {
    for child in &node.children {
        match child.name.as_str() {
            "p" => blocks.push(paragraph(child)),
            "tbl" => {
                let mut rows = Vec::new();
                let all_rows: Vec<_> = child.children.iter().filter(|n| n.name == "tr").collect();
                let mut truncated = all_rows.len() > MAX_TABLE_ROWS;
                for row in all_rows.into_iter().take(MAX_TABLE_ROWS) {
                    let cells: Vec<_> = row.children.iter().filter(|n| n.name == "tc").collect();
                    truncated |= cells.len() > MAX_TABLE_COLUMNS;
                    rows.push(
                        cells
                            .into_iter()
                            .take(MAX_TABLE_COLUMNS)
                            .map(|cell| {
                                Inline::text(
                                    cell.all("p")
                                        .iter()
                                        .map(|p| runs(p))
                                        .collect::<Vec<_>>()
                                        .join("\n"),
                                )
                            })
                            .collect::<Vec<_>>(),
                    );
                }
                let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
                blocks.push(Block::Table {
                    head: None,
                    rows,
                    align: vec![Align::Left; columns],
                });
                if truncated {
                    blocks.push(Block::para("表格预览已截断（最多 500 行、128 列）"));
                }
            }
            "sdt" | "sdtContent" | "customXml" => body_blocks(child, blocks),
            _ => {}
        }
    }
}
