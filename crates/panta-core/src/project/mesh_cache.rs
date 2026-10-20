//! 工程运行期 Mesh 驻留预算及活动资产的 LRU 策略。
use panta_mesh::SurfaceMesh;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

const DEFAULT_MESH_CACHE_BUDGET_BYTES: usize = 128 * 1024 * 1024;

/// 以三角数组容量计量的工程运行期缓存；活动文档 pin 由 ViewModel 的选择意图驱动。
#[derive(Debug)]
pub(super) struct SurfaceMeshCache {
    meshes: HashMap<String, Arc<SurfaceMesh>>,
    recency: VecDeque<String>,
    active_id: Option<String>,
    budget_bytes: usize,
}

impl Default for SurfaceMeshCache {
    fn default() -> Self {
        Self {
            meshes: HashMap::new(),
            recency: VecDeque::new(),
            active_id: None,
            budget_bytes: DEFAULT_MESH_CACHE_BUDGET_BYTES,
        }
    }
}

impl SurfaceMeshCache {
    #[cfg(test)]
    pub(super) fn with_budget(budget_bytes: usize) -> Self {
        Self {
            budget_bytes,
            ..Self::default()
        }
    }

    pub(super) fn insert_active(&mut self, id: String, mesh: SurfaceMesh) {
        self.remove(&id);
        self.meshes.insert(id.clone(), Arc::new(mesh));
        self.active_id = Some(id.clone());
        self.touch(&id);
        self.trim();
    }

    pub(super) fn activate(&mut self, id: &str) -> bool {
        if !self.meshes.contains_key(id) {
            return false;
        }
        self.active_id = Some(id.to_owned());
        self.touch(id);
        self.trim();
        true
    }

    pub(super) fn deactivate(&mut self) {
        self.active_id = None;
        self.trim();
    }

    pub(super) fn remove(&mut self, id: &str) {
        self.meshes.remove(id);
        self.recency.retain(|candidate| candidate != id);
        if self.active_id.as_deref() == Some(id) {
            self.active_id = None;
        }
    }

    pub(super) fn clear(&mut self) {
        self.meshes.clear();
        self.recency.clear();
        self.active_id = None;
    }

    pub(super) fn get(&self, id: &str) -> Option<&SurfaceMesh> {
        self.meshes.get(id).map(Arc::as_ref)
    }

    pub(super) fn snapshot(&self, id: &str) -> Option<Arc<SurfaceMesh>> {
        self.meshes.get(id).map(Arc::clone)
    }

    pub(super) fn resident_ids(&self) -> Vec<String> {
        self.recency.iter().cloned().collect()
    }

    pub(super) fn retained_bytes(&self) -> usize {
        self.meshes.values().fold(0usize, |total, mesh| {
            total.saturating_add(
                mesh.triangles
                    .capacity()
                    .saturating_mul(std::mem::size_of::<[[f64; 3]; 3]>()),
            )
        })
    }

    pub(super) fn touch(&mut self, id: &str) {
        self.recency.retain(|candidate| candidate != id);
        self.recency.push_back(id.to_owned());
    }

    pub(super) fn trim(&mut self) {
        while self.retained_bytes() > self.budget_bytes {
            let Some(candidate) = self
                .recency
                .iter()
                .find(|id| self.active_id.as_deref() != Some(id.as_str()))
                .cloned()
            else {
                // 允许唯一活动 Mesh 超出预算，避免逐出当前激活数据。
                break;
            };
            self.remove(&candidate);
        }
    }
}
