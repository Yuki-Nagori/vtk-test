//! 块缓存与预取（task 027）。
//!
//! 未命中的块按文件偏移排序，相邻（间隙不超过 [`MERGE_GAP`]）的块合并为一次大块顺序读，
//! 单次读上限 [`MAX_READ`]；读完后按块并行解压。Windows 上小读与 seek 代价高，合并读把一次切面
//! 的读调用数降到与“连续区段数”同阶，而不是与块数同阶。

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use super::format::{ChunkBytes, ChunkCells, ChunkHeader, ChunkSource, decode_cells, decompress};
use super::{ChunkError, worker_count};

/// 合并读允许跳过的最大间隙：读多几百 KiB 比多一次读调用便宜。
const MERGE_GAP: u64 = 1 << 20;
/// 单次合并读上限。
const MAX_READ: u64 = 64 << 20;

struct Entry {
    cells: Arc<ChunkCells>,
    bytes: usize,
    used: u64,
}

#[derive(Default)]
struct State {
    entries: HashMap<usize, Entry>,
    loading: HashSet<usize>,
    bytes: usize,
    clock: u64,
    hits: u64,
    misses: u64,
}

/// 按字节预算的 LRU 块缓存。预算小于单批命中块时，本批块仍完整返回给调用方，只是不留在缓存里。
pub struct ChunkCache {
    source: Arc<dyn ChunkSource>,
    headers: Arc<Vec<ChunkHeader>>,
    budget: AtomicUsize,
    state: Mutex<State>,
    ready: Condvar,
}

impl ChunkCache {
    pub(crate) fn new(
        source: Arc<dyn ChunkSource>,
        headers: Arc<Vec<ChunkHeader>>,
        budget: usize,
    ) -> Self {
        Self {
            source,
            headers,
            budget: AtomicUsize::new(budget),
            state: Mutex::new(State::default()),
            ready: Condvar::new(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn set_budget(&self, budget: usize) {
        self.budget.store(budget, Ordering::Relaxed);
        let mut state = self.lock();
        self.evict(&mut state, &HashSet::new());
    }

    pub fn budget(&self) -> usize {
        self.budget.load(Ordering::Relaxed)
    }

    /// （驻留字节，驻留块数，命中次数，未命中次数）。
    pub fn stats(&self) -> (usize, usize, u64, u64) {
        let state = self.lock();
        (state.bytes, state.entries.len(), state.hits, state.misses)
    }

    pub fn contains(&self, id: usize) -> bool {
        self.lock().entries.contains_key(&id)
    }

    pub fn clear(&self) {
        let mut state = self.lock();
        state.entries.clear();
        state.bytes = 0;
    }

    /// 取一批块；未命中的合并读取。返回顺序与 `ids` 一致。
    pub fn get_many(&self, ids: &[usize]) -> Result<Vec<Arc<ChunkCells>>, ChunkError> {
        if let Some(&bad) = ids.iter().find(|&&id| id >= self.headers.len()) {
            return Err(ChunkError::Invalid(format!("chunk {bad} out of range")));
        }
        let mut found: HashMap<usize, Arc<ChunkCells>> = HashMap::new();
        loop {
            let mut claimed = Vec::new();
            let mut waiting = false;
            {
                let mut state = self.lock();
                for &id in ids {
                    if found.contains_key(&id) {
                        continue;
                    }
                    state.clock += 1;
                    let clock = state.clock;
                    if let Some(entry) = state.entries.get_mut(&id) {
                        entry.used = clock;
                        found.insert(id, entry.cells.clone());
                        state.hits += 1;
                    } else if state.loading.contains(&id) {
                        waiting = true;
                    } else {
                        state.loading.insert(id);
                        state.misses += 1;
                        claimed.push(id);
                    }
                }
            }
            if !claimed.is_empty() {
                let loaded = self.load(&claimed);
                let mut state = self.lock();
                claimed.iter().for_each(|id| {
                    state.loading.remove(id);
                });
                match loaded {
                    Ok(loaded) => {
                        let keep: HashSet<usize> = claimed.iter().copied().collect();
                        for (id, cells) in claimed.iter().zip(loaded) {
                            let bytes = cells.resident_bytes();
                            state.clock += 1;
                            let used = state.clock;
                            state.bytes += bytes;
                            state.entries.insert(
                                *id,
                                Entry {
                                    cells: cells.clone(),
                                    bytes,
                                    used,
                                },
                            );
                            found.insert(*id, cells);
                        }
                        self.evict(&mut state, &keep);
                        self.ready.notify_all();
                    }
                    Err(error) => {
                        self.ready.notify_all();
                        return Err(error);
                    }
                }
            }
            if waiting {
                let state = self.lock();
                let pending = ids
                    .iter()
                    .any(|id| !found.contains_key(id) && state.loading.contains(id));
                if pending {
                    drop(
                        self.ready
                            .wait(state)
                            .unwrap_or_else(std::sync::PoisonError::into_inner),
                    );
                }
            }
            if ids.iter().all(|id| found.contains_key(id)) {
                return Ok(ids.iter().filter_map(|id| found.get(id).cloned()).collect());
            }
        }
    }

    /// 超预算时淘汰最久未用的块；优先保留本批 `keep`，本批自身超预算时仍淘汰其中最早的。
    fn evict(&self, state: &mut State, keep: &HashSet<usize>) {
        let budget = self.budget();
        while state.bytes > budget && !state.entries.is_empty() {
            let victim = state
                .entries
                .iter()
                .min_by_key(|(id, entry)| (keep.contains(id), entry.used))
                .map(|(&id, _)| id);
            let Some(victim) = victim else { break };
            if let Some(entry) = state.entries.remove(&victim) {
                state.bytes -= entry.bytes;
            }
        }
    }

    /// 合并读 + 并行解压。
    fn load(&self, ids: &[usize]) -> Result<Vec<Arc<ChunkCells>>, ChunkError> {
        let ranges: Vec<(u64, u64)> = ids
            .iter()
            .map(|&id| self.headers[id].cells_range())
            .collect();
        read_merged(self.source.as_ref(), &ranges, |slot, bytes| {
            let header = &self.headers[ids[slot]];
            let cells = decode_cells(&decompress(bytes, header.cells_raw_len as usize)?)?;
            if cells.tets.len() != header.tet_count as usize
                || cells.nodes.len() != header.node_count as usize
                || cells.cell_count() != header.cell_count as usize
            {
                return Err(ChunkError::Format("chunk counts mismatch".to_owned()));
            }
            Ok(Arc::new(cells))
        })
    }
}

/// 按文件偏移排序后把相邻区间（间隙不超过 [`MERGE_GAP`]、单次不超过 [`MAX_READ`]）合并成大块读，
/// 再对每个区间并行调用 `decode(槽位, 字节)`；结果按输入顺序返回。单元块与场值块共用。
pub(crate) fn read_merged<R: Send>(
    source: &dyn ChunkSource,
    ranges: &[(u64, u64)],
    decode: impl Fn(usize, &[u8]) -> Result<R, ChunkError> + Sync,
) -> Result<Vec<R>, ChunkError> {
    let mut order: Vec<usize> = (0..ranges.len()).collect();
    order.sort_unstable_by_key(|&slot| ranges[slot].0);
    // 区段：(起点, 终点, 区段内的槽位)。
    let mut runs: Vec<(u64, u64, Vec<usize>)> = Vec::new();
    for slot in order {
        let (start, end) = ranges[slot];
        match runs.last_mut() {
            Some(run) if start <= run.1 + MERGE_GAP && end - run.0 <= MAX_READ => {
                run.1 = run.1.max(end);
                run.2.push(slot);
            }
            _ => runs.push((start, end, vec![slot])),
        }
    }
    let buffers: Vec<Result<ChunkBytes, ChunkError>> =
        parallel_map(&runs, |run| source.read_at(run.0, (run.1 - run.0) as usize));
    let buffers: Vec<ChunkBytes> = buffers.into_iter().collect::<Result<_, _>>()?;
    let mut pieces: Vec<(usize, &[u8])> = Vec::with_capacity(ranges.len());
    for (run, buffer) in runs.iter().zip(&buffers) {
        for &slot in &run.2 {
            let (start, end) = ranges[slot];
            let offset = (start - run.0) as usize;
            pieces.push((slot, &buffer[offset..offset + (end - start) as usize]));
        }
    }
    let decoded = parallel_map(&pieces, |(slot, bytes)| {
        decode(*slot, bytes).map(|value| (*slot, value))
    });
    let mut out: Vec<Option<R>> = (0..ranges.len()).map(|_| None).collect();
    for item in decoded {
        let (slot, value) = item?;
        out[slot] = Some(value);
    }
    out.into_iter()
        .map(|value| value.ok_or_else(|| ChunkError::Format("chunk missing".to_owned())))
        .collect()
}

/// 简单的作用域并行 map：原子计数器分发，结果按输入顺序返回。
pub(crate) fn parallel_map<T: Sync, R: Send>(items: &[T], work: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let next = AtomicUsize::new(0);
    let slots: Vec<Mutex<Option<R>>> = items.iter().map(|_| Mutex::new(None)).collect();
    let threads = worker_count().min(items.len());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(index) else { break };
                    let result = work(item);
                    if let Ok(mut slot) = slots[index].lock() {
                        *slot = Some(result);
                    }
                }
            });
        }
    });
    slots
        .into_iter()
        .filter_map(|slot| slot.into_inner().ok().flatten())
        .collect()
}

struct PrefetchState {
    pending: Vec<usize>,
    stopped: bool,
}

/// 后台预取：最新请求覆盖未开始的旧请求；预取失败静默丢弃（正式读取时会再报告）。
pub struct Prefetcher {
    shared: Arc<(Mutex<PrefetchState>, Condvar)>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Prefetcher {
    pub fn new(cache: Arc<ChunkCache>) -> Self {
        let shared = Arc::new((
            Mutex::new(PrefetchState {
                pending: Vec::new(),
                stopped: false,
            }),
            Condvar::new(),
        ));
        let thread_shared = shared.clone();
        let worker = std::thread::Builder::new()
            .name("chunk-prefetch".to_owned())
            .spawn(move || {
                let (lock, signal) = &*thread_shared;
                loop {
                    let batch = {
                        let mut state = lock
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        while state.pending.is_empty() && !state.stopped {
                            state = signal
                                .wait(state)
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                        }
                        if state.stopped {
                            return;
                        }
                        std::mem::take(&mut state.pending)
                    };
                    // 预取量封顶为预算一半：再多只会互相挤出缓存，还会让瞬时解压内存越过预算。
                    let cap = (cache.budget() / 2).max(1);
                    let mut bytes = 0usize;
                    let missing: Vec<usize> = batch
                        .into_iter()
                        .filter(|&id| !cache.contains(id))
                        .take_while(|&id| {
                            let fits = bytes == 0 || bytes < cap;
                            bytes += cache
                                .headers
                                .get(id)
                                .map_or(0, |h| h.cells_raw_len as usize);
                            fits
                        })
                        .collect();
                    if !missing.is_empty() {
                        let _ = cache.get_many(&missing);
                    }
                }
            })
            .ok();
        Self { shared, worker }
    }

    pub fn request(&self, ids: Vec<usize>) {
        let (lock, signal) = &*self.shared;
        if let Ok(mut state) = lock.lock() {
            state.pending = ids;
            signal.notify_one();
        }
    }
}

impl Drop for Prefetcher {
    fn drop(&mut self) {
        let (lock, signal) = &*self.shared;
        if let Ok(mut state) = lock.lock() {
            state.stopped = true;
            state.pending.clear();
            signal.notify_all();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
