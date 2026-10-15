//! Raster decoder isolated behind the viewer contract; GUI only consumes Image IR.
use std::io::{BufReader, Cursor as IoCursor, Read};
use std::path::PathBuf;
use image::{GenericImageView, ImageFormat, ImageReader, Limits};
use vw_core::viewer::{Doc, Viewer};
use vw_core::SessionTempDir;
use vw_protocol::{BlobRef, Block, BlockList, Cursor, DocMeta, Manifest, OpenOptions, PageRange, PluginError, ProbeScore, SearchHits, Source};

const MAX_SOURCE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PIXELS: u64 = 16_000_000;

pub struct ImageViewer { manifest: Manifest }

impl ImageViewer {
    pub fn new(priority: i64) -> Self {
        Self { manifest: Manifest::simple_in_process("vw.image", "image", &["png", "jpg", "jpeg", "gif", "webp", "bmp"], &["view"], priority) }
    }
}

fn supported(format: ImageFormat) -> bool {
    matches!(format, ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif | ImageFormat::WebP | ImageFormat::Bmp)
}

fn malformed(e: impl std::fmt::Display) -> PluginError { PluginError::malformed(format!("无法解码图片：{e}")) }

impl Viewer for ImageViewer {
    fn manifest(&self) -> &Manifest { &self.manifest }
    fn probe(&self, source: &Source) -> Option<ProbeScore> {
        let path = source.as_fs_path()?;
        if !path.is_file() { return None; }
        let mut header = [0; 32];
        let n = std::fs::File::open(path).ok()?.read(&mut header).ok()?;
        if image::guess_format(&header[..n]).ok().is_some_and(supported) {
            Some(ProbeScore { score: 80, reason: "光栅图片文件头".into() })
        } else if path.extension().and_then(|s| s.to_str()).is_some_and(|ext| ["png", "jpg", "jpeg", "gif", "webp", "bmp"].iter().any(|e| ext.eq_ignore_ascii_case(e))) {
            Some(ProbeScore { score: 75, reason: "图片扩展名".into() })
        } else { None }
    }
    fn open(&self, source: &Source, _opts: &OpenOptions) -> Result<Box<dyn Doc>, PluginError> {
        let path = source.as_fs_path().ok_or_else(|| PluginError::malformed("图片插件只支持本地文件"))?;
        let size = std::fs::metadata(path)?.len();
        if size > MAX_SOURCE_BYTES { return Err(PluginError::malformed("图片超过 64 MiB 上限")); }
        let mut reader = ImageReader::new(BufReader::new(std::fs::File::open(path)?)).with_guessed_format().map_err(malformed)?;
        if !reader.format().is_some_and(supported) { return Err(PluginError::unsupported("不支持的图片格式")); }
        let original_format = reader.format().unwrap();
        let mut limits = Limits::default();
        limits.max_image_width = Some(16_000);
        limits.max_image_height = Some(16_000);
        limits.max_alloc = Some(MAX_PIXELS * 4);
        reader.limits(limits);
        let dimensions = ImageReader::open(path).map_err(malformed)?.with_guessed_format().map_err(malformed)?.into_dimensions().map_err(malformed)?;
        if dimensions.0 == 0 || dimensions.1 == 0 || u64::from(dimensions.0) * u64::from(dimensions.1) > MAX_PIXELS { return Err(PluginError::malformed("图片尺寸超过 1600 万像素上限")); }
        let decoded = reader.decode().map_err(malformed)?;
        let (w, h) = decoded.dimensions();
        let mut png = IoCursor::new(Vec::new());
        decoded.write_to(&mut png, ImageFormat::Png).map_err(malformed)?;
        let bytes = png.into_inner();
        if bytes.len() as u64 > MAX_SOURCE_BYTES { return Err(PluginError::malformed("图片预览资源超过 64 MiB 上限")); }
        let root = SessionTempDir::new()?;
        std::fs::write(root.path().join("image.png"), &bytes)?;
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let alt = if original_format == ImageFormat::Gif { format!("{name}（GIF 首帧）") } else { name.clone() };
        Ok(Box::new(ImageDoc { root, title: name, source_size: size, blob_size: bytes.len() as u64, block: Block::Image { blob: "image".into(), alt, w: Some(w), h: Some(h) } }))
    }
}

struct ImageDoc { root: SessionTempDir, title: String, source_size: u64, blob_size: u64, block: Block }
impl Doc for ImageDoc {
    fn meta(&self) -> DocMeta { DocMeta { viewer: "image".into(), mime: Some("image/png".into()), size: Some(self.source_size), title: Some(self.title.clone()), pages: Some(1) } }
    fn page(&self, range: PageRange) -> Result<BlockList, PluginError> { Ok(BlockList { blocks: if range.start == 0 && range.count > 0 { vec![self.block.clone()] } else { vec![] }, total: Some(1) }) }
    fn search(&self, _query: &str, _from: Cursor) -> Result<SearchHits, PluginError> { Ok(SearchHits { hits: vec![], next: None }) }
    fn blob(&self, id: &str) -> Result<BlobRef, PluginError> {
        if id != "image" { return Err(PluginError::permission("未知图片资源")); }
        Ok(BlobRef { path: "image.png".into(), mime: Some("image/png".into()), size: Some(self.blob_size) })
    }
    fn blob_root(&self) -> Option<PathBuf> { Some(self.root.path().to_path_buf()) }
}
