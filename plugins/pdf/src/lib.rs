use vw_core::viewer::{Doc, Viewer};
use vw_protocol::{Manifest, OpenOptions, PluginError, ProbeScore, Source};

pub struct PdfViewer { manifest: Manifest }
impl PdfViewer {
    pub fn new(priority: i64) -> Self { Self { manifest: Manifest::simple_in_process("vw.pdf", "pdf", &["pdf"], &["view"], priority) } }
}
impl Viewer for PdfViewer {
    fn manifest(&self) -> &Manifest { &self.manifest }
    fn probe(&self, source: &Source) -> Option<ProbeScore> {
        if !cfg!(windows) { return None; }
        let path = source.as_fs_path()?;
        if path.extension()?.to_str()?.eq_ignore_ascii_case("pdf") { Some(ProbeScore { score: 90, reason: "PDF 扩展名".into() }) } else { None }
    }
    fn open(&self, source: &Source, _opts: &OpenOptions) -> Result<Box<dyn Doc>, PluginError> {
        #[cfg(windows)] { native::open(source) }
        #[cfg(not(windows))] { let _ = source; Err(PluginError::unsupported("PDF 本地渲染需要 Windows")) }
    }
}

#[cfg(windows)]
mod native {
    use std::path::PathBuf;
    use std::sync::Mutex;
    use windows::core::{PCWSTR, AgileReference};
    use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
    use windows::Storage::Streams::{InMemoryRandomAccessStream, IRandomAccessStream, DataReader};
    use windows::Win32::System::WinRT::{CreateRandomAccessStreamOnFile, RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};
    use vw_core::{SessionTempDir, viewer::Doc};
    use vw_protocol::{BlobRef, Block, BlockList, Cursor, DocMeta, Outline, OutlineItem, PageRange, PluginError, SearchHits, Source};

    fn error(e: windows::core::Error) -> PluginError { PluginError::malformed(format!("PDF 渲染失败：{e}")) }
    struct Apartment(bool);
    impl Apartment {
        fn new() -> Result<Self, PluginError> {
            match unsafe { RoInitialize(RO_INIT_MULTITHREADED) } {
                Ok(()) => Ok(Self(true)),
                Err(e) if e.code().0 as u32 == 0x80010106 => Ok(Self(false)),
                Err(e) => Err(error(e)),
            }
        }
    }
    impl Drop for Apartment { fn drop(&mut self) { if self.0 { unsafe { RoUninitialize(); } } } }

    pub fn open(source: &Source) -> Result<Box<dyn Doc>, PluginError> {
        let _apartment = Apartment::new()?;
        let path = source.as_fs_path().ok_or_else(|| PluginError::malformed("PDF 仅支持文件路径"))?;
        let size = std::fs::metadata(path)?.len();
        if size > 512 * 1024 * 1024 { return Err(PluginError::unsupported("PDF 文件超过 512 MiB 上限")); }
        let canonical = std::fs::canonicalize(path)?;
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = canonical.as_os_str().encode_wide().chain(Some(0)).collect();
        // Win32 file stream preserves desktop path access; StorageFile uses brokered
        // capabilities and rejects ordinary desktop paths in some managed environments.
        let stream: IRandomAccessStream = unsafe { CreateRandomAccessStreamOnFile(PCWSTR(wide.as_ptr()), 0) }.map_err(error)?;
        let pdf = PdfDocument::LoadFromStreamAsync(&stream).map_err(error)?.get().map_err(error)?;
        let pages = pdf.PageCount().map_err(error)?;
        if pages == 0 || pages > 10_000 { return Err(PluginError::unsupported("PDF 页数必须在 1–10000 之间")); }
        let root = SessionTempDir::new()?;
        let title = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let input = AgileReference::new(&stream).map_err(error)?;
        Ok(Box::new(PdfDoc { pdf, _input: input, pages, title, size, root, render: Mutex::new(()) }))
    }

    struct PdfDoc { pdf: PdfDocument, _input: AgileReference<IRandomAccessStream>, pages: u32, title: String, size: u64, root: SessionTempDir, render: Mutex<()> }
    impl Doc for PdfDoc {
        fn meta(&self) -> DocMeta { DocMeta { viewer: "pdf".into(), mime: Some("application/pdf".into()), size: Some(self.size), title: Some(self.title.clone()), pages: Some(u64::from(self.pages)) } }
        fn outline(&self) -> Option<Outline> { Some((0..self.pages).map(|page| OutlineItem { level: 1, text: format!("第 {} 页", page + 1), block: u64::from(page) }).collect()) }
        fn page(&self, range: PageRange) -> Result<BlockList, PluginError> {
            let end = range.start.saturating_add(u64::from(range.count)).min(u64::from(self.pages));
            let blocks = (range.start..end).map(|page| Block::Image { blob: format!("page-{page}"), alt: format!("{} · 第 {} 页", self.title, page + 1), w: None, h: None }).collect();
            Ok(BlockList { blocks, total: Some(u64::from(self.pages)) })
        }
        fn search(&self, _: &str, _: Cursor) -> Result<SearchHits, PluginError> { Err(PluginError::unsupported("PDF 当前为逐页图像预览，暂不支持文本搜索")) }
        fn blob_root(&self) -> Option<PathBuf> { Some(self.root.path().to_path_buf()) }
        fn blob(&self, id: &str) -> Result<BlobRef, PluginError> {
            let page_index = id.strip_prefix("page-").and_then(|n| n.parse::<u32>().ok()).filter(|&n| n < self.pages).ok_or_else(|| PluginError::permission("无效 PDF 页面资源"))?;
            if id != format!("page-{page_index}") { return Err(PluginError::permission("无效 PDF 页面资源")); }
            let _lock = self.render.lock().map_err(|_| PluginError::internal("PDF 渲染锁不可用"))?;
            let path = self.root.path().join(format!("{id}.png"));
            if !path.exists() {
                let _apartment = Apartment::new()?;
                let page = self.pdf.GetPage(page_index).map_err(error)?;
                let size = page.Size().map_err(error)?;
                if !size.Width.is_finite() || !size.Height.is_finite() || size.Width <= 0.0 || size.Height <= 0.0 { return Err(PluginError::malformed("PDF 页面尺寸无效")); }
                let scale = (1440.0 / size.Width.max(size.Height)).min(2.0);
                let options = PdfPageRenderOptions::new().map_err(error)?;
                options.SetDestinationWidth((size.Width * scale).ceil().max(1.0) as u32).map_err(error)?;
                options.SetDestinationHeight((size.Height * scale).ceil().max(1.0) as u32).map_err(error)?;
                // PNG encoder GUID: no active document content reaches the WebView.
                options.SetBitmapEncoderId(windows::core::GUID::from_u128(0x27949969_876a_41d7_9447_568f6a35a4dc)).map_err(error)?;
                let stream = InMemoryRandomAccessStream::new().map_err(error)?;
                let rendered = page.RenderWithOptionsToStreamAsync(&stream, &options).map_err(|e| PluginError::malformed(format!("PDF render start: {e}")))?.get();
                let _ = page.Close();
                rendered.map_err(|e| PluginError::malformed(format!("PDF render wait: {e}")))?;
                let length = stream.Size().map_err(error)?;
                if length == 0 || length > 16 * 1024 * 1024 { return Err(PluginError::unsupported("PDF 页面资源超过 16 MiB")); }
                let input = stream.GetInputStreamAt(0).map_err(error)?;
                let reader = DataReader::CreateDataReader(&input).map_err(error)?;
                let loaded = reader.LoadAsync(length as u32).map_err(error)?.get().map_err(|e| PluginError::malformed(format!("PDF rendered PNG read: {e}")))?;
                if loaded != length as u32 { return Err(PluginError::malformed("PDF 页面资源截断")); }
                let mut png = vec![0; length as usize]; reader.ReadBytes(&mut png).map_err(error)?;
                if !png.starts_with(b"\x89PNG\r\n\x1a\n") { return Err(PluginError::malformed("PDF 渲染未返回 PNG")); }
                std::fs::write(&path, png)?;
            }
            Ok(BlobRef { path: format!("{id}.png"), mime: Some("image/png".into()), size: Some(std::fs::metadata(path)?.len()) })
        }
    }
}
