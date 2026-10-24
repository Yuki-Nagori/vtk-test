//! 网格文件读写：`import` 负责格式识别、来源快照、预览与解析，`export` 负责各格式写出。
//! 两个模块互不依赖；STEP / IGES 的几何读取由后续 OCCT adapter 接入。

pub mod export;
pub mod import;
