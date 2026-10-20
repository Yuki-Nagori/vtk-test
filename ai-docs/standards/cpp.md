# C++ 编码与资源管理

更新日期：2026-10-20。适用于现有 C++ adapter、Qt Bridge 和 VTK 后端；规则与验证按实际改动范围执行。

适用于自有 native 代码。项目基线为 C++20；C++ Core Guidelines 是维护者发布的工程指导，不是 ISO 标准正文。查阅后采用下列项目规则，不要求第三方源码套用本仓库风格。

## 官方依据

Core Guidelines 强调资源管理、类型安全、接口与并发，建议以 RAII 表达资源生命周期。它明确说明语言定义仍以 ISO C++ 标准为准。[C++ Core Guidelines](https://isocpp.github.io/CppCoreGuidelines/CppCoreGuidelines)

## 项目规则

- 每个自有 target 显式声明 C++20 要求并关闭编译器扩展；不引入 C++23 专属设施作为默认依赖，例如不能直接假定 `std::expected` 可用。
- 文件使用 `snake_case.hpp/.cpp`，类型使用 `PascalCase`，普通函数/变量使用 `snake_case`；Qt 属性、槽和信号遵循 Qt 的 `camelCase`，适配器边界允许原生 API 命名。C++ 格式由根 [.clang-format](../../.clang-format) 统一，检查命令 `clang-format --dry-run -Werror <文件>`。
- 优先值语义和 RAII；唯一所有权用 `std::unique_ptr`，仅在确有共享生命周期时使用 `std::shared_ptr`。Qt parent、OCCT handle 和 VTK 引用计数按各自规则管理，不再叠加独立释放者。
- `std::span`、引用和裸指针作为借用时明确有效期，不能跨后台任务保存短命栈数据。公开 ID 使用独立类型，避免把网格索引、几何 ID 和任务 ID 混用。
- 公共头文件只包含必要依赖；几何/网格核心头不暴露 Qt、VTK、OCCT 实现对象。禁止头文件级 `using namespace` 和依赖隐式 include 顺序。
- 可恢复失败返回明确的错误契约；使用异常的适配器在边界捕获并转换。析构不抛异常，不能为消除告警随意添加 `noexcept`。错误表示与 Rust 服务和 CXX DTO 保持一致，不依赖日志文字判断错误类别。
- 后台任务捕获输入快照与取消令牌，不能捕获生命周期不受控的 `this`。不要持锁调用外部回调；共享状态必须记录同步方式。

## 验证与例外

对自有代码应用编译器告警与格式检查。通过边界测试验证索引范围、取消时释放和借用生命周期；涉及平台窗口或图形资源时另做真实窗口验收，按实际情况记录未覆盖的系统故障。当前检查入口见[质量工具链](../modules/quality-tooling.md)。第三方头告警应隔离，不能通过全局禁用告警掩盖自有问题。

涉及 ABI、对齐、零拷贝或异常策略的例外写入对应 task 的决策记录，说明生命周期与验证方法。
