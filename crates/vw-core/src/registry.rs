//! 插件注册表、探测调度与仲裁（设计规格 §7.2）。

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;

use vw_protocol::{PluginError, ProbeScore, Source};

use crate::viewer::Viewer;

/// 对外的 viewer id：manifest 中第一个 contributes.viewer 的 id。
pub fn viewer_id_of(v: &Arc<dyn Viewer>) -> String {
    v.manifest()
        .contributes
        .viewers
        .first()
        .map(|d| d.id.clone())
        .unwrap_or_else(|| v.manifest().id.clone())
}

#[derive(Clone)]
struct CachedScore {
    viewer_id: String,
    score: u8,
    reason: String,
}

pub struct Registry {
    viewers: Vec<Arc<dyn Viewer>>,
    /// 探测缓存，键为 `(规范化路径, mtime, size)`（设计规格 §6.2）。
    probe_cache: Mutex<HashMap<(PathBuf, u128, u64), Vec<CachedScore>>>,
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    pub fn new() -> Self {
        Self { viewers: Vec::new(), probe_cache: Mutex::new(HashMap::new()) }
    }

    pub fn register(&mut self, v: Arc<dyn Viewer>) {
        self.viewers.push(v);
    }

    pub fn viewers(&self) -> &[Arc<dyn Viewer>] {
        &self.viewers
    }

    /// 按 viewer id（contributes 里的 id）或插件 id 查找。
    pub fn find_viewer(&self, id: &str) -> Option<Arc<dyn Viewer>> {
        self.viewers
            .iter()
            .find(|v| {
                v.manifest().id == id
                    || v.manifest().contributes.viewers.iter().any(|d| d.id == id)
            })
            .cloned()
    }

    /// 对所有插件探测打分，按 (分数, priority, id) 降序排序。
    pub fn probe_all(&self, src: &Source) -> Vec<(Arc<dyn Viewer>, ProbeScore)> {
        if let Some(cached) = self.cached_scores(src) {
            return cached
                .into_iter()
                .filter_map(|c| {
                    self.find_viewer(&c.viewer_id)
                        .map(|v| (v, ProbeScore { score: c.score, reason: c.reason }))
                })
                .collect();
        }

        let mut out = Vec::new();
        for v in &self.viewers {
            if let Some(s) = v.probe(src) {
                out.push((v.clone(), s));
            }
        }
        out.sort_by(|a, b| {
            b.1.score
                .cmp(&a.1.score)
                .then_with(|| b.0.manifest().priority.cmp(&a.0.manifest().priority))
                .then_with(|| a.0.manifest().id.cmp(&b.0.manifest().id))
        });

        if let Some(key) = cache_key(src) {
            let cached = out
                .iter()
                .map(|(v, s)| CachedScore { viewer_id: viewer_id_of(v), score: s.score, reason: s.reason.clone() })
                .collect();
            self.probe_cache.lock().unwrap().insert(key, cached);
        }
        out
    }

    /// 仲裁：`--as` 指定优先 → 最高分胜出 → 同分比 priority → 仍并列则报错列出候选。
    pub fn select(
        &self,
        src: &Source,
        as_viewer: Option<&str>,
    ) -> Result<Arc<dyn Viewer>, PluginError> {
        if let Some(id) = as_viewer {
            return self.find_viewer(id).ok_or_else(|| {
                PluginError::no_viewer(format!("没有 id 为 {id:?} 的插件（可用 vw probe 查看候选）"))
            });
        }

        let scored = self.probe_all(src);
        let Some((best, bs)) = scored.first() else {
            return Err(PluginError::no_viewer(format!(
                "没有插件认领 {}。请安装对应插件，或用 --as <viewer-id> 指定。",
                src.display()
            )));
        };
        if let Some((second, ss)) = scored.get(1) {
            if ss.score == bs.score && second.manifest().priority == best.manifest().priority {
                let list = scored
                    .iter()
                    .map(|(v, s)| format!("{}({})", viewer_id_of(v), s.score))
                    .collect::<Vec<_>>()
                    .join("、");
                return Err(PluginError::ambiguous(format!(
                    "多个插件并列认领 {}：{list}。请用 --as <viewer-id> 指定。",
                    src.display()
                )));
            }
        }
        Ok(best.clone())
    }

    fn cached_scores(&self, src: &Source) -> Option<Vec<CachedScore>> {
        let key = cache_key(src)?;
        self.probe_cache.lock().unwrap().get(&key).cloned()
    }
}

fn cache_key(src: &Source) -> Option<(PathBuf, u128, u64)> {
    let p = src.as_fs_path()?;
    let md = fs::metadata(p).ok()?;
    let mtime = md.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_nanos();
    Some((p.to_path_buf(), mtime, md.len()))
}
