/// Qt 文本进入 Rust 服务前的可往返 UTF-8 边界。
#pragma once

#include <QString>
#include <string>

namespace panta::bridge {

/// 拒绝有损 Unicode 转换；失败保留 out，诊断写入可选的 error。
[[nodiscard]] bool to_boundary_utf8(const QString& text, std::string* out, QString* error);

} // namespace panta::bridge
