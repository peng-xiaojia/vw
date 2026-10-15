use std::{io::Write, path::PathBuf};
use vw_core::viewer::Viewer;
use vw_plugin_office::OfficeViewer;
use vw_protocol::{run_text, Block, Cursor, OpenOptions, PageRange, Source};
use zip::{write::SimpleFileOptions, ZipWriter};

fn fixture(name: &str, parts: &[(&str, &str)]) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("office");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    let mut zip = ZipWriter::new(std::fs::File::create(&path).unwrap());
    for (part, xml) in parts {
        zip.start_file(*part, SimpleFileOptions::default()).unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
    path
}
fn blocks(path: PathBuf) -> Vec<Block> {
    OfficeViewer::new(70)
        .open(&Source::path(path), &OpenOptions::default())
        .unwrap()
        .page(PageRange {
            start: 0,
            count: 500,
        })
        .unwrap()
        .blocks
}

#[test]
fn word_paragraphs_headings_tables_entities_and_search() {
    let path = fixture(
        "text.docx",
        &[(
            "word/document.xml",
            r#"<w:document xmlns:w="urn:w"><w:body><w:p><w:pPr><w:pStyle w:val="Heading2"/></w:pPr><w:r><w:t>标题</w:t></w:r></w:p><w:p><w:r><w:t>Hello &amp; </w:t></w:r><w:r><w:t>世界 &#x1F600;</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>cell A</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>cell B</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#,
        )],
    );
    let viewer = OfficeViewer::new(70);
    assert_eq!(viewer.probe(&Source::path(&path)).unwrap().score, 95);
    let doc = viewer
        .open(&Source::path(&path), &OpenOptions::default())
        .unwrap();
    assert!(doc.as_editable().is_none());
    let page = doc
        .page(PageRange {
            start: 0,
            count: 50,
        })
        .unwrap();
    assert!(
        matches!(&page.blocks[0], Block::Heading { level: 2, text } if run_text(text) == "标题")
    );
    assert!(matches!(&page.blocks[1], Block::Para { text } if run_text(text) == "Hello & 世界 😀"));
    assert!(
        matches!(&page.blocks[2], Block::Table { rows, .. } if rows.len() == 1 && rows[0].len() == 2)
    );
    assert_eq!(doc.search("世界", Cursor::start()).unwrap().hits.len(), 1);
    assert_eq!(doc.outline().unwrap()[0].text, "标题");
    assert!(doc
        .page(PageRange {
            start: u64::MAX,
            count: 1
        })
        .unwrap()
        .blocks
        .is_empty());
}

#[test]
fn workbook_sheet_order_shared_inline_strings_sparse_rows_and_cached_formula() {
    let path = fixture(
        "cells.xlsx",
        &[
            (
                "xl/workbook.xml",
                r#"<workbook xmlns:r="urn:r"><sheets><sheet name="第二 &amp; 页" sheetId="9" r:id="rB"/><sheet name="First" sheetId="1" r:id="rA"/></sheets></workbook>"#,
            ),
            (
                "xl/_rels/workbook.xml.rels",
                r#"<Relationships><Relationship Id="rA" Target="worksheets/sheet1.xml"/><Relationship Id="rB" Target="worksheets/sheet9.xml"/></Relationships>"#,
            ),
            (
                "xl/sharedStrings.xml",
                r#"<sst><si><r><t>Shared </t></r><r><t>世界</t></r></si></sst>"#,
            ),
            (
                "xl/worksheets/sheet9.xml",
                r#"<worksheet><sheetData><row r="2"><c r="B2" t="s"><v>0</v></c><c r="D2"><f>1+2</f><v>3</v></c></row><row r="4"><c r="A4" t="inlineStr"><is><t>inline</t></is></c><c r="C4" t="b"><v>1</v></c></row></sheetData></worksheet>"#,
            ),
            (
                "xl/worksheets/sheet1.xml",
                r#"<worksheet><sheetData><row><c><v>42</v></c></row></sheetData></worksheet>"#,
            ),
        ],
    );
    let parsed = blocks(path);
    assert!(matches!(&parsed[0], Block::Heading { text, .. } if run_text(text) == "第二 & 页"));
    let Block::Table { head, rows, .. } = &parsed[1] else {
        panic!("sheet table")
    };
    assert_eq!(run_text(head.as_ref().unwrap()), "行ABCD");
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[1][2], vw_protocol::Inline::text("Shared 世界"));
    assert_eq!(rows[1][4], vw_protocol::Inline::text("3"));
    assert_eq!(rows[3][1], vw_protocol::Inline::text("inline"));
    assert_eq!(rows[3][3], vw_protocol::Inline::text("TRUE"));
    assert!(matches!(&parsed[2], Block::Heading { text, .. } if run_text(text) == "First"));
}

#[test]
fn presentation_uses_numeric_slide_order_and_text() {
    let slide = r#"<p:sld xmlns:p="urn:p" xmlns:a="urn:a"><p:cSld><p:spTree><p:sp><p:nvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>SLIDE</a:t></a:r></a:p></p:txBody></p:sp><p:sp><p:txBody><a:p><a:r><a:t>Body &amp; text</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#;
    let ten = slide.replace("SLIDE", "Ten");
    let two = slide.replace("SLIDE", "Two");
    let path = fixture(
        "slides.pptx",
        &[
            ("ppt/slides/slide10.xml", &ten),
            ("ppt/slides/slide2.xml", &two),
        ],
    );
    let parsed = blocks(path);
    assert!(matches!(&parsed[0], Block::Heading { text, .. } if run_text(text).contains("Two")));
    assert!(matches!(&parsed[1], Block::Para { text } if run_text(text) == "Body & text"));
    assert!(matches!(&parsed[3], Block::Heading { text, .. } if run_text(text).contains("Ten")));
}

#[test]
fn damaged_documents_and_unsupported_legacy_extensions_are_clear_errors() {
    let viewer = OfficeViewer::new(70);
    let broken = fixture("broken.docx", &[("word/document.xml", "<document><body>")]);
    assert!(viewer
        .open(&Source::path(broken), &OpenOptions::default())
        .is_err());
    assert!(viewer.probe(&Source::path("legacy.doc")).is_none());
}

#[test]
fn bounded_archive_and_xml_policies_reject_disallowed_input() {
    use vw_plugin_office::OfficeLimits;
    let viewer = OfficeViewer::with_limits(
        70,
        OfficeLimits {
            members: 2,
            total_bytes: 100,
            xml_bytes: 60,
        },
    );
    let large = fixture(
        "limit.docx",
        &[(
            "word/document.xml",
            &format!("<document><body>{}</body></document>", "x".repeat(70)),
        )],
    );
    assert!(viewer
        .open(&Source::path(large), &OpenOptions::default())
        .is_err());
    let many = fixture(
        "many.docx",
        &[
            ("word/document.xml", "<document/>"),
            ("one", "1"),
            ("two", "2"),
        ],
    );
    assert!(viewer
        .open(&Source::path(many), &OpenOptions::default())
        .is_err());
    let dtd = fixture(
        "dtd.docx",
        &[(
            "word/document.xml",
            "<!DOCTYPE document><document><body/></document>",
        )],
    );
    assert!(viewer
        .open(&Source::path(dtd), &OpenOptions::default())
        .is_err());
    let invalid = fixture(
        "path.docx",
        &[
            ("word/document.xml", "<document><body/></document>"),
            ("../outside", "x"),
        ],
    );
    assert!(viewer
        .open(&Source::path(invalid), &OpenOptions::default())
        .is_err());
}

#[test]
fn presentation_saved_order_uses_relationship_id_regardless_of_attribute_order() {
    let path = fixture(
        "ordered.pptx",
        &[
            (
                "ppt/presentation.xml",
                r#"<p:presentation xmlns:p="urn:p" xmlns:r="urn:r"><p:sldIdLst><p:sldId r:id="rTen" id="256"/><p:sldId id="257" r:id="rTwo"/></p:sldIdLst></p:presentation>"#,
            ),
            (
                "ppt/_rels/presentation.xml.rels",
                r#"<Relationships><Relationship Id="rTwo" Target="slides/slide2.xml"/><Relationship Id="rTen" Target="slides/slide10.xml"/></Relationships>"#,
            ),
            (
                "ppt/slides/slide2.xml",
                r#"<sld><sp><txBody><p><r><t>Two</t></r></p></txBody></sp></sld>"#,
            ),
            (
                "ppt/slides/slide10.xml",
                r#"<sld><sp><txBody><p><r><t>Ten</t></r></p></txBody></sp></sld>"#,
            ),
        ],
    );
    let parsed = blocks(path);
    assert!(matches!(&parsed[1], Block::Para { text } if run_text(text) == "Ten"));
    assert!(matches!(&parsed[4], Block::Para { text } if run_text(text) == "Two"));
}

#[test]
fn workbook_cells_can_arrive_out_of_column_order_without_losing_values() {
    let path = fixture(
        "unordered.xlsx",
        &[
            (
                "xl/workbook.xml",
                r#"<workbook xmlns:r="urn:r"><sheets><sheet name="Sheet" r:id="rA"/></sheets></workbook>"#,
            ),
            (
                "xl/_rels/workbook.xml.rels",
                r#"<Relationships><Relationship Id="rA" Target="worksheets/sheet1.xml"/></Relationships>"#,
            ),
            (
                "xl/worksheets/sheet1.xml",
                r#"<worksheet><sheetData><row r="1"><c r="D1"><v>4</v></c><c r="A1"><v>1</v></c></row></sheetData></worksheet>"#,
            ),
        ],
    );
    let parsed = blocks(path);
    let Block::Table { rows, .. } = &parsed[1] else {
        panic!("table")
    };
    assert_eq!(rows[0][1], vw_protocol::Inline::text("1"));
    assert_eq!(rows[0][4], vw_protocol::Inline::text("4"));
}

#[test]
fn shared_strings_cannot_expand_the_preview_beyond_its_text_budget() {
    let strings = format!("<sst><si><t>{}</t></si></sst>", "x".repeat(1024 * 1024));
    let cells = (0..17)
        .map(|_| r#"<c t="s"><v>0</v></c>"#)
        .collect::<String>();
    let sheet = format!("<worksheet><sheetData><row>{cells}</row></sheetData></worksheet>");
    let path = fixture(
        "amplified.xlsx",
        &[
            (
                "xl/workbook.xml",
                r#"<workbook xmlns:r="urn:r"><sheets><sheet name="Sheet" r:id="rA"/></sheets></workbook>"#,
            ),
            (
                "xl/_rels/workbook.xml.rels",
                r#"<Relationships><Relationship Id="rA" Target="worksheets/sheet1.xml"/></Relationships>"#,
            ),
            ("xl/sharedStrings.xml", &strings),
            ("xl/worksheets/sheet1.xml", &sheet),
        ],
    );
    assert!(OfficeViewer::new(70)
        .open(&Source::path(path), &OpenOptions::default())
        .is_err());
}

#[test]
fn full_500_by_128_sheet_fits_the_documented_preview_limits() {
    let path = fixture("full-boundary.xlsx", &[
        ("xl/workbook.xml", r#"<workbook xmlns:r="urn:r"><sheets><sheet name="Sheet" r:id="rA"/></sheets></workbook>"#),
        ("xl/_rels/workbook.xml.rels", r#"<Relationships><Relationship Id="rA" Target="worksheets/sheet1.xml"/></Relationships>"#),
        ("xl/worksheets/sheet1.xml", r#"<worksheet><sheetData><row r="500"><c r="DX500"><v>128</v></c></row></sheetData></worksheet>"#),
    ]);
    let parsed = blocks(path);
    let Block::Table { rows, .. } = &parsed[1] else { panic!("table") };
    assert_eq!(rows.len(), 500);
    assert_eq!(rows[499].len(), 129);
    assert_eq!(rows[499][128], vw_protocol::Inline::text("128"));
}
