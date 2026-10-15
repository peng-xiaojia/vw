use crate::{
    damaged,
    package::{resolve, Package},
    xml::{runs, Node},
};
use std::collections::HashMap;
use vw_protocol::{Block, PluginError};

const MAX_SLIDES: usize = 500;

pub(crate) fn parse(package: &mut Package) -> Result<Vec<Block>, PluginError> {
    let paths = if package.contains("ppt/presentation.xml") {
        let presentation = package.xml("ppt/presentation.xml")?;
        let relationships = package.xml("ppt/_rels/presentation.xml.rels")?;
        let mut targets = HashMap::new();
        for relation in relationships.all("Relationship") {
            if relation.attr("TargetMode") == Some("External") {
                continue;
            }
            if let (Some(id), Some(target)) = (relation.attr("Id"), relation.attr("Target")) {
                targets.insert(id, target);
            }
        }
        presentation
            .all("sldId")
            .into_iter()
            .map(|slide| {
                let id = slide
                    .attr("r:id")
                    .or_else(|| slide.attr("id"))
                    .ok_or_else(|| damaged("幻灯片关系 ID 缺失"))?;
                resolve(
                    "ppt",
                    targets
                        .get(id)
                        .ok_or_else(|| damaged("幻灯片关系不存在或为外部关系"))?,
                )
            })
            .collect::<Result<Vec<_>, PluginError>>()?
    } else {
        let mut slides: Vec<_> = package
            .names()
            .filter_map(|name| {
                name.strip_prefix("ppt/slides/slide")?
                    .strip_suffix(".xml")?
                    .parse::<u32>()
                    .ok()
                    .map(|number| (number, name.to_owned()))
            })
            .collect();
        slides.sort_by_key(|(number, _)| *number);
        slides.into_iter().map(|(_, path)| path).collect()
    };
    let mut blocks = Vec::new();
    for (index, path) in paths.iter().take(MAX_SLIDES).enumerate() {
        let slide = package.xml(path)?;
        if slide.name != "sld" {
            return Err(damaged("幻灯片根节点无效"));
        }
        let shapes = slide.all("sp");
        let title = shapes
            .iter()
            .find(|shape| is_title(shape))
            .map(|shape| paragraphs(shape).join(" "))
            .filter(|s| !s.is_empty());
        blocks.push(Block::heading(
            1,
            format!(
                "第 {} 页{}",
                index + 1,
                title.map(|s| format!(" · {s}")).unwrap_or_default()
            ),
        ));
        for shape in shapes {
            if !is_title(shape) {
                blocks.extend(paragraphs(shape).into_iter().map(Block::para));
            }
        }
        for table in slide.all("tbl") {
            let rows = table
                .children
                .iter()
                .filter(|n| n.name == "tr")
                .take(500)
                .map(|row| {
                    row.children
                        .iter()
                        .filter(|n| n.name == "tc")
                        .take(128)
                        .map(|cell| vw_protocol::Inline::text(paragraphs(cell).join("\n")))
                        .collect()
                })
                .collect();
            blocks.push(Block::Table {
                head: None,
                rows,
                align: Vec::new(),
            });
        }
        blocks.push(Block::PageBreak);
    }
    if paths.len() > MAX_SLIDES {
        blocks.push(Block::para("幻灯片预览已截断（最多 500 页）"));
    }
    if blocks.is_empty() {
        blocks.push(Block::para("（空演示文稿）"));
    }
    Ok(blocks)
}
fn is_title(node: &Node) -> bool {
    node.all("ph")
        .iter()
        .any(|ph| matches!(ph.attr("type"), Some("title" | "ctrTitle")))
}
fn paragraphs(node: &Node) -> Vec<String> {
    node.all("p")
        .into_iter()
        .map(runs)
        .filter(|s| !s.is_empty())
        .collect()
}
