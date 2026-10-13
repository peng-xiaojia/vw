//! vw 内置插件：归档（zip）列表与成员物化（设计规格 §5.1）。
//!
//! - 打开 `.zip` → 成员清单表格；
//! - `Source::Archive` / `materialize()` → 把成员解到会话临时目录，
//!   核心随后按普通文件重新探测打开。
//! - zip 炸弹防护：成员数、总解压量、单成员解压量三重上限（§5.1/§12.3）。

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use vw_core::viewer::{ArchiveSource, Doc, Viewer};
use vw_protocol::{
    search_blocks, Align, Block, BlockList, Cursor, DocMeta, Inline, Manifest, OpenOptions, Outline, PageRange,
    ProbeScore, PluginError, SearchHits, Source,
};
use zip::ZipArchive;

pub const DEFAULT_MEMBER_COUNT_CAP: usize = 10_000;
pub const DEFAULT_TOTAL_UNCOMPRESSED_CAP: u64 = 1024 * 1024 * 1024; // 1 GB
pub const DEFAULT_MEMBER_UNCOMPRESSED_CAP: u64 = 512 * 1024 * 1024; // 512 MB

const ROWS_PER_BLOCK: usize = 500;
const SNIFF_BYTES: usize = 4;

pub struct ArchiveViewer {
    manifest: Manifest,
    viewer_id: String,
    member_count_cap: usize,
    total_cap: u64,
    member_cap: u64,
}

impl ArchiveViewer {
    pub fn new(priority: i64) -> Self {
        Self {
            manifest: Manifest::simple_in_process("vw.archive", "archive", &["zip"], &["view"], priority),
            viewer_id: "archive".into(),
            member_count_cap: DEFAULT_MEMBER_COUNT_CAP,
            total_cap: DEFAULT_TOTAL_UNCOMPRESSED_CAP,
            member_cap: DEFAULT_MEMBER_UNCOMPRESSED_CAP,
        }
    }

    /// 测试用：收紧上限。
    pub fn with_caps(priority: i64, member_count_cap: usize, total_cap: u64, member_cap: u64) -> Self {
        let mut v = Self::new(priority);
        v.member_count_cap = member_count_cap;
        v.total_cap = total_cap;
        v.member_cap = member_cap;
        v
    }

    fn open_zip(path: &Path) -> Result<ZipArchive<std::io::BufReader<std::fs::File>>, PluginError> {
        let f = std::fs::File::open(path).map_err(PluginError::from)?;
        ZipArchive::new(std::io::BufReader::new(f))
            .map_err(|e| PluginError::malformed(format!("无法读取归档：{e}")))
    }

    /// 读取全部成员元信息（执行成员数与总量上限检查）。
    fn members(&self, path: &Path) -> Result<Vec<MemberInfo>, PluginError> {
        let mut za = Self::open_zip(path)?;
        if za.len() > self.member_count_cap {
            return Err(PluginError::malformed(format!(
                "归档成员数 {} 超过上限 {}（zip 炸弹防护）",
                za.len(),
                self.member_count_cap
            )));
        }
        let mut out = Vec::new();
        let mut total: u64 = 0;
        for i in 0..za.len() {
            let f = za.by_index(i).map_err(|e| PluginError::malformed(format!("成员读取失败：{e}")))?;
            let size = f.size();
            total += size;
            if total > self.total_cap {
                return Err(PluginError::malformed(format!(
                    "归档总解压量超过上限 {} 字节（zip 炸弹防护）",
                    self.total_cap
                )));
            }
            out.push(MemberInfo {
                name: f.name().to_string(),
                size,
                compressed: f.compressed_size(),
                is_dir: f.is_dir(),
                modified: f.last_modified().map(|d| d.to_string()).unwrap_or_default(),
            });
        }
        Ok(out)
    }
}

struct MemberInfo {
    name: String,
    size: u64,
    compressed: u64,
    is_dir: bool,
    modified: String,
}

impl ArchiveSource for ArchiveViewer {
    /// 物化单个成员到 `dest_dir`。文件名取成员末段；重名时以内容哈希前缀消解。
    fn materialize(&self, outer: &Path, inner: &str, dest_dir: &Path) -> Result<PathBuf, PluginError> {
        let mut za = Self::open_zip(outer)?;
        let mut zf = za
            .by_name(inner)
            .map_err(|_| PluginError::malformed(format!("归档内不存在成员：{inner}")))?;
        if zf.size() > self.member_cap {
            return Err(PluginError::malformed(format!(
                "成员解压后 {} 字节，超过单成员上限 {}（zip 炸弹防护）",
                zf.size(),
                self.member_cap
            )));
        }
        let base = inner
            .rsplit(['/', '\\'])
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or("member")
            .to_string();
        let mut dest = dest_dir.join(&base);
        if dest.exists() {
            let hash = blake3::hash(inner.as_bytes());
            let hex = hash.to_string();
            dest = dest_dir.join(format!("{}-{base}", &hex[..8]));
        }
        let mut out = std::fs::File::create(&dest).map_err(PluginError::from)?;
        let mut buf = [0u8; 65_536];
        let mut written: u64 = 0;
        loop {
            let n = zf.read(&mut buf).map_err(PluginError::from)?;
            if n == 0 {
                break;
            }
            written += n as u64;
            if written > self.member_cap {
                let _ = std::fs::remove_file(&dest);
                return Err(PluginError::malformed("成员实际解压量超过上限（zip 炸弹防护）"));
            }
            out.write_all(&buf[..n]).map_err(PluginError::from)?;
        }
        out.sync_all().map_err(PluginError::from)?;
        Ok(dest)
    }
}

struct ArchiveDoc {
    viewer_id: String,
    blocks: Vec<Block>,
    title: Option<String>,
    total: u64,
    path: PathBuf,
    #[allow(dead_code)] // 预留给 GUI 详情页展示防护上限
    caps: (usize, u64, u64),
}

impl Doc for ArchiveDoc {
    fn meta(&self) -> DocMeta {
        DocMeta {
            viewer: self.viewer_id.clone(),
            mime: Some("application/zip".into()),
            size: std::fs::metadata(&self.path).ok().map(|m| m.len()),
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
            return Ok(BlockList { blocks: vec![], total: Some(self.total) });
        }
        let end = (start + range.count as usize).min(self.blocks.len());
        Ok(BlockList { blocks: self.blocks[start..end].to_vec(), total: Some(self.total) })
    }

    fn search(&self, query: &str, from: Cursor) -> Result<SearchHits, PluginError> {
        Ok(search_blocks(&self.blocks, query, from))
    }
}

impl Viewer for ArchiveViewer {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn as_archive(&self) -> Option<&dyn ArchiveSource> {
        Some(self)
    }

    fn probe(&self, source: &Source) -> Option<ProbeScore> {
        match source {
            Source::Archive { .. } => Some(ProbeScore { score: 70, reason: "归档成员源".into() }),
            Source::Dir { .. } => None,
            Source::Path { path } => {
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if ext.eq_ignore_ascii_case("zip") {
                        return Some(ProbeScore { score: 75, reason: "扩展名匹配".into() });
                    }
                }
                let mut f = std::fs::File::open(path).ok()?;
                let mut head = [0u8; SNIFF_BYTES];
                let n = std::io::Read::read(&mut f, &mut head).ok()?;
                if &head[..n] == b"PK\x03\x04" {
                    Some(ProbeScore { score: 80, reason: "魔数 PK\\x03\\x04".into() })
                } else {
                    None
                }
            }
        }
    }

    fn open(&self, source: &Source, _opts: &OpenOptions) -> Result<Box<dyn Doc>, PluginError> {
        let path = source
            .as_fs_path()
            .ok_or_else(|| PluginError::malformed("archive 插件只支持本地路径源"))?
            .to_path_buf();
        let members = self.members(&path)?;
        let head = vec![
            Inline::text("成员"),
            Inline::text("大小"),
            Inline::text("压缩后"),
            Inline::text("修改时间"),
        ];
        let mut rows: Vec<Vec<Inline>> = members
            .iter()
            .map(|m| {
                vec![
                    Inline::text(m.name.clone()),
                    Inline::text(if m.is_dir { "目录".into() } else { m.size.to_string() }),
                    Inline::text(if m.is_dir { "-".into() } else { m.compressed.to_string() }),
                    Inline::text(m.modified.clone()),
                ]
            })
            .collect();
        let mut blocks = Vec::new();
        if rows.is_empty() {
            rows.push(vec![Inline::text("（空归档）")]);
        }
        for chunk in rows.chunks(ROWS_PER_BLOCK) {
            blocks.push(Block::Table {
                head: Some(head.clone()),
                rows: chunk.to_vec(),
                align: vec![Align::Left, Align::Right, Align::Right, Align::Left],
            });
        }
        let total = blocks.len() as u64;
        let title = path.file_name().map(|n| n.to_string_lossy().to_string());
        Ok(Box::new(ArchiveDoc {
            viewer_id: self.viewer_id.clone(),
            blocks,
            title,
            total,
            path,
            caps: (self.member_count_cap, self.total_cap, self.member_cap),
        }))
    }
}
