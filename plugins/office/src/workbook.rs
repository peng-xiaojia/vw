use crate::{
    damaged,
    package::{resolve, Package},
    xml::{runs, Node},
};
use std::{borrow::Cow, collections::HashMap};
use vw_protocol::{Align, Block, Inline, PluginError};

const MAX_ROWS: usize = 500;
const MAX_COLUMNS: usize = 128;
const MAX_SHEETS: usize = 128;
const MAX_PREVIEW_TEXT: usize = 16 * 1024 * 1024;
const MAX_PREVIEW_CELLS: usize = 65_000;
#[derive(Default)]
struct Budget {
    text: usize,
    cells: usize,
}
impl Budget {
    fn text(&mut self, bytes: usize) -> Result<(), PluginError> {
        self.text = self
            .text
            .checked_add(bytes)
            .filter(|n| *n <= MAX_PREVIEW_TEXT)
            .ok_or_else(|| damaged("预览文本超过 16 MiB 上限"))?;
        Ok(())
    }
}

pub(crate) fn parse(package: &mut Package) -> Result<Vec<Block>, PluginError> {
    let workbook = package.xml("xl/workbook.xml")?;
    if workbook.name != "workbook" {
        return Err(damaged("Excel 根节点无效"));
    }
    let relationships = package.xml("xl/_rels/workbook.xml.rels")?;
    let mut targets = HashMap::new();
    for relation in relationships.all("Relationship") {
        if relation.attr("TargetMode") == Some("External") {
            continue;
        }
        if let (Some(id), Some(target)) = (relation.attr("Id"), relation.attr("Target")) {
            targets.insert(id.to_owned(), target.to_owned());
        }
    }
    let strings = if package.contains("xl/sharedStrings.xml") {
        package
            .xml("xl/sharedStrings.xml")?
            .all("si")
            .into_iter()
            .map(runs)
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let sheets = workbook
        .child("sheets")
        .ok_or_else(|| damaged("缺少工作表列表"))?;
    let mut blocks = Vec::new();
    let mut budget = Budget::default();
    for sheet in sheets
        .children
        .iter()
        .filter(|n| n.name == "sheet")
        .take(MAX_SHEETS)
    {
        let id = sheet
            .attr("r:id")
            .or_else(|| sheet.attr("id"))
            .ok_or_else(|| damaged("工作表缺少关系 ID"))?;
        let target = targets
            .get(id)
            .ok_or_else(|| damaged("工作表关系不存在或为外部关系"))?;
        let root = package.xml(&resolve("xl", target)?)?;
        if root.name != "worksheet" {
            return Err(damaged("工作表根节点无效"));
        }
        let name = sheet.attr("name").unwrap_or("工作表");
        budget.text(name.len())?;
        blocks.push(Block::heading(1, name));
        let (table, truncated) = table(&root, &strings, &mut budget)?;
        blocks.push(table);
        if truncated {
            blocks.push(Block::para("工作表预览已截断（最多 500 行、128 列）"));
        }
    }
    if sheets.children.iter().filter(|n| n.name == "sheet").count() > MAX_SHEETS {
        blocks.push(Block::para("工作表预览已截断（最多 128 张）"));
    }
    if blocks.is_empty() {
        blocks.push(Block::para("（空工作簿）"));
    }
    Ok(blocks)
}

fn column(reference: &str) -> Option<usize> {
    let mut value = 0usize;
    let letters: String = reference
        .chars()
        .take_while(char::is_ascii_alphabetic)
        .collect();
    if letters.is_empty() {
        return None;
    }
    for byte in letters.bytes() {
        value = value
            .checked_mul(26)?
            .checked_add(usize::from(byte.to_ascii_uppercase() - b'A') + 1)?;
    }
    value.checked_sub(1)
}
fn column_name(mut index: usize) -> String {
    let mut result = Vec::new();
    index += 1;
    while index > 0 {
        index -= 1;
        result.push((b'A' + (index % 26) as u8) as char);
        index /= 26;
    }
    result.into_iter().rev().collect()
}
fn cell_text<'a>(cell: &'a Node, strings: &'a [String]) -> Result<Cow<'a, str>, PluginError> {
    let value = cell.child("v").map(|n| n.text.as_str()).unwrap_or("");
    match cell.attr("t") {
        Some("s") => {
            let index = value
                .parse::<usize>()
                .map_err(|_| damaged("共享字符串索引无效"))?;
            strings
                .get(index)
                .map(|s| Cow::Borrowed(s.as_str()))
                .ok_or_else(|| damaged("共享字符串索引越界"))
        }
        Some("inlineStr") => Ok(Cow::Owned(cell.child("is").map(runs).unwrap_or_default())),
        Some("b") => Ok(if value == "1" { "TRUE" } else { "FALSE" }.into()),
        // Formulas are never evaluated; only the saved cached value is displayed.
        _ => Ok(Cow::Borrowed(value)),
    }
}
fn table(
    root: &Node,
    strings: &[String],
    budget: &mut Budget,
) -> Result<(Block, bool), PluginError> {
    let mut rows: Vec<Vec<Inline>> = Vec::new();
    let mut columns = 0;
    let mut truncated = false;
    if let Some(data) = root.child("sheetData") {
        let mut next_row = 0;
        for row in data.children.iter().filter(|n| n.name == "row") {
            let index = match row.attr("r") {
                Some(value) => value
                    .parse::<usize>()
                    .ok()
                    .and_then(|n| n.checked_sub(1))
                    .ok_or_else(|| damaged("行号无效"))?,
                None => next_row,
            };
            next_row = index.saturating_add(1);
            if index >= MAX_ROWS {
                truncated = true;
                continue;
            }
            while rows.len() <= index {
                rows.push(vec![Inline::text((rows.len() + 1).to_string())]);
            }
            let mut next_column = 0;
            for cell in row.children.iter().filter(|n| n.name == "c") {
                let col = match cell.attr("r") {
                    Some(reference) => {
                        column(reference).ok_or_else(|| damaged("单元格引用无效"))?
                    }
                    None => next_column,
                };
                next_column = col.saturating_add(1);
                if col >= MAX_COLUMNS {
                    truncated = true;
                    continue;
                }
                columns = columns.max(col + 1);
                if rows[index].len() < col + 2 {
                    rows[index].resize(col + 2, Inline::text(""));
                }
                let value = cell_text(cell, strings)?;
                budget.text(value.len())?;
                rows[index][col + 1] = Inline::text(value.into_owned());
            }
        }
    }
    budget.cells = budget
        .cells
        .checked_add((rows.len() + 1) * (columns + 1))
        .filter(|n| *n <= MAX_PREVIEW_CELLS)
        .ok_or_else(|| damaged("预览总单元格超过 65000 个上限"))?;
    for row in &mut rows {
        row.resize(columns + 1, Inline::text(""));
    }
    let mut head = vec![Inline::text("行")];
    head.extend((0..columns).map(|n| Inline::text(column_name(n))));
    Ok((
        Block::Table {
            head: Some(head),
            rows,
            align: vec![Align::Left; columns + 1],
        },
        truncated,
    ))
}
