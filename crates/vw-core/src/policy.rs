//! 策略层：路径校验、基线哈希（后台计算）、冲突检测、原子写与备份。
//! 这些是核心统一实施的安全职责——插件永不直接写目标文件（设计规格 §5.4）。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::Mutex;
use std::time::SystemTime;

use vw_protocol::{ErrorKind, PluginError, Source};

/// 打开时的文件基线：`(mtime, size, hash)`。
/// 哈希用 BLAKE3，由后台线程计算，不阻塞首屏；保存时若尚未就绪则等待。
pub struct Baseline {
    path: PathBuf,
    mtime: SystemTime,
    size: u64,
    hash: BaselineHash,
}

enum HashState {
    Pending(Receiver<std::io::Result<String>>),
    Ready(String),
    Failed(String),
}

struct BaselineHash {
    state: Mutex<HashState>,
}

impl BaselineHash {
    fn spawn(path: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(hash_file(&path));
        });
        Self { state: Mutex::new(HashState::Pending(rx)) }
    }

    fn ready(hex: String) -> Self {
        Self { state: Mutex::new(HashState::Ready(hex)) }
    }

    fn get(&self) -> Result<String, PluginError> {
        let taken = {
            let mut st = self.state.lock().unwrap();
            match &*st {
                HashState::Ready(h) => return Ok(h.clone()),
                HashState::Failed(e) => return Err(PluginError::internal(e.clone())),
                HashState::Pending(_) => {
                    std::mem::replace(&mut *st, HashState::Failed("unreachable".into()))
                }
            }
        };
        match taken {
            HashState::Pending(rx) => {
                let res = rx
                    .recv()
                    .map_err(|e| std::io::Error::other(format!("基线哈希线程意外退出：{e}")))
                    .and_then(|r| r);
                let mut st = self.state.lock().unwrap();
                match res {
                    Ok(h) => {
                        *st = HashState::Ready(h.clone());
                        Ok(h)
                    }
                    Err(e) => {
                        let msg = format!("基线哈希不可用：{e}");
                        *st = HashState::Failed(msg.clone());
                        Err(PluginError::internal(msg))
                    }
                }
            }
            _ => Err(PluginError::internal("基线哈希状态错误")),
        }
    }
}

pub fn hash_file(path: &Path) -> std::io::Result<String> {
    let bytes = fs::read(path)?;
    Ok(blake3::hash(&bytes).to_string())
}

pub fn fingerprint(path: &Path) -> std::io::Result<(SystemTime, u64)> {
    let md = fs::metadata(path)?;
    Ok((md.modified()?, md.len()))
}

pub fn capture_baseline(path: PathBuf) -> std::io::Result<Baseline> {
    let (mtime, size) = fingerprint(&path)?;
    Ok(Baseline { mtime, size, hash: BaselineHash::spawn(path.clone()), path })
}

impl Baseline {
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 测试与诊断用：取基线哈希（必要时等待后台线程完成）。
    pub fn hash_hex(&self) -> Result<String, PluginError> {
        self.hash.get()
    }

    /// 保存成功后用新内容直接构建基线（哈希来自刚写入的字节，无需重读）。
    pub fn after_write(path: PathBuf, bytes: &[u8]) -> std::io::Result<Baseline> {
        let (mtime, size) = fingerprint(&path)?;
        Ok(Baseline { mtime, size, hash: BaselineHash::ready(blake3::hash(bytes).to_string()), path })
    }

    /// 冲突检测：比对基线与磁盘现状。内容相同（例如仅被 touch）视为未变。
    pub fn check_unchanged(&self) -> Result<(), PluginError> {
        let (mtime, size) = fingerprint(&self.path).map_err(PluginError::from)?;
        if mtime == self.mtime && size == self.size {
            return Ok(());
        }
        let base_hash = self.hash.get()?;
        let cur_hash = hash_file(&self.path).map_err(PluginError::from)?;
        if base_hash == cur_hash {
            return Ok(());
        }
        Err(PluginError::conflict(format!(
            "文件已在打开后被外部修改，拒绝覆盖保存：{}",
            self.path.display()
        )))
    }
}

/// 校验 `Source`：P1 仅支持存在的本地文件路径。
pub fn validate_source(src: &Source) -> Result<PathBuf, PluginError> {
    match src {
        Source::Path { .. } => {}
        Source::Dir { .. } => {
            return Err(PluginError::new(ErrorKind::Unsupported, "目录查看在 P2 提供"))
        }
        Source::Archive { .. } => {
            return Err(PluginError::new(ErrorKind::Unsupported, "归档成员查看在 P2 提供"))
        }
    }
    let p = src.as_fs_path().expect("已排除非路径源");
    let canon = fs::canonicalize(p).map_err(PluginError::from)?;
    if !canon.is_file() {
        return Err(PluginError::malformed(format!("不是常规文件：{}", canon.display())));
    }
    Ok(canon)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Backup {
    #[default]
    None,
    /// 保存前把原文件复制为 `<原名>.bak`（覆盖旧 .bak）。
    Bak,
    /// 带毫秒时间戳的备份，保留多份。
    Timestamped,
}

#[derive(Debug, Clone)]
pub struct SaveReport {
    pub bytes: u64,
    pub backup: Option<PathBuf>,
}

/// 原子写：同目录临时文件 → fsync → 原子重命名覆盖。
/// 临时文件写入失败或重命名失败都不触碰原文件。
pub fn atomic_write(path: &Path, bytes: &[u8], backup: Backup) -> Result<SaveReport, PluginError> {
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .ok_or_else(|| PluginError::internal(format!("目标路径缺少父目录：{}", path.display())))?;

    let mut report = SaveReport { bytes: bytes.len() as u64, backup: None };
    if backup != Backup::None && path.exists() {
        let mut os = path.as_os_str().to_os_string();
        match backup {
            Backup::Bak => os.push(".bak"),
            Backup::Timestamped => {
                let millis = SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0);
                os.push(format!(".{millis}.bak"));
            }
            Backup::None => unreachable!(),
        }
        let bak = PathBuf::from(os);
        fs::copy(path, &bak).map_err(PluginError::from)?;
        report.backup = Some(bak);
    }

    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    let tmp = dir.join(format!(".{name}.tmp-{}", uuid::Uuid::new_v4()));
    {
        use std::io::Write;
        let mut f = fs::File::create(&tmp).map_err(PluginError::from)?;
        f.write_all(bytes).map_err(|e| {
            let _ = fs::remove_file(&tmp);
            PluginError::from(e)
        })?;
        f.sync_all().map_err(|e| {
            let _ = fs::remove_file(&tmp);
            PluginError::from(e)
        })?;
    }
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        PluginError::from(e)
    })?;
    Ok(report)
}
