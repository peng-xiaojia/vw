//! 插件 manifest（设计规格 §7.3）。内置与外置插件使用同一 schema。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TransportKind {
    InProcess,
    StdioJsonrpc,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transport {
    pub kind: TransportKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub framing: Option<String>,
    /// 仅 stdio 插件需要；支持 `{platform}` 与 `{ext}` 占位符。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewerDecl {
    pub id: String,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub mime: Vec<String>,
    /// `view` 与 `edit`；未声明 `edit` 即只读。
    #[serde(default)]
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Contributes {
    #[serde(default)]
    pub viewers: Vec<ViewerDecl>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolSection {
    #[serde(default = "default_contract")]
    pub contract: u32,
}

impl Default for ProtocolSection {
    fn default() -> Self {
        Self { contract: default_contract() }
    }
}

fn default_contract() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnginesSection {
    #[serde(default = "default_engines")]
    pub vw: String,
}

impl Default for EnginesSection {
    fn default() -> Self {
        Self { vw: default_engines() }
    }
}

fn default_engines() -> String {
    ">=0.1.0".into()
}

fn default_priority() -> i64 {
    50
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub manifest_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub protocol: ProtocolSection,
    #[serde(default)]
    pub engines: EnginesSection,
    pub transport: Transport,
    #[serde(default)]
    pub contributes: Contributes,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default = "default_priority")]
    pub priority: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

impl Manifest {
    /// 测试与内置插件用的最小 in-process manifest 构造器。
    pub fn simple_in_process(
        plugin_id: &str,
        viewer_id: &str,
        extensions: &[&str],
        capabilities: &[&str],
        priority: i64,
    ) -> Manifest {
        serde_json::from_value(serde_json::json!({
            "manifest_version": 1,
            "id": plugin_id,
            "name": plugin_id,
            "version": "0.1.0",
            "transport": { "kind": "in-process" },
            "contributes": { "viewers": [ {
                "id": viewer_id,
                "extensions": extensions,
                "capabilities": capabilities,
            } ] },
            "priority": priority,
        }))
        .expect("内置 manifest 构造错误")
    }

    /// 测试与目录式安装用的最小 stdio manifest 构造器。
    pub fn simple_stdio(
        plugin_id: &str,
        viewer_id: &str,
        extensions: &[&str],
        capabilities: &[&str],
        priority: i64,
        executable: &str,
    ) -> Manifest {
        serde_json::from_value(serde_json::json!({
            "manifest_version": 1,
            "id": plugin_id,
            "name": plugin_id,
            "version": "0.1.0",
            "transport": { "kind": "stdio-jsonrpc", "framing": "jsonl", "executable": executable },
            "contributes": { "viewers": [ {
                "id": viewer_id,
                "extensions": extensions,
                "capabilities": capabilities,
            } ] },
            "priority": priority,
        }))
        .expect("内置 manifest 构造错误")
    }

    /// 某个 viewer 声明的能力（"view" / "edit"）。
    pub fn capabilities_of(&self, viewer_id: &str) -> Vec<String> {
        self.contributes
            .viewers
            .iter()
            .find(|v| v.id == viewer_id)
            .map(|v| v.capabilities.clone())
            .unwrap_or_default()
    }

    pub fn declares_edit(&self, viewer_id: &str) -> bool {
        self.capabilities_of(viewer_id).iter().any(|c| c == "edit")
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.manifest_version != 1 {
            return Err(format!("不支持的 manifest_version：{}", self.manifest_version));
        }
        if self.protocol.contract != 1 {
            return Err(format!("不支持的协议契约版本：{}", self.protocol.contract));
        }
        match self.transport.kind {
            TransportKind::InProcess => {}
            TransportKind::StdioJsonrpc => {
                if self.transport.executable.is_none() {
                    return Err("stdio 插件必须声明 transport.executable".into());
                }
            }
        }
        if self.contributes.viewers.is_empty() {
            return Err("manifest 未声明任何 viewer".into());
        }
        for v in &self.contributes.viewers {
            if v.id.is_empty() {
                return Err("viewer id 不能为空".into());
            }
            for c in &v.capabilities {
                if c != "view" && c != "edit" {
                    return Err(format!("未知能力：{c}（只允许 view / edit）"));
                }
            }
        }
        Ok(())
    }
}
