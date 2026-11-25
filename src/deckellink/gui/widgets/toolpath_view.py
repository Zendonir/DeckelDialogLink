"""Graphic display of simulated tool paths (plane views and a rotatable 3D view)."""

from __future__ import annotations

import math
from enum import StrEnum

from PySide6.QtCore import QPointF, QRectF, Qt, Signal
from PySide6.QtGui import (
    QColor,
    QMouseEvent,
    QPainter,
    QPainterPath,
    QPaintEvent,
    QPen,
    QResizeEvent,
    QWheelEvent,
)
from PySide6.QtWidgets import QWidget

from deckellink.dialog11sim.geometry import Vec3
from deckellink.dialog11sim.model import MoveKind, SimResult

BACKGROUND = QColor("#0f1b2d")
GRID = QColor("#22344f")
AXIS_COLORS = (QColor("#ff6b6b"), QColor("#51cf66"), QColor("#4dabf7"))
RAPID = QColor("#f59f00")
FEED = QColor("#74c0fc")
DONE = QColor("#69db7c")
PROGRAMMED = QColor("#adb5bd")
TOOL = QColor("#ffffff")
TEXT = QColor("#e9ecef")


class ViewMode(StrEnum):
    XY = "xy"
    XZ = "xz"
    YZ = "yz"
    ISO = "iso"

    @property
    def label(self) -> str:
        return {
            "xy": "Draufsicht XY",
            "xz": "Vorderansicht XZ",
            "yz": "Seitenansicht YZ",
            "iso": "3D-Ansicht",
        }[self.value]


class ToolpathView(QWidget):
    """Draws the result of a simulation; the current tool position is set from outside."""

    view_changed = Signal(str)

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setMinimumSize(320, 240)
        self.setFocusPolicy(Qt.FocusPolicy.StrongFocus)
        self.setMouseTracking(True)
        self.setToolTip("Mausrad: Zoom, linke Maustaste: Verschieben, rechte Maustaste (3D): Drehen")
        self.result: SimResult | None = None
        self.mode = ViewMode.ISO
        self.yaw = -35.0
        self.pitch = 30.0
        self.zoom = 1.0
        self.pan = QPointF(0.0, 0.0)
        self.show_rapid = True
        self.show_programmed = True
        self.tool_position: Vec3 | None = None
        self.tool_radius = 0.0
        self.done_moves = 0
        self.done_point: Vec3 | None = None
        self.manual_trace: list[Vec3] = []
        self._drag: QPointF | None = None
        self._drag_button = Qt.MouseButton.NoButton
        self._cache_key: tuple[object, ...] | None = None
        self._paths: dict[str, QPainterPath] = {}
        self._done_cache: tuple[int, QPainterPath] = (0, QPainterPath())
        self._bounds: tuple[Vec3, Vec3] = ((-50.0, -50.0, -10.0), (50.0, 50.0, 50.0))

    # ------------------------------------------------------------------ data
    def set_result(self, result: SimResult | None) -> None:
        self.result = result
        self._cache_key = None
        self._done_cache = (0, QPainterPath())
        bounds = result.bounds() if result is not None else None
        if bounds is not None:
            self._bounds = bounds
        self.done_moves = 0
        self.done_point = None
        self.update()

    def set_mode(self, mode: ViewMode) -> None:
        self.mode = mode
        self._cache_key = None
        self._done_cache = (0, QPainterPath())
        self.view_changed.emit(mode.label)
        self.update()

    def cycle_mode(self) -> ViewMode:
        modes = list(ViewMode)
        self.set_mode(modes[(modes.index(self.mode) + 1) % len(modes)])
        return self.mode

    def set_tool(self, position: Vec3 | None, radius: float, done_moves: int) -> None:
        self.tool_position = position
        self.tool_radius = radius
        if done_moves < self._done_cache[0]:
            self._done_cache = (0, QPainterPath())
        self.done_moves = done_moves
        self.done_point = position
        self.update()

    def fit(self) -> None:
        self.zoom = 1.0
        self.pan = QPointF(0.0, 0.0)
        self.update()

    def set_bounds(self, lo: Vec3, hi: Vec3) -> None:
        self._bounds = (lo, hi)
        self._cache_key = None
        self.update()

    # ------------------------------------------------------------------ projection
    def project(self, p: Vec3) -> tuple[float, float]:
        """Model coordinates -> 2D view coordinates (y up, model units)."""
        x, y, z = p
        if self.mode is ViewMode.XY:
            return x, y
        if self.mode is ViewMode.XZ:
            return x, z
        if self.mode is ViewMode.YZ:
            return y, z
        yaw, pitch = math.radians(self.yaw), math.radians(self.pitch)
        rx = x * math.cos(yaw) - y * math.sin(yaw)
        ry = x * math.sin(yaw) + y * math.cos(yaw)
        return rx, z * math.cos(pitch) + ry * math.sin(pitch)

    def _view_rect(self) -> QRectF:
        lo, hi = self._bounds
        corners = [self.project((x, y, z)) for x in (lo[0], hi[0]) for y in (lo[1], hi[1]) for z in (lo[2], hi[2])]
        xs = [c[0] for c in corners]
        ys = [c[1] for c in corners]
        width = max(max(xs) - min(xs), 1.0)
        height = max(max(ys) - min(ys), 1.0)
        return QRectF(min(xs), min(ys), width, height)

    def _scale_and_origin(self) -> tuple[float, QPointF]:
        rect = self._view_rect()
        margin = 30.0
        avail_w = max(self.width() - 2 * margin, 10.0)
        avail_h = max(self.height() - 2 * margin, 10.0)
        scale = min(avail_w / rect.width(), avail_h / rect.height()) * self.zoom
        center = rect.center()
        origin = QPointF(
            self.width() / 2 - center.x() * scale + self.pan.x(), self.height() / 2 + center.y() * scale + self.pan.y()
        )
        return scale, origin

    def to_screen(self, p: Vec3) -> QPointF:
        scale, origin = self._scale_and_origin()
        u, v = self.project(p)
        return QPointF(origin.x() + u * scale, origin.y() - v * scale)

    # ------------------------------------------------------------------ path cache (model units)
    def _ensure_paths(self) -> None:
        key = (id(self.result), self.mode, round(self.yaw, 2), round(self.pitch, 2))
        if key == self._cache_key:
            return
        self._cache_key = key
        self._paths = {"rapid": QPainterPath(), "feed": QPainterPath(), "programmed": QPainterPath()}
        if self.result is None:
            return
        for move in self.result.moves:
            path = self._paths["rapid" if move.kind is MoveKind.RAPID else "feed"]
            self._add_polyline(path, move.points)
        for move in self.result.programmed:
            if move.kind is MoveKind.FEED:
                self._add_polyline(self._paths["programmed"], move.points)

    def _add_polyline(self, path: QPainterPath, points: tuple[Vec3, ...] | list[Vec3]) -> None:
        first = True
        for p in points:
            u, v = self.project(p)
            if first:
                path.moveTo(u, -v)
                first = False
            else:
                path.lineTo(u, -v)

    def _done_path(self) -> QPainterPath:
        if self.result is None:
            return QPainterPath()
        count, path = self._done_cache
        if count > self.done_moves:
            count, path = 0, QPainterPath()
        moves = self.result.moves
        for index in range(count, min(self.done_moves, len(moves))):
            if moves[index].kind is MoveKind.FEED:
                self._add_polyline(path, moves[index].points)
        self._done_cache = (min(self.done_moves, len(moves)), path)
        return path

    # ------------------------------------------------------------------ painting
    def paintEvent(self, event: QPaintEvent) -> None:
        del event
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing)
        painter.fillRect(self.rect(), BACKGROUND)
        scale, origin = self._scale_and_origin()
        self._draw_grid(painter, scale, origin)
        self._draw_axes(painter, scale, origin)
        self._ensure_paths()
        painter.save()
        painter.translate(origin)
        painter.scale(scale, scale)

        def pen(color: QColor, width: float, style: Qt.PenStyle = Qt.PenStyle.SolidLine) -> QPen:
            p = QPen(color, width / scale, style)
            p.setCosmetic(False)
            return p

        if self.show_programmed and self.mode is not ViewMode.ISO:
            painter.setPen(pen(PROGRAMMED, 1.0, Qt.PenStyle.DotLine))
            painter.drawPath(self._paths.get("programmed", QPainterPath()))
        if self.show_rapid:
            painter.setPen(pen(RAPID, 1.2, Qt.PenStyle.DashLine))
            painter.drawPath(self._paths.get("rapid", QPainterPath()))
        painter.setPen(pen(FEED, 1.4))
        painter.drawPath(self._paths.get("feed", QPainterPath()))
        painter.setPen(pen(DONE, 2.4))
        painter.drawPath(self._done_path())
        if self.result is not None and self.tool_position is not None and 0 <= self.done_moves < len(self.result.moves):
            current = self.result.moves[self.done_moves]
            if current.kind is MoveKind.FEED:
                partial = QPainterPath()
                self._add_polyline(partial, [current.start, self.tool_position])
                painter.drawPath(partial)
        if self.manual_trace:
            trace = QPainterPath()
            self._add_polyline(trace, self.manual_trace)
            painter.setPen(pen(QColor("#e599f7"), 1.6))
            painter.drawPath(trace)
        painter.restore()
        self._draw_tool(painter)
        self._draw_legend(painter)
        painter.end()

    def _draw_grid(self, painter: QPainter, scale: float, origin: QPointF) -> None:
        if self.mode is ViewMode.ISO:
            return
        step = _nice_step(60.0 / max(scale, 1e-9))
        painter.setPen(QPen(GRID, 1))
        left = (0 - origin.x()) / scale
        right = (self.width() - origin.x()) / scale
        top = (origin.y() - 0) / scale
        bottom = (origin.y() - self.height()) / scale
        x = math.floor(left / step) * step
        while x <= right:
            sx = origin.x() + x * scale
            painter.drawLine(QPointF(sx, 0), QPointF(sx, self.height()))
            x += step
        y = math.floor(bottom / step) * step
        while y <= top:
            sy = origin.y() - y * scale
            painter.drawLine(QPointF(0, sy), QPointF(self.width(), sy))
            y += step
        painter.setPen(TEXT)
        painter.drawText(8, self.height() - 8, f"Raster {step:g} mm")

    def _draw_axes(self, painter: QPainter, scale: float, origin: QPointF) -> None:
        length = 40.0 / max(scale, 1e-9)
        zero = self.to_screen((0.0, 0.0, 0.0))
        for axis, color in enumerate(AXIS_COLORS):
            end = [0.0, 0.0, 0.0]
            end[axis] = length
            tip = self.to_screen((end[0], end[1], end[2]))
            if abs(tip.x() - zero.x()) < 1 and abs(tip.y() - zero.y()) < 1:
                continue
            painter.setPen(QPen(color, 2))
            painter.drawLine(zero, tip)
            painter.drawText(tip + QPointF(4, -4), "XYZ"[axis])
        del origin

    def _draw_tool(self, painter: QPainter) -> None:
        if self.tool_position is None:
            return
        scale, _origin = self._scale_and_origin()
        center = self.to_screen(self.tool_position)
        painter.setPen(QPen(TOOL, 2))
        painter.setBrush(Qt.BrushStyle.NoBrush)
        if self.tool_radius > 0 and self.mode is ViewMode.XY:
            r = self.tool_radius * scale
            painter.drawEllipse(center, r, r)
        elif self.tool_radius > 0 and self.mode in (ViewMode.XZ, ViewMode.YZ):
            r = self.tool_radius * scale
            painter.drawRect(QRectF(center.x() - r, center.y() - 3 * r, 2 * r, 3 * r))
        painter.drawLine(center + QPointF(-8, 0), center + QPointF(8, 0))
        painter.drawLine(center + QPointF(0, -8), center + QPointF(0, 8))

    def _draw_legend(self, painter: QPainter) -> None:
        painter.setPen(TEXT)
        painter.drawText(8, 18, self.mode.label)
        items = (("Vorschub", FEED), ("Eilgang", RAPID), ("bearbeitet", DONE))
        x = 8
        for text, color in items:
            painter.fillRect(QRectF(x, 26, 14, 4), color)
            painter.drawText(x + 18, 32, text)
            x += 100

    # ------------------------------------------------------------------ interaction
    def wheelEvent(self, event: QWheelEvent) -> None:
        factor = 1.15 if event.angleDelta().y() > 0 else 1 / 1.15
        self.zoom = min(max(self.zoom * factor, 0.05), 200.0)
        self.update()

    def mousePressEvent(self, event: QMouseEvent) -> None:
        self._drag = event.position()
        self._drag_button = event.button()

    def mouseMoveEvent(self, event: QMouseEvent) -> None:
        if self._drag is None:
            return
        delta = event.position() - self._drag
        self._drag = event.position()
        if self._drag_button == Qt.MouseButton.RightButton and self.mode is ViewMode.ISO:
            self.yaw += delta.x() * 0.5
            self.pitch = min(max(self.pitch - delta.y() * 0.5, -89.0), 89.0)
            self._done_cache = (0, QPainterPath())
        else:
            self.pan += delta
        self.update()

    def mouseReleaseEvent(self, event: QMouseEvent) -> None:
        del event
        self._drag = None

    def mouseDoubleClickEvent(self, event: QMouseEvent) -> None:
        del event
        self.fit()

    def resizeEvent(self, event: QResizeEvent) -> None:
        super().resizeEvent(event)
        self.update()


def _nice_step(raw: float) -> float:
    if raw <= 0:
        return 1.0
    exponent = math.floor(math.log10(raw))
    base = raw / 10**exponent
    nice = 1 if base < 1.5 else 2 if base < 3.5 else 5 if base < 7.5 else 10
    return float(nice * 10**exponent)
