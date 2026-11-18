"""Generate the application icon (PNG + ICO) with Qt. Run: python tools/make_icon.py"""

from __future__ import annotations

import sys
from pathlib import Path

from PySide6.QtCore import QRectF, Qt
from PySide6.QtGui import QColor, QFont, QGuiApplication, QImage, QPainter, QPen

TARGET = Path(__file__).resolve().parent.parent / "src" / "deckellink" / "resources" / "icons"


def render(size: int) -> QImage:
    image = QImage(size, size, QImage.Format.Format_ARGB32)
    image.fill(Qt.GlobalColor.transparent)
    painter = QPainter(image)
    painter.setRenderHint(QPainter.RenderHint.Antialiasing)
    painter.setBrush(QColor("#263238"))
    painter.setPen(Qt.PenStyle.NoPen)
    painter.drawRoundedRect(QRectF(0, 0, size, size), size * 0.16, size * 0.16)
    pen = QPen(QColor("#ffcc00"))
    pen.setWidthF(size * 0.07)
    painter.setPen(pen)
    y = size * 0.72
    painter.drawLine(int(size * 0.18), int(y), int(size * 0.82), int(y))
    painter.drawLine(int(size * 0.66), int(y - size * 0.1), int(size * 0.82), int(y))
    painter.setPen(QColor("#ffffff"))
    font = QFont("DejaVu Sans", 1)
    font.setBold(True)
    font.setPixelSize(int(size * 0.42))
    painter.setFont(font)
    painter.drawText(QRectF(0, size * 0.08, size, size * 0.55), Qt.AlignmentFlag.AlignCenter, "DL")
    painter.end()
    return image


def main() -> int:
    QGuiApplication(sys.argv)
    TARGET.mkdir(parents=True, exist_ok=True)
    render(256).save(str(TARGET / "deckellink.png"), b"PNG")
    if not render(256).save(str(TARGET / "deckellink.ico"), b"ICO"):
        print("ICO writer not available", file=sys.stderr)
        return 1
    print(f"Icons written to {TARGET}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
