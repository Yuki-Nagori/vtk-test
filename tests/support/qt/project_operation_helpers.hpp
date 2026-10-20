// 工程测试等待真实后台终态；命令准入成功不代表文件已提交或显示缓冲已就绪。
#pragma once

#include "panta/visualization/mesh_source.hpp"
#include "project_view_model.hpp"
#include <QEventLoop>
#include <QObject>
#include <QString>
#include <QTimer>
#include <memory>

namespace panta::test {

inline bool finish_project_operation(bridge::ProjectViewModel& model, bool accepted,
                                     int timeout_ms = 60000) {
    if (!accepted) {
        return false;
    }
    if (model.projectOperationPending()) {
        QEventLoop loop;
        QTimer timeout;
        timeout.setSingleShot(true);
        QObject::connect(&model, &bridge::ProjectViewModel::projectOperationFinished, &loop,
                         &QEventLoop::quit);
        QObject::connect(&timeout, &QTimer::timeout, &loop, &QEventLoop::quit);
        timeout.start(timeout_ms);
        loop.exec();
    }
    return !model.projectOperationPending() && model.errorCode().isEmpty();
}

inline bool wait_for_active_mesh(bridge::ProjectViewModel& model, const QString document_id,
                                 int timeout_ms = 60000) {
    const auto ready = [&model, &document_id] {
        return model.activeDocumentId() == document_id && model.mesh_snapshot() != nullptr;
    };
    if (!ready()) {
        QEventLoop loop;
        QTimer timeout;
        timeout.setSingleShot(true);
        const auto check = [&loop, &ready] {
            if (ready()) {
                loop.quit();
            }
        };
        QObject::connect(&model, &bridge::ProjectViewModel::meshChanged, &loop, check);
        QObject::connect(&model, &bridge::ProjectViewModel::activeDocumentChanged, &loop, check);
        QObject::connect(&timeout, &QTimer::timeout, &loop, &QEventLoop::quit);
        timeout.start(timeout_ms);
        loop.exec();
    }
    return ready();
}

} // namespace panta::test
