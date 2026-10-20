#pragma once

#include <QByteArray>
#include <QEventLoop>
#include <QLoggingCategory>
#include <QObject>
#include <QString>
#include <QTimer>
#include <QtCore/qlogging.h>
#include <chrono>
#include <cstddef>
#include <cstdio>
#include <mutex>
#include <stdexcept>

namespace panta::test {

/// 捕获 GUI 线程的 VtkViewport 提交日志；保持其他日志和原过滤器可见。
class FrameSubmissionCapture final {
  public:
    FrameSubmissionCapture() {
        {
            std::lock_guard lock(mutex());
            if (s_active != nullptr) {
                throw std::logic_error("Frame submission capture cannot be nested");
            }
            s_active = this;
        }
        // 分类注册也可能调用 filter；安装时不持有捕获锁，避免锁顺序反转。
        const auto previous = QLoggingCategory::installFilter(filter);
        {
            std::lock_guard lock(mutex());
            m_previousFilter = previous;
        }
        QLoggingCategory::installFilter(filter);
        std::lock_guard lock(mutex());
        m_previousHandler = qInstallMessageHandler(message);
        s_previousHandler = m_previousHandler;
    }

    FrameSubmissionCapture(const FrameSubmissionCapture&) = delete;
    auto operator=(const FrameSubmissionCapture&) -> FrameSubmissionCapture& = delete;

    ~FrameSubmissionCapture() {
        {
            std::lock_guard lock(mutex());
            qInstallMessageHandler(m_previousHandler);
        }
        QLoggingCategory::installFilter(m_previousFilter);
        std::lock_guard lock(mutex());
        s_active = nullptr;
    }

    [[nodiscard]] auto count() const -> std::size_t {
        std::lock_guard lock(mutex());
        return m_frames;
    }

    void reset() {
        std::lock_guard lock(mutex());
        m_frames = 0;
        m_surfaceSyncs = 0;
    }

    [[nodiscard]] auto surface_sync_count() const -> std::size_t {
        std::lock_guard lock(mutex());
        return m_surfaceSyncs;
    }

    [[nodiscard]] auto wait_after(std::size_t previousCount, std::chrono::milliseconds timeout)
        -> bool {
        if (count() > previousCount) {
            return true;
        }
        QEventLoop loop;
        {
            std::lock_guard lock(mutex());
            m_waitLoop = &loop;
        }
        QTimer timer;
        timer.setSingleShot(true);
        QObject::connect(&timer, &QTimer::timeout, &loop, &QEventLoop::quit);
        timer.start(timeout);
        if (count() <= previousCount) {
            loop.exec();
        }
        {
            std::lock_guard lock(mutex());
            m_waitLoop = nullptr;
        }
        return count() > previousCount;
    }

  private:
    static void filter(QLoggingCategory* category) {
        QLoggingCategory::CategoryFilter previous = nullptr;
        {
            std::lock_guard lock(mutex());
            if (s_active != nullptr) {
                previous = s_active->m_previousFilter;
            }
        }
        if (previous != nullptr) {
            previous(category);
        }
        if (qstrcmp(category->categoryName(), "panta.viewport") == 0) {
            category->setEnabled(QtDebugMsg, true);
        }
    }

    static void message(QtMsgType type, const QMessageLogContext& context, const QString& text) {
        QtMessageHandler previous = nullptr;
        {
            std::lock_guard lock(mutex());
            auto* capture = s_active;
            previous = capture == nullptr ? s_previousHandler : capture->m_previousHandler;
            const bool viewport =
                context.category != nullptr && qstrcmp(context.category, "panta.viewport") == 0;
            if (capture != nullptr && viewport && type == QtDebugMsg) {
                if (text.startsWith(QStringLiteral("frame submitted"))) {
                    ++capture->m_frames;
                    if (capture->m_waitLoop != nullptr) {
                        capture->m_waitLoop->quit();
                    }
                    return;
                }
                if (text.startsWith(QStringLiteral("surface synchronized"))) {
                    ++capture->m_surfaceSyncs;
                    return;
                }
            }
        }
        // 原日志处理器可有自己的锁与 Qt 分类；转发时不持有捕获锁。
        if (previous != nullptr) {
            previous(type, context, text);
        } else {
            const auto formatted = qFormatLogMessage(type, context, text).toLocal8Bit();
            std::fprintf(stderr, "%s\n", formatted.constData());
        }
    }

    static auto mutex() -> std::mutex& {
        static std::mutex value;
        return value;
    }
    inline static FrameSubmissionCapture* s_active = nullptr;
    inline static QtMessageHandler s_previousHandler = nullptr;
    std::size_t m_frames = 0;
    std::size_t m_surfaceSyncs = 0;
    QEventLoop* m_waitLoop = nullptr;
    QtMessageHandler m_previousHandler = nullptr;
    QLoggingCategory::CategoryFilter m_previousFilter = nullptr;
};

} // namespace panta::test
