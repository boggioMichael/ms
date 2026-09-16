"""Compact current-sentence caption bubble for the avatar overlay."""

from PySide6.QtCore import QPoint, QPropertyAnimation, QRect, Qt
from PySide6.QtGui import QColor, QPainter, QPen
from PySide6.QtWidgets import QGraphicsOpacityEffect, QLabel, QWidget


SIDE_OFFSET = 70


class SpeechBubble(QWidget):
    """Show one current spoken sentence near the avatar without taking input."""

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowFlags(Qt.WindowType.Tool | Qt.WindowType.FramelessWindowHint
                            | Qt.WindowType.WindowStaysOnTopHint)
        self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground)
        self.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents)
        self.setFixedSize(260, 66)
        self.label = QLabel(self)
        self.label.setGeometry(16, 9, 228, 43)
        self.label.setWordWrap(True)
        self.label.setStyleSheet('color: #57351F; background: transparent; font-size: 13px;')
        self.label.setTextFormat(Qt.TextFormat.PlainText)
        self.effect = QGraphicsOpacityEffect(self)
        self.setGraphicsEffect(self.effect)
        self.effect.setOpacity(0.0)
        self.animation = QPropertyAnimation(self.effect, b'opacity', self)
        self.animation.setDuration(140)
        self.hiding = False
        self.tail_center_x = self.width() // 2
        self.tail_on_top = False
        self.animation.finished.connect(self._on_fade_finished)

    def set_caption(self, text: str, speaking: bool, enabled: bool) -> None:
        """Present the current sentence only when active playback permits it."""
        if text.strip() and speaking and enabled:
            self.label.setText(text.strip())
            self._fade_to(1.0)
        else:
            self._fade_to(0.0)

    def follow_avatar(self, avatar_geometry: QRect, available_geometry: QRect) -> None:
        """Place the bubble above the avatar while keeping its tail attached."""
        target_x = avatar_geometry.center().x()
        direction = 1 if target_x <= available_geometry.center().x() else -1
        x = target_x - self.width() // 2 + direction * SIDE_OFFSET
        x = max(available_geometry.left(), min(x, available_geometry.right() - self.width() + 1))
        y = avatar_geometry.top() - self.height() + 6
        self.tail_on_top = y < available_geometry.top()
        if self.tail_on_top:
            y = avatar_geometry.bottom() - 6
        y = max(available_geometry.top(), min(y, available_geometry.bottom() - self.height() + 1))
        self.tail_center_x = max(18, min(target_x - x, self.width() - 18))
        self.label.setGeometry(16, 17 if self.tail_on_top else 9, 228, 43)
        self.move(x, y)
        self.update()

    def _fade_to(self, opacity: float) -> None:
        self.animation.stop()
        self.hiding = opacity == 0.0
        if not self.hiding:
            self.show()
            self.raise_()
        self.animation.setStartValue(self.effect.opacity())
        self.animation.setEndValue(opacity)
        self.animation.start()

    def _on_fade_finished(self) -> None:
        if self.hiding and self.effect.opacity() == 0.0:
            self.hide()

    def paintEvent(self, event) -> None:
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing)
        painter.setPen(QPen(QColor('#57351F'), 1.2))
        painter.setBrush(QColor('#FFF4DD'))
        body_top = 11 if self.tail_on_top else 3
        painter.drawRoundedRect(3, body_top, self.width() - 6, self.height() - 11, 7, 7)
        painter.setPen(QPen(QColor('#D99A43'), 1.0))
        inner_top = 14 if self.tail_on_top else 6
        painter.drawRoundedRect(6, inner_top, self.width() - 12, self.height() - 17, 5, 5)
        painter.setPen(Qt.PenStyle.NoPen)
        painter.setBrush(QColor('#FFF4DD'))
        if self.tail_on_top:
            points = [QPoint(self.tail_center_x - 7, 11), QPoint(self.tail_center_x + 7, 11),
                      QPoint(self.tail_center_x, 2)]
        else:
            points = [QPoint(self.tail_center_x - 7, self.height() - 11),
                      QPoint(self.tail_center_x + 7, self.height() - 11),
                      QPoint(self.tail_center_x, self.height() - 2)]
        painter.drawPolygon(points)
