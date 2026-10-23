#include "viewport_tools.hpp"

#include <cmath>
#include <limits>

namespace panta::visualization {
namespace {

using Vec = std::array<double, 3>;

Vec sub(const Vec& a, const Vec& b) { return {a[0] - b[0], a[1] - b[1], a[2] - b[2]}; }
Vec cross(const Vec& a, const Vec& b) {
    return {a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]};
}
double dot(const Vec& a, const Vec& b) { return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]; }
Vec normalized(const Vec& v) {
    const double length = std::sqrt(dot(v, v));
    return {v[0] / length, v[1] / length, v[2] / length};
}

} // namespace

std::optional<ViewPreset> parse_view_preset(const QString& name) {
    if (name == QLatin1String("front"))
        return ViewPreset::Front;
    if (name == QLatin1String("back"))
        return ViewPreset::Back;
    if (name == QLatin1String("left"))
        return ViewPreset::Left;
    if (name == QLatin1String("right"))
        return ViewPreset::Right;
    if (name == QLatin1String("top"))
        return ViewPreset::Top;
    if (name == QLatin1String("bottom"))
        return ViewPreset::Bottom;
    if (name == QLatin1String("iso"))
        return ViewPreset::Isometric;
    return std::nullopt;
}

CameraFrame preset_frame(ViewPreset preset) {
    switch (preset) {
    case ViewPreset::Front:
        return {{0.0, -1.0, 0.0}, {0.0, 0.0, 1.0}};
    case ViewPreset::Back:
        return {{0.0, 1.0, 0.0}, {0.0, 0.0, 1.0}};
    case ViewPreset::Left:
        return {{-1.0, 0.0, 0.0}, {0.0, 0.0, 1.0}};
    case ViewPreset::Right:
        return {{1.0, 0.0, 0.0}, {0.0, 0.0, 1.0}};
    case ViewPreset::Top:
        return {{0.0, 0.0, 1.0}, {0.0, 1.0, 0.0}};
    case ViewPreset::Bottom:
        return {{0.0, 0.0, -1.0}, {0.0, 1.0, 0.0}};
    case ViewPreset::Isometric:
        break;
    }
    // 等轴测：从 (+1,-1,+1) 方向看，view_up 为 Z 在视平面上的正交投影。
    const Vec backward = normalized({1.0, -1.0, 1.0});
    const Vec up =
        normalized(sub({0.0, 0.0, 1.0}, {backward[0] * backward[2], backward[1] * backward[2],
                                         backward[2] * backward[2]}));
    return {backward, up};
}

std::optional<PickHit> pick_triangle_soup(const SurfaceMeshSnapshot& mesh, const Vec& origin,
                                          const Vec& direction) {
    constexpr double kEpsilon = 1e-12;
    std::optional<PickHit> best;
    double best_t = std::numeric_limits<double>::infinity();
    const auto& v = mesh.vertices;
    for (std::size_t base = 0; base + 2 < v.size(); base += 3) {
        const Vec edge1 = sub(v[base + 1], v[base]);
        const Vec edge2 = sub(v[base + 2], v[base]);
        const Vec p = cross(direction, edge2);
        const double det = dot(edge1, p);
        if (std::abs(det) < kEpsilon)
            continue;
        const double inverse = 1.0 / det;
        const Vec s = sub(origin, v[base]);
        const double u = dot(s, p) * inverse;
        if (u < 0.0 || u > 1.0)
            continue;
        const Vec q = cross(s, edge1);
        const double w = dot(direction, q) * inverse;
        if (w < 0.0 || u + w > 1.0)
            continue;
        const double t = dot(edge2, q) * inverse;
        if (t < 0.0 || t >= best_t)
            continue;
        best_t = t;
        best = PickHit{{origin[0] + direction[0] * t, origin[1] + direction[1] * t,
                        origin[2] + direction[2] * t},
                       base / 3,
                       t};
    }
    return best;
}

} // namespace panta::visualization
