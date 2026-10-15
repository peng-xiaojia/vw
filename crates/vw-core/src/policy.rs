//! 策略层：路径校验、基线哈希（后台计算）、冲突检测、原子写与备份。
//! 这些是核心统一实施的安全职责——插件永不直接写目标文件（设计规格 §5.4）。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::Mutex;
use std::time::SystemTime;

use vw_protocol::{ErrorKind, PluginError, Source};

/// 打开时的文件基线：`(mtime, size, hash)`。
/// 哈希用 BLAKE3，由后台线程以「稳定快照」方式计算（前后两次 fingerprint
/// 一致才采纳），不阻塞首屏；保存时若尚未就绪则等待。
pub struct Baseline {
    path: PathBuf,
    snap: BaselineSnap,
}

/// 一个内容快照。
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub mtime: SystemTime,
    pub size: u64,
    pub hash: String,
}

enum HashState {
    Pending(Receiver<std::io::Result<Snapshot>>),
    Ready(Snapshot),
    Failed(String),
}

struct BaselineSnap {
    state: Mutex<HashState>,
}

/// 哈希期间文件未再变化才算有效基线（防止把篡改后的内容当基线）。
fn hash_stable(path: &Path) -> std::io::Result<Snapshot> {
    for _ in 0..8 {
        let (mtime, size) = fingerprint(path)?;
        let hash = hash_file(path)?;
        let (mtime2, size2) = fingerprint(path)?;
        if mtime == mtime2 && size == size2 {
            return Ok(Snapshot { mtime, size, hash });
        }
    }
    Err(std::io::Error::other("文件持续变化，无法建立基线"))
}

impl BaselineSnap {
    fn spawn(path: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(hash_stable(&path));
        });
        Self {
            state: Mutex::new(HashState::Pending(rx)),
        }
    }

    fn ready(snap: Snapshot) -> Self {
        Self {
            state: Mutex::new(HashState::Ready(snap)),
        }
    }

    fn get(&self) -> Result<Snapshot, PluginError> {
        let mut st = self.state.lock().unwrap();
        match &*st {
            HashState::Ready(s) => return Ok(s.clone()),
            HashState::Failed(e) => return Err(PluginError::internal(e.clone())),
            HashState::Pending(_) => {}
        }
        let taken = std::mem::replace(&mut *st, HashState::Failed("基线哈希不可用".into()));
        match taken {
            HashState::Pending(rx) => {
                let res = rx
                    .recv()
                    .map_err(|e| std::io::Error::other(format!("基线哈希线程意外退出：{e}")))
                    .and_then(|r| r);
                match res {
                    Ok(s) => {
                        *st = HashState::Ready(s.clone());
                        Ok(s)
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
    use std::io::Read;
    let mut file = fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hasher.finalize().to_string())
}

pub fn fingerprint(path: &Path) -> std::io::Result<(SystemTime, u64)> {
    let md = fs::metadata(path)?;
    Ok((md.modified()?, md.len()))
}

pub fn capture_baseline(path: PathBuf) -> std::io::Result<Baseline> {
    let snap = BaselineSnap::spawn(path.clone());
    Ok(Baseline { path, snap })
}

/// 小文件阈值：open 返回前等待基线就绪，保证基线确定性地对应打开时的内容。
pub const BASELINE_WAIT_LIMIT: u64 = 4 * 1024 * 1024;

impl Baseline {
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 取基线快照（必要时等待后台线程完成）。
    pub fn snapshot(&self) -> Result<Snapshot, PluginError> {
        self.snap.get()
    }

    /// 诊断/测试用：等待并返回基线哈希。
    pub fn hash_hex(&self) -> Result<String, PluginError> {
        Ok(self.snap.get()?.hash)
    }

    /// 保存成功后用新内容直接构建基线（哈希来自刚写入的字节，无需重读）。
    pub fn after_write(path: PathBuf, bytes: &[u8]) -> std::io::Result<Baseline> {
        let (mtime, size) = fingerprint(&path)?;
        let hash = blake3::hash(bytes).to_string();
        Ok(Baseline {
            path,
            snap: BaselineSnap::ready(Snapshot { mtime, size, hash }),
        })
    }

    /// 冲突检测：比对基线与磁盘现状。内容相同（例如仅被 touch）视为未变。
    pub fn check_unchanged(&self) -> Result<(), PluginError> {
        let snap = self.snap.get()?;
        // Equal metadata does not prove equal contents (copy tools may preserve timestamps).
        let cur_hash = hash_file(&self.path).map_err(PluginError::from)?;
        if cur_hash == snap.hash {
            return Ok(());
        }
        Err(PluginError::conflict(format!(
            "文件已在打开后被外部修改，拒绝覆盖保存：{}",
            self.path.display()
        )))
    }
}

/// 校验 `Source`：本地文件或目录（目录查看为 P2 能力）。
pub fn validate_source(src: &Source) -> Result<PathBuf, PluginError> {
    match src {
        Source::Path { .. } | Source::Dir { .. } => {}
        Source::Archive { .. } => {
            return Err(PluginError::new(
                ErrorKind::Unsupported,
                "归档源需经 open_member 物化后打开（见 session::open_member）",
            ))
        }
    }
    let p = src.as_fs_path().expect("已排除非路径源");
    let canon = fs::canonicalize(p).map_err(PluginError::from)?;
    match src {
        Source::Dir { .. } => {
            if !canon.is_dir() {
                return Err(PluginError::malformed(format!(
                    "不是目录：{}",
                    canon.display()
                )));
            }
        }
        _ => {
            if !canon.is_file() {
                return Err(PluginError::malformed(format!(
                    "不是常规文件：{}",
                    canon.display()
                )));
            }
        }
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

    let mut report = SaveReport {
        bytes: bytes.len() as u64,
        backup: None,
    };
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pending_baseline_supports_concurrent_waiters() {
        let (tx, rx) = mpsc::channel();
        let snap = std::sync::Arc::new(BaselineSnap {
            state: Mutex::new(HashState::Pending(rx)),
        });
        let other = snap.clone();
        let first = std::thread::spawn(move || other.get());
        std::thread::sleep(std::time::Duration::from_millis(30));
        let (done_tx, done_rx) = mpsc::channel();
        let second = std::thread::spawn(move || {
            let result = snap.get();
            done_tx.send(()).unwrap();
            result
        });
        let early = done_rx
            .recv_timeout(std::time::Duration::from_millis(100))
            .is_ok();
        tx.send(Ok(Snapshot {
            mtime: SystemTime::now(),
            size: 1,
            hash: "same".into(),
        }))
        .unwrap();
        assert_eq!(first.join().unwrap().unwrap().hash, "same");
        assert_eq!(second.join().unwrap().unwrap().hash, "same");
        assert!(!early, "all pending readers must wait for the hash");
    }
}
