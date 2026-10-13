//! vw conformance 一致性套件（设计规格 §12.1）。
//!
//! 同一份用例驱动任意「能打开本地文件」的后端：
//! 进程内注册表、内存管道 sidecar、真实子进程 sidecar。
//! 输出必须一致；这是「架构统一」从承诺变成结构保证的手段。

use std::path::{Path, PathBuf};

use vw_core::OpenedDoc;
use vw_protocol::{EditCapability, ErrorKind, LineEnding, OpenOptions, PluginError, Source};

/// 打开文件的入口：由被测后端提供。
pub type OpenFn<'a> = dyn Fn(&Path) -> Result<OpenedDoc, PluginError> + 'a;

pub fn write_file(dir: &Path, name: &str, content: &[u8]) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, content).unwrap();
    p
}

/// 基础查看：meta、分页、搜索、IR JSON 序列化保真（§4.2）。
pub fn case_view_and_search(open: &OpenFn, dir: &Path, name: &str) -> Result<(), String> {
    let content = "alpha line\nbeta line\ngamma line\nbeta again\n";
    let path = write_file(dir, name, content.as_bytes());
    let doc = open(&path).map_err(|e| format!("open 失败：{e}"))?;
    if doc.meta.mime.as_deref() != Some("text/plain") {
        return Err(format!("mime 不符：{:?}", doc.meta.mime));
    }
    let all = doc.page_all(10_000).map_err(|e| e.to_string())?;
    let joined = all.blocks.iter().filter_map(vw_protocol::block_text).collect::<Vec<_>>().join("");
    if !joined.contains("beta again") {
        return Err("分页内容缺行".into());
    }
    if all.total != Some(all.blocks.len() as u64) {
        return Err(format!("total 不一致：{:?} vs {}", all.total, all.blocks.len()));
    }
    let hits = doc.search("beta", vw_protocol::Cursor::start()).map_err(|e| e.to_string())?;
    if hits.hits.len() != 2 {
        return Err(format!("search 命中数应为 2，实际 {}", hits.hits.len()));
    }
    // IR JSON 往返保真：in-process 与 stdio 的块都必须经得起 serde 往返
    let json = serde_json::to_value(&all).map_err(|e| e.to_string())?;
    let back = vw_protocol::block_list_from_value(&json);
    if back != all {
        return Err("IR JSON 往返不一致".into());
    }
    doc.close();
    Ok(())
}

/// 编辑逐字节往返（§12.7）：BOM + CRLF + 无尾随换行。
pub fn case_edit_round_trip(open: &OpenFn, dir: &Path, name: &str) -> Result<(), String> {
    let raw: &[u8] = b"\xEF\xBB\xBFk,v\r\n1,2\r\n3";
    let path = write_file(dir, name, raw);
    let doc = open(&path).map_err(|e| format!("open 失败：{e}"))?;
    let cap = doc.edit_capability().ok_or("该文件应可编辑")?;
    match &cap {
        EditCapability::Text { has_bom, line_ending, .. } => {
            if !*has_bom {
                return Err("应声明 has_bom=true".into());
            }
            if *line_ending != LineEnding::Preserve {
                return Err("行尾应为 Preserve".into());
            }
        }
        other => return Err(format!("能力应为 Text，实际 {other:?}")),
    }
    let snap = doc.snapshot_text().map_err(|e| format!("snapshot 失败：{e}"))?;
    // 原样保存：文件字节必须不变
    doc.save_text(snap.clone()).map_err(|e| format!("原样保存失败：{e}"))?;
    let after = std::fs::read(&path).map_err(|e| e.to_string())?;
    if after != raw {
        return Err("原样保存改变了文件字节（往返不一致）".into());
    }
    // 修改后保存生效（BOM 仍在头部）
    doc.save_text(format!("{snap}\n4,4")).map_err(|e| format!("修改保存失败：{e}"))?;
    let after2 = std::fs::read(&path).map_err(|e| e.to_string())?;
    if !after2.starts_with(b"\xEF\xBB\xBF") || !after2.ends_with(b"\n4,4") {
        return Err("修改未正确写入".into());
    }
    doc.close();
    Ok(())
}

/// 冲突阻断（§12.8）：打开后外部篡改 → 保存必须被拒绝且不覆盖。
pub fn case_conflict(open: &OpenFn, dir: &Path, name: &str) -> Result<(), String> {
    let path = write_file(dir, name, b"original content here\n");
    let doc = open(&path).map_err(|e| e.to_string())?;
    std::fs::write(&path, b"tampered").map_err(|e| e.to_string())?;
    match doc.save_text("should not land".into()) {
        Err(e) if e.kind == ErrorKind::Conflict => {}
        Err(e) => return Err(format!("应为 Conflict，实际 {e}")),
        Ok(_) => return Err("篡改后保存未被阻断".into()),
    }
    if std::fs::read(&path).map_err(|e| e.to_string())? != b"tampered" {
        return Err("冲突后原文件被覆盖".into());
    }
    doc.close();
    Ok(())
}

/// 只读降级（§5.4）：无效 UTF-8 → 不可编辑但可查看。
pub fn case_readonly_degradation(open: &OpenFn, dir: &Path, name: &str) -> Result<(), String> {
    let path = write_file(dir, name, b"text \xFF\xFE invalid");
    let doc = open(&path).map_err(|e| e.to_string())?;
    if doc.edit_capability().is_some() {
        return Err("无效 UTF-8 应降级只读".into());
    }
    if doc.page_all(100).map_err(|e| e.to_string())?.blocks.is_empty() {
        return Err("只读文件应可查看".into());
    }
    if doc.save_text("x".into()).is_ok() {
        return Err("只读保存应失败".into());
    }
    doc.close();
    Ok(())
}

/// 一键全套（文本类插件的一致性验收）。
pub fn run_text_suite(open: &OpenFn, dir: &Path, ext: &str) -> Result<(), String> {
    case_view_and_search(open, dir, &format!("cf-view.{ext}"))?;
    case_edit_round_trip(open, dir, &format!("cf-edit.{ext}"))?;
    case_conflict(open, dir, &format!("cf-conflict.{ext}"))?;
    case_readonly_degradation(open, dir, &format!("cf-ro.{ext}"))?;
    Ok(())
}
