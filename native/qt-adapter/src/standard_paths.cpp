#include "panta/qt_adapter/standard_paths.hpp"

#include <QStandardPaths>

namespace panta::qt_adapter {

QString standard_location(StandardLocation location) {
    switch (location) {
    case StandardLocation::UserConfig:
        return QStandardPaths::writableLocation(QStandardPaths::AppConfigLocation);
    case StandardLocation::AppData:
        return QStandardPaths::writableLocation(QStandardPaths::AppDataLocation);
    case StandardLocation::Cache:
        return QStandardPaths::writableLocation(QStandardPaths::CacheLocation);
    case StandardLocation::Session:
        return QStandardPaths::writableLocation(QStandardPaths::TempLocation);
    case StandardLocation::Documents:
        return QStandardPaths::writableLocation(QStandardPaths::DocumentsLocation);
    }
    return {};
}

} // namespace panta::qt_adapter
