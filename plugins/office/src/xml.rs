use crate::damaged;
use quick_xml::{
    events::{BytesStart, Event},
    Reader, XmlVersion,
};
use vw_protocol::PluginError;

const MAX_NODES: usize = 200_000;
const MAX_DEPTH: usize = 128;
#[derive(Default)]
pub(crate) struct Node {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
    pub text: String,
}
impl Node {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(n, _)| n == name)
            .or_else(|| {
                self.attrs
                    .iter()
                    .find(|(n, _)| n.rsplit(':').next() == Some(name))
            })
            .map(|(_, v)| v.as_str())
    }
    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|n| n.name == name)
    }
    pub fn descendants<'a>(&'a self, name: &str, output: &mut Vec<&'a Node>) {
        for child in &self.children {
            if child.name == name {
                output.push(child);
            }
            child.descendants(name, output);
        }
    }
    pub fn all<'a>(&'a self, name: &str) -> Vec<&'a Node> {
        let mut out = Vec::new();
        self.descendants(name, &mut out);
        out
    }
}
fn local(name: &str) -> String {
    name.rsplit(':').next().unwrap_or("").into()
}
fn node(start: &BytesStart<'_>) -> Result<Node, PluginError> {
    let mut node = Node {
        name: local(start.name().as_ref()),
        ..Node::default()
    };
    for attr in start.attributes() {
        let attr = attr.map_err(damaged)?;
        node.attrs.push((
            attr.key.as_ref().to_owned(),
            attr.normalized_value(XmlVersion::Implicit1_0)
                .map_err(damaged)?
                .into_owned(),
        ));
    }
    Ok(node)
}
pub(crate) fn parse(bytes: &[u8]) -> Result<Node, PluginError> {
    let text = std::str::from_utf8(bytes).map_err(|_| damaged("XML 必须使用 UTF-8 编码"))?;
    let mut reader = Reader::from_str(text.trim_start_matches('\u{feff}'));
    let mut stack = vec![Node::default()];
    let mut count = 0usize;
    loop {
        match reader.read_event().map_err(damaged)? {
            Event::Start(start) => {
                count += 1;
                if count > MAX_NODES || stack.len() > MAX_DEPTH {
                    return Err(damaged("XML 节点数或嵌套深度超过上限"));
                }
                stack.push(node(&start)?);
            }
            Event::Empty(start) => {
                count += 1;
                if count > MAX_NODES {
                    return Err(damaged("XML 节点数超过上限"));
                }
                stack.last_mut().unwrap().children.push(node(&start)?);
            }
            Event::End(_) => {
                if stack.len() < 2 {
                    return Err(damaged("XML 结束标签无效"));
                }
                let node = stack.pop().unwrap();
                stack.last_mut().unwrap().children.push(node);
            }
            Event::Text(text) => stack
                .last_mut()
                .unwrap()
                .text
                .push_str(&text.xml10_content()),
            Event::CData(text) => stack
                .last_mut()
                .unwrap()
                .text
                .push_str(&text.xml10_content()),
            Event::GeneralRef(reference) => {
                let entity = format!("&{};", reference.as_ref());
                stack
                    .last_mut()
                    .unwrap()
                    .text
                    .push_str(&quick_xml::escape::unescape(&entity).map_err(damaged)?);
            }
            Event::DocType(_) => return Err(damaged("不支持 DTD 或外部实体")),
            Event::Eof => break,
            _ => {}
        }
    }
    if stack.len() != 1 {
        return Err(damaged("XML 标签未闭合"));
    }
    let mut root = stack.pop().unwrap();
    if root.children.len() != 1 || !root.text.trim().is_empty() {
        return Err(damaged("XML 根节点无效"));
    }
    Ok(root.children.pop().unwrap())
}
pub(crate) fn runs(node: &Node) -> String {
    if node.name == "t" {
        return node.text.clone();
    }
    if node.name == "tab" {
        return "\t".into();
    }
    if matches!(node.name.as_str(), "br" | "cr") {
        return "\n".into();
    }
    node.children.iter().map(runs).collect()
}
