//! 内存双工管道：两端各自持有「对端写入缓冲」。
//! 用于 conformance 测试中把 SDK 服务端与宿主客户端接在同一进程内。

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

struct Channel {
    buf: Mutex<VecDeque<u8>>,
    cv: Condvar,
    closed: AtomicBool,
}

impl Channel {
    fn new() -> Self {
        Self { buf: Mutex::new(VecDeque::new()), cv: Condvar::new(), closed: AtomicBool::new(false) }
    }

    fn write(&self, data: &[u8]) -> io::Result<usize> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "管道对端已关闭"));
        }
        self.buf.lock().unwrap().extend(data.iter().copied());
        self.cv.notify_all();
        Ok(data.len())
    }

    fn read(&self, out: &mut [u8]) -> io::Result<usize> {
        let mut guard = self.buf.lock().unwrap();
        loop {
            if !guard.is_empty() {
                let n = out.len().min(guard.len());
                for slot in out.iter_mut().take(n) {
                    *slot = guard.pop_front().unwrap();
                }
                return Ok(n);
            }
            if self.closed.load(Ordering::SeqCst) {
                return Ok(0);
            }
            guard = self.cv.wait(guard).unwrap();
        }
    }

    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.cv.notify_all();
    }
}

/// 双工端：写入进入对端的读取缓冲，反之亦然。`Clone` 共享同一端。
#[derive(Clone)]
pub struct PipeEnd {
    /// 我写、对端读。
    write_ch: Arc<Channel>,
    /// 对端写、我读。
    read_ch: Arc<Channel>,
}

impl PipeEnd {
    /// 关闭本端写侧：对端的 `read` 将得到 EOF。
    pub fn close_write(&self) {
        self.write_ch.close();
    }
}

impl Read for PipeEnd {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.read_ch.read(buf)
    }
}

impl Write for PipeEnd {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write_ch.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// 建立一对连通的内存管道。
pub fn mempipe() -> (PipeEnd, PipeEnd) {
    let a_to_b = Arc::new(Channel::new());
    let b_to_a = Arc::new(Channel::new());
    let a = PipeEnd { write_ch: a_to_b.clone(), read_ch: b_to_a.clone() };
    let b = PipeEnd { write_ch: b_to_a, read_ch: a_to_b };
    (a, b)
}
