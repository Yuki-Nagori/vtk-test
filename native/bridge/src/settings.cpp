#include "settings.hpp"

#include <QObject>
#include <memory>
#include <panta/qt_adapter/system_motion_preference.hpp>

namespace panta::bridge {

Settings::Settings(QObject* parent)
    : QObject(parent), m_preference(std::make_unique<panta::qt_adapter::SystemMotionPreference>()) {
    connect(m_preference.get(), &panta::qt_adapter::SystemMotionPreference::reducedMotionChanged,
            this, &Settings::reducedMotionChanged);
}

Settings::~Settings() = default;

bool Settings::reducedMotion() const { return m_preference->reducedMotion(); }

} // namespace panta::bridge
