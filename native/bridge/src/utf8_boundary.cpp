#include "utf8_boundary.hpp"

#include <QByteArray>
#include <cstddef>
#include <string>

namespace panta::bridge {

bool to_boundary_utf8(const QString& text, std::string* out, QString* error) {
    const QByteArray utf8 = text.toUtf8();
    // 有损转换（未配对代理项等）在往返比较中暴露：当前契约只支持可往返
    // Unicode 路径，拒绝而不是替换字符后访问其它文件。
    if (QString::fromUtf8(utf8) != text) {
        if (error != nullptr) {
            *error = QStringLiteral("path.non_unicode");
        }
        return false;
    }
    if (out != nullptr) {
        *out = std::string(utf8.constData(), static_cast<std::size_t>(utf8.size()));
    }
    return true;
}

} // namespace panta::bridge
