//! 宿主侧连接：JSON Lines 请求/响应、乱序匹配、超时与取消、崩溃检测。
//! 支持两种线缆：真实子进程与内存管道（conformance 测试用）。

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::process::Child;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use vw_protocol::{PluginError, Request, Response};
use vw_plugin_sdk::PipeEnd;

pub(crate) struct Conn {
    stdin: Mutex<Box<dyn Write + Send>>,
    next_id: AtomicU64,
    pending: Arc<Mutex<HashMap<u64, mpsc::SyncSender<Result<serde_json::Value, PluginError>>>>>,
    crashed: Arc<AtomicBool>,
    kill: Box<dyn Fn() + Send + Sync>,
    pub stderr_tail: Arc<Mutex<VecDeque<String>>>,
    /// 读取线程句柄（Conn 析构时随进程退出，不显式 join）
    _reader: Mutex<Option<std::thread::JoinHandle<()>>>,
    _stderr: Option<std::thread::JoinHandle<()>>,
}

impl Conn {
    /// 接入一条线缆。`kill` 必须让对端最终关闭输出（read 得到 EOF）。
    pub fn connect(
        stdin: Box<dyn Write + Send>,
        stdout: Box<dyn BufRead + Send>,
        stderr: Option<Box<dyn BufRead + Send>>,
        kill: Box<dyn Fn() + Send + Sync>,
        log: Option<std::fs::File>,
    ) -> Arc<Conn> {
        let pending: Arc<Mutex<HashMap<u64, mpsc::SyncSender<Result<serde_json::Value, PluginError>>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let crashed = Arc::new(AtomicBool::new(false));
        let stderr_tail: Arc<Mutex<VecDeque<String>>> = Arc::new(Mutex::new(VecDeque::new()));

        let reader = {
            let pending = pending.clone();
            let crashed = crashed.clone();
            std::thread::spawn(move || {
                let mut lines = stdout.lines();
                while let Some(Ok(line)) = lines.next() {
                    if line.trim().is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<Response>(&line) {
                        Ok(resp) => {
                            if let Some(tx) = pending.lock().unwrap().remove(&resp.id) {
                                let payload = match (resp.result, resp.error) {
                                    (Some(v), None) => Ok(v),
                                    (None, Some(e)) => Err(e),
                                    (None, None) => Ok(serde_json::Value::Null),
                                    (Some(_), Some(_)) => {
                                        Err(PluginError::internal("响应同时含 result 与 error"))
                                    }
                                };
                                let _ = tx.send(payload);
                            }
                            // 未知 id 的响应：静默丢弃（可能是超时后的迟到响应）
                        }
                        Err(_) => {
                            // 一行坏消息不终止连接；崩溃检测交给 EOF
                        }
                    }
                }
                // 对端 EOF：标记崩溃并清空全部在途请求
                crashed.store(true, Ordering::SeqCst);
                for (_, tx) in pending.lock().unwrap().drain() {
                    let _ = tx.send(Err(PluginError::crashed("插件进程已退出")));
                }
            })
        };

        let stderr_thread = stderr.map(|mut err| {
            let tail = stderr_tail.clone();
            let mut log = log;
            std::thread::spawn(move || {
                let mut line = String::new();
                loop {
                    line.clear();
                    match err.read_line(&mut line) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            {
                                let mut t = tail.lock().unwrap();
                                if t.len() >= 200 {
                                    t.pop_front();
                                }
                                t.push_back(line.trim_end().to_string());
                            }
                            // §9：同步落盘，供 vw doctor 与事后排查
                            if let Some(f) = log.as_mut() {
                                let _ = f.write_all(line.as_bytes());
                            }
                        }
                    }
                }
            })
        });

        Arc::new(Conn {
            stdin: Mutex::new(stdin),
            next_id: AtomicU64::new(1),
            pending,
            crashed,
            kill,
            stderr_tail,
            _reader: Mutex::new(Some(reader)),
            _stderr: stderr_thread,
        })
    }

    /// 从子进程接入（接管 Child 所有权，kill 闭包持有它）。
    /// `log`：插件 stderr 落盘文件（§9 诊断；None = 仅内存尾部）。
    pub fn connect_process(mut child: Child, log: Option<std::fs::File>) -> Result<Arc<Conn>, PluginError> {
        let stdin = child.stdin.take().ok_or_else(|| PluginError::internal("无法接管子进程 stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| PluginError::internal("无法接管子进程 stdout"))?;
        let stderr = child.stderr.take().ok_or_else(|| PluginError::internal("无法接管子进程 stderr"))?;
        let child = Arc::new(Mutex::new(child));
        let killer = child.clone();
        let kill: Box<dyn Fn() + Send + Sync> = Box::new(move || {
            let _ = killer.lock().unwrap().kill();
            let _ = killer.lock().unwrap().wait();
        });
        Ok(Self::connect(
            Box::new(stdin),
            Box::new(BufReader::new(stdout)),
            Some(Box::new(BufReader::new(stderr))),
            kill,
            log,
        ))
    }

    /// 从内存管道接入（对端跑 vw_plugin_sdk::serve）。
    pub fn connect_memory(pipe: PipeEnd, kill: Box<dyn Fn() + Send + Sync>) -> Arc<Conn> {
        let read_half = pipe.clone();
        Self::connect(
            Box::new(pipe),
            Box::new(BufReader::new(read_half)),
            None,
            kill,
            None,
        )
    }

    pub fn is_crashed(&self) -> bool {
        self.crashed.load(Ordering::SeqCst)
    }

    pub fn request(
        &self,
        method: &str,
        params: serde_json::Value,
        timeout: Duration,
    ) -> Result<serde_json::Value, PluginError> {
        if self.is_crashed() {
            return Err(PluginError::crashed("插件进程已崩溃，等待重启"));
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::sync_channel(1);
        self.pending.lock().unwrap().insert(id, tx);

        let line = serde_json::to_string(&Request { id, method: method.into(), params })?;
        {
            let mut w = self.stdin.lock().unwrap();
            if let Err(e) = w.write_all(line.as_bytes()).and_then(|_| w.write_all(b"\n")).and_then(|_| w.flush()) {
                self.pending.lock().unwrap().remove(&id);
                self.crashed.store(true, Ordering::SeqCst);
                return Err(PluginError::crashed(format!("向插件写入请求失败：{e}")));
            }
        }

        match rx.recv_timeout(timeout) {
            Ok(v) => v,
            Err(RecvTimeoutError::Timeout) => {
                // 先礼后兵：发 cancel，等宽限期，仍无响应则强杀（设计规格 §9）
                let cid = self.next_id.fetch_add(1, Ordering::SeqCst);
                let (ctx, _crx) = mpsc::sync_channel(1);
                self.pending.lock().unwrap().insert(cid, ctx);
                let cline = serde_json::to_string(&Request {
                    id: cid,
                    method: "cancel".into(),
                    params: serde_json::json!({ "target": id }),
                })
                .unwrap_or_default();
                let mut w = self.stdin.lock().unwrap();
                let _ = w.write_all(cline.as_bytes());
                let _ = w.write_all(b"\n");
                let _ = w.flush();
                drop(w);

                match rx.recv_timeout(Duration::from_secs(1)) {
                    Ok(v) => v,
                    Err(_) => {
                        (self.kill)();
                        self.pending.lock().unwrap().remove(&cid);
                        Err(PluginError::timeout(format!("请求 {method} 超时（{timeout:?}），已取消并重启插件进程")))
                    }
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                Err(PluginError::crashed("插件进程在请求处理中退出"))
            }
        }
    }
}
