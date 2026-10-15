use crate::{
    damaged,
    xml::{self, Node},
    OfficeLimits,
};
use std::{
    collections::HashSet,
    fs::File,
    io::{BufReader, Read},
    path::Path,
};
use vw_protocol::PluginError;
use zip::ZipArchive;

pub(crate) struct Package {
    archive: ZipArchive<BufReader<File>>,
    names: HashSet<String>,
    limits: OfficeLimits,
    read_bytes: u64,
}
pub(crate) fn valid_path(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('/')
        && !name.contains(['\\', ':', '\0'])
        && name
            .trim_end_matches('/')
            .split('/')
            .all(|s| !s.is_empty() && s != "." && s != "..")
}
impl Package {
    pub fn open(path: &Path, limits: OfficeLimits) -> Result<Self, PluginError> {
        if std::fs::metadata(path)?.len() > limits.total_bytes {
            return Err(damaged("文件大小超过上限"));
        }
        let mut archive = ZipArchive::new(BufReader::new(File::open(path)?)).map_err(damaged)?;
        if archive.len() > limits.members {
            return Err(damaged("归档成员数超过上限"));
        }
        let mut total = 0u64;
        let mut names = HashSet::new();
        for index in 0..archive.len() {
            let member = archive.by_index(index).map_err(damaged)?;
            if !valid_path(member.name()) || !names.insert(member.name().to_owned()) {
                return Err(damaged("归档成员路径无效或重复"));
            }
            total = total
                .checked_add(member.size())
                .ok_or_else(|| damaged("归档大小溢出"))?;
            if total > limits.total_bytes {
                return Err(damaged("归档总解压量超过上限"));
            }
            if (member.name().ends_with(".xml") || member.name().ends_with(".rels"))
                && member.size() > limits.xml_bytes
            {
                return Err(damaged("XML 成员超过上限"));
            }
        }
        Ok(Self {
            archive,
            names,
            limits,
            read_bytes: 0,
        })
    }
    pub fn contains(&self, name: &str) -> bool {
        self.names.contains(name)
    }
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.names.iter().map(String::as_str)
    }
    pub fn xml(&mut self, name: &str) -> Result<Node, PluginError> {
        if !valid_path(name) {
            return Err(damaged("XML 路径无效"));
        }
        let member = self
            .archive
            .by_name(name)
            .map_err(|e| damaged(format!("缺少或无法读取 {name}：{e}")))?;
        if member.size() > self.limits.xml_bytes {
            return Err(damaged("XML 成员超过上限"));
        }
        let mut bytes = Vec::new();
        member
            .take(self.limits.xml_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(damaged)?;
        self.read_bytes = self
            .read_bytes
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| damaged("读取大小溢出"))?;
        if bytes.len() as u64 > self.limits.xml_bytes || self.read_bytes > self.limits.total_bytes {
            return Err(damaged("XML 解压量超过上限"));
        }
        xml::parse(&bytes).map_err(|e| damaged(format!("{name}：{}", e.message)))
    }
}
pub(crate) fn resolve(base: &str, target: &str) -> Result<String, PluginError> {
    let path = if let Some(abs) = target.strip_prefix('/') {
        abs.to_owned()
    } else {
        format!("{base}/{target}")
    };
    if !valid_path(&path) {
        return Err(damaged("文档内部关系路径无效"));
    }
    Ok(path)
}
