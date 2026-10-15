use std::{path::PathBuf, sync::Arc};
use vw_core::{
    viewer::{Doc, Viewer},
    Registry,
};
use vw_protocol::*;

struct ResourceViewer {
    manifest: Manifest,
    root: PathBuf,
    reference: String,
    mime: String,
}
struct ResourceDoc {
    root: PathBuf,
    reference: String,
    mime: String,
}
impl Viewer for ResourceViewer {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    fn probe(&self, _: &Source) -> Option<ProbeScore> {
        Some(ProbeScore {
            score: 100,
            reason: "test".into(),
        })
    }
    fn open(&self, _: &Source, _: &OpenOptions) -> Result<Box<dyn Doc>, PluginError> {
        Ok(Box::new(ResourceDoc {
            root: self.root.clone(),
            reference: self.reference.clone(),
            mime: self.mime.clone(),
        }))
    }
}
impl Doc for ResourceDoc {
    fn meta(&self) -> DocMeta {
        DocMeta {
            viewer: "resource-test".into(),
            mime: None,
            size: None,
            title: None,
            pages: None,
        }
    }
    fn outline(&self) -> Option<Outline> {
        None
    }
    fn page(&self, _: PageRange) -> Result<BlockList, PluginError> {
        Ok(BlockList {
            blocks: vec![],
            total: Some(0),
        })
    }
    fn search(&self, _: &str, _: Cursor) -> Result<SearchHits, PluginError> {
        Ok(SearchHits {
            hits: vec![],
            next: None,
        })
    }
    fn blob_root(&self) -> Option<PathBuf> {
        Some(self.root.clone())
    }
    fn blob(&self, _: &str) -> Result<BlobRef, PluginError> {
        Ok(BlobRef {
            path: self.reference.clone(),
            mime: Some(self.mime.clone()),
            size: None,
        })
    }
}

#[test]
fn controlled_blob_rejects_escape_svg_large_and_closed_handles() {
    let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("app-blob-security");
    vw_core::set_tmp_override(Some(tmp.clone()));
    let root = tmp.join("sess-resource-test");
    std::fs::create_dir_all(&root).unwrap();
    let source = root.join("source.test");
    std::fs::write(&source, b"test").unwrap();
    std::fs::write(root.join("image.png"), b"\x89PNG\r\n\x1a\n").unwrap();
    std::fs::write(tmp.join("outside.png"), b"private").unwrap();
    let open = |reference: &str, mime: &str| {
        let mut registry = Registry::new();
        registry.register(Arc::new(ResourceViewer {
            manifest: Manifest::simple_in_process(
                "test.resource",
                "resource-test",
                &[],
                &["view"],
                90,
            ),
            root: root.clone(),
            reference: reference.into(),
            mime: mime.into(),
        }));
        let state = vw_app::AppState::with_registry(registry);
        let info = state
            .open_impl(source.to_str().unwrap(), None, None)
            .unwrap();
        (state, info.handle)
    };
    let (state, handle) = open("image.png", "image/png");
    assert_eq!(state.blob_impl(&handle, "image").unwrap().0, "image/png");
    state.close_impl(&handle).unwrap();
    assert!(state.blob_impl(&handle, "image").is_err());
    for reference in ["../outside.png", source.to_str().unwrap()] {
        let (state, handle) = open(reference, "image/png");
        assert!(state.blob_impl(&handle, "image").is_err());
    }
    let (state, handle) = open("image.png", "image/svg+xml");
    assert!(state.blob_impl(&handle, "image").is_err());
    std::fs::File::create(root.join("huge.png"))
        .unwrap()
        .set_len(64 * 1024 * 1024 + 1)
        .unwrap();
    let (state, handle) = open("huge.png", "image/png");
    assert!(state.blob_impl(&handle, "image").is_err());
}
