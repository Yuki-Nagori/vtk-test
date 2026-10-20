//! 有界后台执行；领域流程、取消判断和提交裁决仍属于调用方。
use std::collections::VecDeque;
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};

type Work = Box<dyn FnOnce() + Send>;
struct Job {
    work: Work,
    failed: Option<Work>,
}

#[derive(Default)]
struct State {
    queue: VecDeque<Job>,
    workers: usize,
    outstanding: usize,
    closed: bool,
}

struct Shared {
    name: &'static str,
    workers: usize,
    capacity: usize,
    state: Mutex<State>,
    idle: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn start_worker(shared: Arc<Shared>) -> io::Result<()> {
    std::thread::Builder::new()
        .name(shared.name.into())
        .spawn(move || run_worker(shared))
        .map(drop)
}

/// 在途容量包含执行中和排队的作业；拒绝不阻塞调用线程。
/// 空闲 worker 自动退出，不保存已结束的 JoinHandle。
pub(crate) struct Executor {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for Executor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Executor")
            .field("name", &self.shared.name)
            .field("workers", &self.shared.workers)
            .field("capacity", &self.shared.capacity)
            .finish_non_exhaustive()
    }
}

impl Executor {
    pub(crate) fn new(name: &'static str, workers: usize, capacity: usize) -> Arc<Self> {
        Arc::new(Self {
            shared: Arc::new(Shared {
                name,
                workers,
                capacity,
                state: Mutex::new(State::default()),
                idle: Condvar::new(),
            }),
        })
    }

    /// 无额外终态回调的作业；执行体持有的 sender 在 unwind 时自动断开。
    pub(crate) fn submit(&self, work: impl FnOnce() + Send + 'static) -> io::Result<()> {
        self.submit_with(
            Job {
                work: Box::new(work),
                failed: None,
            },
            start_worker,
        )
    }

    /// failed 只在执行体 panic 时调用，负责领域终态。
    /// 拒绝 / 启动失败时尚未执行，由调用方回滚准入。
    pub(crate) fn submit_reported(
        &self,
        work: impl FnOnce() + Send + 'static,
        failed: impl FnOnce() + Send + 'static,
    ) -> io::Result<()> {
        self.submit_with(
            Job {
                work: Box::new(work),
                failed: Some(Box::new(failed)),
            },
            start_worker,
        )
    }

    // 启动器可注入，确定性验证 OS 拒绝启动时的账目回滚。
    fn submit_with(&self, job: Job, start: fn(Arc<Shared>) -> io::Result<()>) -> io::Result<()> {
        let mut state = self.shared.lock();
        if state.closed {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "background executor closed",
            ));
        }
        if state.outstanding >= self.shared.capacity {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "background capacity exhausted",
            ));
        }
        state.queue.push_back(job);
        state.outstanding += 1;
        if state.workers < self.shared.workers {
            let shared = Arc::clone(&self.shared);
            state.workers += 1;
            if let Err(error) = start(shared) {
                state.workers -= 1;
                state.outstanding -= 1;
                state.queue.pop_back();
                return Err(error);
            }
        }
        Ok(())
    }

    /// 仅用于宿主事件循环退出后的收尾；禁止在本通道 worker 中等待自己。
    /// 先关闭准入，再等已接受作业收尾，不丢弃排队的写入。
    pub(crate) fn close_and_wait(&self) {
        let mut state = self.shared.lock();
        state.closed = true;
        while state.outstanding != 0 || state.workers != 0 {
            state = self
                .shared
                .idle
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

fn run_worker(shared: Arc<Shared>) {
    loop {
        let job = {
            let mut state = shared.lock();
            match state.queue.pop_front() {
                Some(job) => job,
                None => {
                    state.workers -= 1;
                    shared.idle.notify_all();
                    return;
                }
            }
        };
        // panic 不是业务失败；隔离执行体后交付失败回执，保持容量账目可回收。
        if catch_unwind(AssertUnwindSafe(job.work)).is_err()
            && let Some(failed) = job.failed
        {
            let _ = catch_unwind(AssertUnwindSafe(failed));
        }
        shared.lock().outstanding -= 1;
    }
}

// 读取最多两个解析快照并行；独立写入通道避免读取占满后阻塞提交收尾。
// 容量是初始准入策略，109 记录代表性负载测量，不能据此承诺单文件内存上限。
pub(crate) fn reads() -> Arc<Executor> {
    static EXECUTOR: OnceLock<Arc<Executor>> = OnceLock::new();
    Arc::clone(EXECUTOR.get_or_init(|| Executor::new("panta-read", 2, 64)))
}
pub(crate) fn writes() -> Arc<Executor> {
    static EXECUTOR: OnceLock<Arc<Executor>> = OnceLock::new();
    Arc::clone(EXECUTOR.get_or_init(|| Executor::new("panta-write", 1, 32)))
}

#[cfg(test)]
mod tests {
    use super::Executor;
    use std::sync::mpsc;
    use std::time::Duration;

    fn no_op() {}

    fn send_once(sender: mpsc::Sender<()>) -> impl FnOnce() {
        move || {
            let _ = sender.send(());
        }
    }

    #[test]
    fn debug_reports_executor_configuration() {
        let executor = Executor::new("debug-test", 3, 7);
        let rendered = format!("{executor:?}");

        assert!(rendered.contains("name: \"debug-test\""));
        assert!(rendered.contains("workers: 3"));
        assert!(rendered.contains("capacity: 7"));
        assert!(rendered.contains(".."));
    }

    #[test]
    fn capacity_includes_running_and_queued_work_and_recovers()
    -> Result<(), Box<dyn std::error::Error>> {
        let executor = Executor::new("bounded-test", 1, 2);
        let (started, ready) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        executor.submit(move || {
            let _ = started.send(());
            let _ = gate.recv();
        })?;
        ready.recv_timeout(Duration::from_secs(2))?;
        let (finished, done) = mpsc::channel();
        executor.submit(send_once(finished))?;
        assert_eq!(
            executor
                .submit(no_op)
                .err()
                .ok_or("capacity not enforced")?
                .kind(),
            std::io::ErrorKind::WouldBlock
        );
        assert_eq!(
            executor
                .shared
                .state
                .lock()
                .map_err(|_| "poisoned")?
                .workers,
            1
        );
        release.send(())?;
        done.recv_timeout(Duration::from_secs(2))?;
        wait_idle(&executor)?;
        executor.submit(no_op)?;
        wait_idle(&executor)?;
        Ok(())
    }

    #[test]
    fn panicked_work_reports_failure_and_does_not_strand_queued_work()
    -> Result<(), Box<dyn std::error::Error>> {
        let executor = Executor::new("panic-test", 1, 2);
        let (failed, failure) = mpsc::channel();
        executor.submit_reported(
            || panic!("worker probe"),
            move || {
                let _ = failed.send(());
            },
        )?;
        failure.recv_timeout(Duration::from_secs(2))?;
        wait_idle(&executor)?;
        let (sent, received) = mpsc::channel();
        executor.submit(send_once(sent))?;
        received.recv_timeout(Duration::from_secs(2))?;
        wait_idle(&executor)?;
        Ok(())
    }

    #[test]
    fn close_waits_for_accepted_work_and_rejects_further_admission()
    -> Result<(), Box<dyn std::error::Error>> {
        let executor = Executor::new("close-test", 1, 2);
        let (release, gate) = mpsc::channel();
        let (sent, received) = mpsc::channel();
        executor.submit(move || {
            let _ = gate.recv();
        })?;
        executor.submit(move || {
            let _ = sent.send(());
        })?;
        let closed = std::sync::Arc::clone(&executor);
        let waiter = std::thread::spawn(move || closed.close_and_wait());
        release.send(())?;
        waiter.join().map_err(|_| "shutdown waiter panicked")?;
        received.recv_timeout(Duration::from_secs(2))?;
        assert_eq!(
            executor
                .submit(no_op)
                .err()
                .ok_or("closed admission succeeded")?
                .kind(),
            std::io::ErrorKind::BrokenPipe
        );
        wait_idle(&executor)?;
        Ok(())
    }

    #[test]
    fn spawn_failure_rolls_back_capacity_and_drops_the_unstarted_job()
    -> Result<(), Box<dyn std::error::Error>> {
        let executor = Executor::new("spawn-failure-test", 1, 1);
        let (sent, received) = mpsc::channel();
        let error = executor
            .submit_with(
                super::Job {
                    work: Box::new(send_once(sent)),
                    failed: None,
                },
                |_| Err(std::io::Error::other("spawn probe")),
            )
            .err()
            .ok_or("spawn failure was ignored")?;
        assert_eq!(error.to_string(), "spawn probe");
        assert!(matches!(
            received.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
        wait_idle(&executor)?;
        executor.submit(no_op)?;
        wait_idle(&executor)?;
        Ok(())
    }

    #[test]
    fn diagnostics_show_limits_and_poisoned_lock_can_resume_admission()
    -> Result<(), Box<dyn std::error::Error>> {
        let executor = Executor::new("diagnostic-test", 1, 2);
        let diagnostic = format!("{executor:?}");
        assert!(diagnostic.contains("diagnostic-test"));
        assert!(diagnostic.contains("workers: 1"));
        assert!(diagnostic.contains("capacity: 2"));
        let shared = std::sync::Arc::clone(&executor.shared);
        let poison = std::thread::spawn(move || {
            let _guard = shared.lock();
            panic!("lock probe before any state mutation");
        });
        assert!(poison.join().is_err());
        let (sent, received) = mpsc::channel();
        executor.submit(move || {
            let _ = sent.send(());
        })?;
        received.recv_timeout(Duration::from_secs(2))?;
        wait_idle(&executor)?;
        Ok(())
    }

    fn wait_idle(executor: &Executor) -> Result<(), Box<dyn std::error::Error>> {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            let state = executor.shared.lock();
            if state.outstanding == 0 && state.workers == 0 {
                return Ok(());
            }
            drop(state);
            if std::time::Instant::now() >= deadline {
                return Err("workers did not retire".into());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

/// 工程会话的执行通道；生产默认共享进程级容量，测试可注入独立通道。
#[derive(Debug)]
pub(crate) struct ProjectExecution {
    pub(crate) reads: Arc<Executor>,
    pub(crate) writes: Arc<Executor>,
}
impl Default for ProjectExecution {
    fn default() -> Self {
        Self {
            reads: reads(),
            writes: writes(),
        }
    }
}
