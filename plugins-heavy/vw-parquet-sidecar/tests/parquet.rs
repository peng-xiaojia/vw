//! P5 端到端：构造真实 Parquet 文件 → spawn sidecar 子进程 → HostViewer 打开。

use std::sync::Arc;

use parquet::basic::{Compression, Repetition, Type as PhysicalType};
use parquet::column::writer::ColumnWriter;
use parquet::file::properties::WriterProperties;
use parquet::file::writer::SerializedFileWriter;
use parquet::schema::types::Type;
use vw_core::{Registry, Session};
use vw_plugin_host::{HostConfig, HostViewer};
use vw_protocol::{Manifest, OpenOptions, Source};

fn make_parquet(path: &std::path::Path) {
    let schema = Arc::new(
        Type::group_type_builder("schema")
            .with_fields(vec![
                Arc::new(
                    Type::primitive_type_builder("name", PhysicalType::BYTE_ARRAY)
                        .with_repetition(Repetition::REQUIRED)
                        .build()
                        .unwrap(),
                ),
                Arc::new(
                    Type::primitive_type_builder("age", PhysicalType::INT32)
                        .with_repetition(Repetition::REQUIRED)
                        .build()
                        .unwrap(),
                ),
            ])
            .build()
            .unwrap(),
    );
    let props = WriterProperties::builder().set_compression(Compression::SNAPPY).build();
    let f = std::fs::File::create(path).unwrap();
    let mut w = SerializedFileWriter::new(f, schema, props.into()).unwrap();
    let mut rg = w.next_row_group().unwrap();
    // 列式写入：先整列 name，再整列 age
    if let Some(mut col) = rg.next_column().unwrap() {
        col.typed::<parquet::data_type::ByteArrayType>()
            .write_batch(
                &[
                    parquet::data_type::ByteArray::from("alice"),
                    parquet::data_type::ByteArray::from("bob"),
                ],
                Some(&[1, 1]),
                None,
            )
            .unwrap();
        col.close().unwrap();
    }
    if let Some(mut col) = rg.next_column().unwrap() {
        col.typed::<parquet::data_type::Int32Type>()
            .write_batch(&[30, 25], Some(&[1, 1]), None)
            .unwrap();
        col.close().unwrap();
    }
    rg.close().unwrap();
    w.close().unwrap();
}

#[test]
fn parquet_sidecar_end_to_end() {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("p5");
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("people.parquet");
    make_parquet(&f);

    vw_core::set_tmp_override(Some(std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("p5-tmp")));
    let exe = env!("CARGO_BIN_EXE_vw-parquet-sidecar");
    let manifest = Manifest::simple_stdio(
        "dev.vw.parquet",
        "parquet",
        &["parquet"],
        &["view"],
        60,
        exe,
    );
    let host = HostViewer::from_manifest(manifest, HostConfig::default()).unwrap();
    let mut reg = Registry::new();
    reg.register(Arc::new(host));
    let session = Session::new(Arc::new(reg));

    // 魔数 probe（无扩展名也能认）
    assert!(session.registry().probe_all(&Source::path(&f)).iter().any(|(v, s)| {
        vw_core::registry::viewer_id_of(v) == "parquet" && s.score >= 85
    }));

    // 新写盘的文件可能被 AV 实时扫描短暂锁住（os error 5）：带退避重试
    let doc = {
        let mut out: Option<Result<vw_core::OpenedDoc, vw_protocol::PluginError>> = None;
        for _ in 0..20 {
            match session.open(Source::path(&f), OpenOptions::default()) {
                Ok(d) => {
                    out = Some(Ok(d));
                    break;
                }
                Err(e) if e.kind == vw_protocol::ErrorKind::PermissionDenied => {
                    out = Some(Err(e));
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                Err(e) => {
                    out = Some(Err(e));
                    break;
                }
            }
        }
        match out {
            Some(Ok(d)) => d,
            Some(Err(e)) => panic!("open 失败：{e}"),
            None => panic!("open 未执行"),
        }
    };
    assert_eq!(doc.viewer_id, "parquet");
    assert_eq!(doc.meta.mime.as_deref(), Some("application/x-parquet"));
    let all = doc.page_all(100).unwrap();
    let text = all.blocks.iter().filter_map(vw_protocol::block_text).collect::<Vec<_>>().join("\n");
    assert!(text.contains("alice") && text.contains("30"), "{text}");
    assert!(text.contains("name") && text.contains("age"), "{text}");
    // 只读
    assert!(doc.edit_capability().is_none());
}
