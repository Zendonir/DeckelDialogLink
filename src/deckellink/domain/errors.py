"""User-facing error descriptions.

Technical exceptions are translated into German messages with probable causes.
The technical details remain available in a collapsible section and the log.
"""

from __future__ import annotations

from dataclasses import dataclass, field


@dataclass(frozen=True)
class UserFacingError:
    title: str
    causes: tuple[str, ...] = ()
    technical_details: str = ""
    operation: str = ""
    exception_type: str = ""

    def as_text(self) -> str:
        lines = [self.title]
        if self.causes:
            lines.append("")
            lines.append("Mögliche Ursachen:")
            lines.extend(f"- {cause}" for cause in self.causes)
        if self.technical_details:
            lines.append("")
            lines.append("Technische Details:")
            lines.append(self.technical_details)
        return "\n".join(lines)


class DeckelLinkError(Exception):
    """Base class for application specific errors."""


@dataclass
class FieldError:
    field: str
    message: str

    def __str__(self) -> str:
        return f"{self.field}: {self.message}"


class ProfileValidationError(DeckelLinkError):
    def __init__(self, errors: list[FieldError], source: str = "") -> None:
        self.errors = errors
        self.source = source
        joined = "; ".join(str(e) for e in errors)
        super().__init__(f"Ungültiges Profil {source}: {joined}".strip())


@dataclass
class ValidationBlockedError(DeckelLinkError):
    """Raised if a transfer is attempted while blocking findings exist."""

    reasons: list[str] = field(default_factory=list)

    def __str__(self) -> str:
        return "; ".join(self.reasons)
