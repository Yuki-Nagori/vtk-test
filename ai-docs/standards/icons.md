# panta 图标设计规范

更新日期：2026-10-24。适用于最小可视化测试界面的 `qml/icons/` 与实际使用点。资源清单以 [QML 构建配置](../../qml/CMakeLists.txt) 为准；不为已移除的 Ribbon、商业 CAE 面板或 HTML 原型保留图标。

## 视觉语言

- 图标使用清楚的主体轮廓与稳定比例，同一动作复用同一资源和含义。颜色只作辅助，不单独表达选中、错误或禁用状态。
- 保留 SVG 的 viewBox、坐标比例和必要留白；尺寸由 Theme 与宿主控制，不为适配固定画布拉伸路径。
- caret、关闭和打开等操作符号可使用单色，通过宿主状态着色。需要彩色图标时保留原始配色，不经过单色遮罩。
- 应用标识与操作图标分别管理。`qml/branding/panta-placeholder.svg` 是临时应用标识；替换时同步资源与窗口使用点。
- 图标是装饰，动作语义由按钮文字、tooltip 与 `Accessible.name` 提供；保留键盘焦点和可理解的禁用原因。

## 资源与调用

项目自绘：`caret.svg`、`close.svg`、`open.svg`、`stl-file.svg`（导入资产页签），以及 CAE 专用的 `section.svg`（切面）和 `tree-field.svg`（标量场，task 039）。第三方：Lucide 图标（见下文来源记录）。新增资源先确认是否可复用已有概念，再登记到 QML 构建资源和实际使用点；不增加没有消费者的图标。

`ThemedIcon.preserveSourceColors` 控制加载方式：单色资源使用 `image://panta-icons/<name>/<argb>`，彩色资源直接通过 `qrc:/qt/qml/Panta/Shell/icons/<name>.svg` 加载。provider 白名单与已打包资源保持一致，未知 ID 返回失败，不静默替换成另一个动作图标。

- 静态 SVG 经模块资源打包，运行中的 QML 不加载 `ai-docs/` 或依赖当前工作目录。
- SVG 不嵌入位图、字体、脚本或外部 URL。宿主使用 Qt 逻辑像素，不在图标组件中乘 DPR。
- QML Image 的 SVG 解码依赖锁定 Qt 的 qsvg 插件；安装产物需验证插件和资源均可加载。
- `caret.svg` 的 viewBox 为 `0 0 7 5`，路径为 `M0.7 0.6h5.6L3.5 4.4z`；修改几何时用实际显示尺寸检查对齐与清晰度。

## 来源与使用记录

项目自绘图标在资源文件中保留来源说明。新增资源记录作者 / 来源 URL、许可证、版本、修改情况与使用位置。

第三方图标集按 [034](../task/034-ui-redesign.md) Q3 使用 [Lucide](https://lucide.dev/)（ISC）；Lucide 中由 Feather 派生的图标另受 MIT 许可，以上游 `LICENSE` 列出的名单为准；本仓库使用的派生图标在下表「许可」列标为「ISC + MIT（Feather）」，SVG 注释附 MIT 版权行，`LICENSE-lucide.txt` 收录上游许可全文（含 MIT 段与名单）。task 039 复核时发现 `tree-expand.svg`（`chevron-right`）属于派生图标而原记录写作未使用，已一并更正。引入规则：

- 固定上游版本，从该版本 tag 的 `icons/<name>.svg` 取原文件；不改几何，只把根属性整理为单行并加注释。
- 每个 SVG 注释写明上游版本、原文件名、许可证与版权行（Feather 派生图标另写 MIT 版权行），并指向 `qml/icons/LICENSE-lucide.txt`（许可全文，随源码分发）。
- 引入前对照上游 `LICENSE` 的 Feather 名单（含旧名，如 `alert-triangle` 对应 `triangle-alert`）。
- 资源按用途命名（如 `playback-play`），在下表登记原名；单色图标加入 provider 白名单。

| 资源 | 上游（版本 / 原名） | 许可 | 修改 | 使用位置 |
|---|---|---|---|---|
| `playback-first.svg` | Lucide 1.61.0 / `skip-back` | ISC | 无几何修改 | `PlaybackBar` 首帧 |
| `playback-previous.svg` | Lucide 1.61.0 / `step-back` | ISC | 无几何修改 | `PlaybackBar` 上一帧 |
| `playback-play.svg` | Lucide 1.61.0 / `play` | ISC | 无几何修改 | `PlaybackBar` 播放 |
| `playback-pause.svg` | Lucide 1.61.0 / `pause` | ISC | 无几何修改 | `PlaybackBar` 暂停 |
| `playback-next.svg` | Lucide 1.61.0 / `step-forward` | ISC | 无几何修改 | `PlaybackBar` 下一帧 |
| `playback-last.svg` | Lucide 1.61.0 / `skip-forward` | ISC | 无几何修改 | `PlaybackBar` 末帧 |
| `playback-loop.svg` | Lucide 1.61.0 / `repeat` | ISC | 无几何修改 | `PlaybackBar` 循环开关（开：描边 + 强调色） |
| `project-new.svg` | Lucide 1.61.0 / `file-plus` | ISC | 无几何修改 | `WorkbenchToolBar` 新建工程 |
| `project-open.svg` | Lucide 1.61.0 / `folder-open` | ISC | 无几何修改 | `WorkbenchToolBar` 打开工程 |
| `project-save.svg` | Lucide 1.61.0 / `save` | ISC | 无几何修改 | `WorkbenchToolBar` 保存工程 |
| `mesh-import.svg` | Lucide 1.61.0 / `file-input` | ISC | 无几何修改 | `WorkbenchToolBar` 导入网格 |
| `mesh-export.svg` | Lucide 1.61.0 / `file-output` | ISC | 无几何修改 | `WorkbenchToolBar` 导出活动网格 |
| `view-preset.svg` | Lucide 1.61.0 / `box` | ISC | 无几何修改 | `WorkbenchToolBar` 视图预设菜单 |
| `view-reset.svg` | Lucide 1.61.0 / `rotate-ccw` | ISC | 无几何修改 | `WorkbenchToolBar` 复位视图 |
| `screenshot.svg` | Lucide 1.61.0 / `camera` | ISC | 无几何修改 | `WorkbenchToolBar` 视口截图 |
| `panel-bottom.svg` | Lucide 1.61.0 / `panel-bottom` | ISC | 无几何修改 | `BottomPanel` 折叠 / 展开 |
| `panel-left.svg` | Lucide 1.61.0 / `panel-left` | ISC | 无几何修改 | `WorkbenchSidebar` 折叠、左栏窄轨展开 |
| `visibility-on.svg` | Lucide 1.61.0 / `eye` | ISC | 无几何修改 | `WorkbenchSidebar` 对象树可见性（显示中） |
| `visibility-off.svg` | Lucide 1.61.0 / `eye-off` | ISC | 无几何修改 | `WorkbenchSidebar` 对象树可见性（已隐藏） |
| `tree-expand.svg` | Lucide 1.61.0 / `chevron-right` | ISC + MIT（Feather） | 无几何修改 | `WorkbenchSidebar` 对象树展开指示（展开时旋转 90°） |
| `tree-project.svg` | Lucide 1.61.0 / `folder` | ISC | 无几何修改 | `WorkbenchSidebar` 对象树「工程」节点 |
| `tree-asset.svg` | Lucide 1.61.0 / `layers` | ISC | 无几何修改 | `WorkbenchSidebar` 对象树资产节点 |
| `tree-time-steps.svg` | Lucide 1.61.0 / `clock` | ISC + MIT（Feather） | 无几何修改 | `WorkbenchSidebar` 对象树「时间序列」节点 |
| `tree-quality.svg` | Lucide 1.61.0 / `activity` | ISC | 无几何修改 | `WorkbenchSidebar` 对象树「质量诊断」节点 |
| `tree-animation.svg` | Lucide 1.61.0 / `film` | ISC | 无几何修改 | `WorkbenchSidebar` 对象树「表面序列」节点 |
| `status-info.svg` | Lucide 1.61.0 / `info` | ISC + MIT（Feather） | 无几何修改 | `BottomPanel` 消息列表、`WorkbenchStatusBar` 状态消息（信息） |
| `status-success.svg` | Lucide 1.61.0 / `circle-check` | ISC | 无几何修改 | 同上（完成） |
| `status-warning.svg` | Lucide 1.61.0 / `triangle-alert` | ISC + MIT（Feather） | 无几何修改 | 同上（警告） |
| `status-error.svg` | Lucide 1.61.0 / `circle-x` | ISC + MIT（Feather） | 无几何修改 | 同上（错误） |

项目自绘 CAE 图标（task 039，24×24、2 px 描边、`currentColor`，与 Lucide 同一网格）：

| 资源 | 内容 | 使用位置 |
|---|---|---|
| `section.svg` | 斜置切平面与法向箭头（替换原临时使用的 Lucide `scissors`） | `WorkbenchToolBar` 切面开关（开：描边 + 强调色）、对象树「切面」节点 |
| `tree-field.svg` | 分段色标条与刻度 | 对象树「着色」节点 |

暂缓（无消费者或需自绘，不预先引入）：六向视图与等轴视图图标（视图预设目前是带文字的菜单）、体网格资产图标（资产类型尚未在树上区分体 / 表面网格）。

参考截图不证明第三方品牌、专有字形或图标包可复用，来源不明的素材需先核验。

生成式服务输出的权利归属不能证明独创性或没有第三方权利冲突。涉及生成资产时记录服务、输入来源与修改情况；商业发布前按实际素材核对条款与权利。[OpenAI Terms of Use](https://openai.com/policies/row-terms-of-use/)

## 验证与审查

资源变更检查打包清单、provider 白名单、调用点与测试的一致性；在锁定 Qt 下验证加载成功与无效 ID 路径。UI 变更检查浅色背景、选中 / 悬停 / 禁用 / 键盘焦点以及不同显示缩放下的对齐和清晰度。

按改动运行格式、QML lint 和相应组件测试；真实 QML 窗口检查不能由浏览器显示代替。涉及资源解码、高频更新或帧呈现的性能问题时，按[QML 性能要求](qml.md#性能基准)另建诊断任务，不为静态资源调整机械增加基准。