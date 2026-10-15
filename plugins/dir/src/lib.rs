//! vw 内置插件：目录列表（P2）。处理 `Source::Dir`，产出成员表格。

use std::io::Read;
use std::time::UNIX_EPOCH;

use vw_core::viewer::{Doc, Viewer};
use vw_protocol::{
    search_blocks, Align, Block, BlockList, Cursor, DocMeta, Inline, Manifest, OpenOptions,
    Outline, PageRange, PluginError, ProbeScore, SearchHits, Source,
};

const ROWS_PER_BLOCK: usize = 500;

/// Native directory navigation data. Paths are identities; display names never
/// have to be parsed back from the viewer's table cells.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DirectoryListing {
    pub path: String,
    pub parent: Option<String>,
    pub entries: Vec<DirectoryEntry>,
}

pub fn list_directory(path: &std::path::Path) -> Result<DirectoryListing, PluginError> {
    let path = path.canonicalize().map_err(PluginError::from)?;
    if !path.is_dir() {
        return Err(PluginError::malformed("请求的路径不是目录"));
    }
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&path).map_err(PluginError::from)? {
        let entry = entry.map_err(PluginError::from)?;
        let metadata = entry.metadata().map_err(PluginError::from)?;
        entries.push(DirectoryEntry {
            name: entry.file_name().to_string_lossy().into_owned(),
            path: entry.path().display().to_string(),
            is_dir: metadata.is_dir(),
            size: if metadata.is_dir() {
                None
            } else {
                Some(metadata.len())
            },
        });
    }
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.path.cmp(&b.path))
    });
    Ok(DirectoryListing {
        path: path.display().to_string(),
        parent: path.parent().map(|p| p.display().to_string()),
        entries,
    })
}

pub struct DirViewer {
    manifest: Manifest,
    viewer_id: String,
}

impl DirViewer {
    pub fn new(priority: i64) -> Self {
        Self {
            manifest: Manifest::simple_in_process("vw.dir", "dir", &[], &["view"], priority),
            viewer_id: "dir".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Entry {
    Dir { name: String },
    File { name: String, size: u64 },
}

fn list_entries(path: &std::path::Path) -> Result<Vec<Entry>, PluginError> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(path).map_err(PluginError::from)? {
        let e = e.map_err(PluginError::from)?;
        let name = e.file_name().to_string_lossy().to_string();
        let md = e.metadata().map_err(PluginError::from)?;
        if md.is_dir() {
            out.push(Entry::Dir { name });
        } else {
            out.push(Entry::File {
                name,
                size: md.len(),
            });
        }
    }
    // 目录在前，各自按名称排序
    out.sort_by(|a, b| match (a, b) {
        (Entry::Dir { .. }, Entry::File { .. }) => std::cmp::Ordering::Less,
        (Entry::File { .. }, Entry::Dir { .. }) => std::cmp::Ordering::Greater,
        _ => entry_name(a).cmp(entry_name(b)),
    });
    Ok(out)
}

fn entry_name(e: &Entry) -> &str {
    match e {
        Entry::Dir { name } | Entry::File { name, .. } => name,
    }
}

fn mtime_secs(path: &std::path::Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Unix 时间戳 → `YYYY-MM-DD HH:MM`（无外部依赖的民用历换算）。
fn fmt_time(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (h, mi, _s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // Howard Hinnant civil_from_days
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02}")
}

fn build_blocks(dir: &std::path::Path, entries: &[Entry]) -> Vec<Block> {
    let mut rows: Vec<Vec<Inline>> = entries
        .iter()
        .map(|e| {
            let child = dir.join(entry_name(e));
            let (kind, size) = match e {
                Entry::Dir { .. } => ("目录", "-".to_string()),
                Entry::File { size, .. } => ("文件", size.to_string()),
            };
            vec![
                Inline::text(entry_name(e)),
                Inline::text(kind),
                Inline::text(size),
                Inline::text(fmt_time(mtime_secs(&child))),
            ]
        })
        .collect();
    let head = vec![
        Inline::text("名称"),
        Inline::text("类型"),
        Inline::text("大小"),
        Inline::text("修改时间"),
    ];
    let mut blocks = Vec::new();
    if rows.is_empty() {
        rows.push(vec![Inline::text("（空目录）")]);
    }
    for chunk in rows.chunks(ROWS_PER_BLOCK) {
        blocks.push(Block::Table {
            head: Some(head.clone()),
            rows: chunk.to_vec(),
            align: vec![Align::Left, Align::Left, Align::Right, Align::Left],
        });
    }
    blocks
}

struct DirDoc {
    viewer_id: String,
    blocks: Vec<Block>,
    title: Option<String>,
    total: u64,
}

impl Doc for DirDoc {
    fn meta(&self) -> DocMeta {
        DocMeta {
            viewer: self.viewer_id.clone(),
            mime: Some("inode/directory".into()),
            size: None,
            title: self.title.clone(),
            pages: Some(self.total),
        }
    }

    fn outline(&self) -> Option<Outline> {
        None
    }

    fn page(&self, range: PageRange) -> Result<BlockList, PluginError> {
        let start = range.start as usize;
        if start >= self.blocks.len() {
            return Ok(BlockList {
                blocks: vec![],
                total: Some(self.total),
            });
        }
        let end = (start + range.count as usize).min(self.blocks.len());
        Ok(BlockList {
            blocks: self.blocks[start..end].to_vec(),
            total: Some(self.total),
        })
    }

    fn search(&self, query: &str, from: Cursor) -> Result<SearchHits, PluginError> {
        Ok(search_blocks(&self.blocks, query, from))
    }
}

impl Viewer for DirViewer {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn probe(&self, source: &Source) -> Option<ProbeScore> {
        match source {
            Source::Dir { .. } => Some(ProbeScore {
                score: 60,
                reason: "目录源".into(),
            }),
            Source::Path { path } => {
                if path.is_dir() {
                    // 用户直接给了目录路径：仍可处理，但分低分
                    Some(ProbeScore {
                        score: 30,
                        reason: "路径为目录".into(),
                    })
                } else {
                    // 快速嗅探确认不是我们该抢的：读一个字节都嫌多，直接放弃
                    let _ = std::fs::File::open(path).and_then(|mut f| {
                        let mut b = [0u8; 1];
                        f.read(&mut b)?;
                        Ok(())
                    });
                    None
                }
            }
            Source::Archive { .. } => None,
        }
    }

    fn open(&self, source: &Source, _opts: &OpenOptions) -> Result<Box<dyn Doc>, PluginError> {
        let path = source
            .as_fs_path()
            .ok_or_else(|| PluginError::malformed("dir 插件只支持本地路径源"))?
            .to_path_buf();
        if !path.is_dir() {
            return Err(PluginError::malformed(format!(
                "不是目录：{}",
                path.display()
            )));
        }
        let entries = list_entries(&path)?;
        let blocks = build_blocks(&path, &entries);
        let total = blocks.len() as u64;
        let title = path
            .file_name()
            .map(|n| format!("{}/", n.to_string_lossy()))
            .or_else(|| Some(path.display().to_string()));
        Ok(Box::new(DirDoc {
            viewer_id: self.viewer_id.clone(),
            blocks,
            title,
            total,
        }))
    }
}
