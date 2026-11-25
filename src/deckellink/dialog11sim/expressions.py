"""Arithmetic of DIALOG 11 parameter programming.

Operators ``+ - x /`` (``*`` and ``:`` are accepted as well), brackets, the
functions SIN, COS, TAN, ASIN, ACOS, ATAN (degrees), SQR (square), SQRT, INT,
ABS, RND, FRC, NEG and the constant PI - as listed in the training
documentation (module 11).
"""

from __future__ import annotations

import math
import re
from collections.abc import Callable

_TOKEN_RE = re.compile(
    r"\s*(?:"
    r"(?P<num>\d+\.?\d*|\.\d+)"
    r"|(?P<param>PPT\d+|PP\d+|PT\d+|P\d+)"
    r"|(?P<name>ASIN|ACOS|ATAN|SQRT|SQR|SIN|COS|TAN|INT|ABS|RND|FRC|NEG|PI)"
    r"|(?P<op>[-+*/:X()])"
    r")"
)

FUNCTIONS: dict[str, Callable[[float], float]] = {
    "SIN": lambda v: math.sin(math.radians(v)),
    "COS": lambda v: math.cos(math.radians(v)),
    "TAN": lambda v: math.tan(math.radians(v)),
    "ASIN": lambda v: math.degrees(math.asin(v)),
    "ACOS": lambda v: math.degrees(math.acos(v)),
    "ATAN": lambda v: math.degrees(math.atan(v)),
    "SQR": lambda v: v * v,
    "SQRT": math.sqrt,
    "INT": lambda v: float(math.trunc(v)),
    "ABS": abs,
    "RND": lambda v: float(math.floor(v + 0.5)) if v >= 0 else -float(math.floor(-v + 0.5)),
    "FRC": lambda v: v - math.trunc(v),
    "NEG": lambda v: -v,
}

MAX_DEPTH = 40


class ExpressionError(ValueError):
    """Expression cannot be evaluated."""


def tokenize(text: str) -> list[tuple[str, str]]:
    tokens: list[tuple[str, str]] = []
    text = text.upper().strip()
    pos = 0
    while pos < len(text):
        if text[pos].isspace():
            pos += 1
            continue
        match = _TOKEN_RE.match(text, pos)
        if match is None or match.end() == pos:
            raise ExpressionError(f"Unerwartetes Zeichen '{text[pos]}' im Ausdruck")
        kind = match.lastgroup or ""
        tokens.append((kind, match.group(kind)))
        pos = match.end()
    return tokens


class _Parser:
    def __init__(self, tokens: list[tuple[str, str]], resolve: Callable[[str], float]) -> None:
        self.tokens = tokens
        self.pos = 0
        self.resolve = resolve
        self.depth = 0

    def peek(self) -> tuple[str, str] | None:
        return self.tokens[self.pos] if self.pos < len(self.tokens) else None

    def take(self) -> tuple[str, str]:
        token = self.peek()
        if token is None:
            raise ExpressionError("Ausdruck unvollständig")
        self.pos += 1
        return token

    def expression(self) -> float:
        value = self.term()
        while (token := self.peek()) in (("op", "+"), ("op", "-")):
            assert token is not None
            self.take()
            right = self.term()
            value = value + right if token[1] == "+" else value - right
        return value

    def term(self) -> float:
        value = self.factor()
        while (token := self.peek()) is not None and token[0] == "op" and token[1] in "*/:X":
            self.take()
            right = self.factor()
            if token[1] in "*X":
                value *= right
            else:
                if right == 0:
                    raise ExpressionError("Division durch Null")
                value /= right
        return value

    def factor(self) -> float:
        self.depth += 1
        if self.depth > MAX_DEPTH:
            raise ExpressionError("Ausdruck zu tief verschachtelt")
        try:
            kind, text = self.take()
            if kind == "op" and text in "+-":
                value = self.factor()
                return -value if text == "-" else value
            if kind == "num":
                return float(text)
            if kind == "param":
                return self.resolve(text)
            if kind == "name":
                if text == "PI":
                    return math.pi
                argument = self.factor()
                try:
                    return FUNCTIONS[text](argument)
                except (ValueError, OverflowError) as exc:
                    raise ExpressionError(f"{text}({argument:g}) ist nicht definiert") from exc
            if (kind, text) == ("op", "("):
                value = self.expression()
                if self.take() != ("op", ")"):
                    raise ExpressionError("Schließende Klammer fehlt")
                return value
            raise ExpressionError(f"Unerwartetes Zeichen '{text}' im Ausdruck")
        finally:
            self.depth -= 1


def evaluate(text: str, resolve: Callable[[str], float]) -> float:
    """Evaluate ``text``; parameter references are resolved via ``resolve``."""
    parser = _Parser(tokenize(text), resolve)
    value = parser.expression()
    if parser.peek() is not None:
        raise ExpressionError(f"Unerwarteter Rest im Ausdruck: '{parser.peek()[1]}'")  # type: ignore[index]
    if math.isnan(value) or math.isinf(value):
        raise ExpressionError("Ergebnis ist keine gültige Zahl")
    return value


def is_arithmetic(text: str) -> bool:
    """True if ``text`` is a number or a bracketed/parameter expression (not a word chain)."""
    stripped = text.strip().upper()
    if stripped.startswith("("):
        return True
    return bool(re.fullmatch(r"[-+]?(?:\d+\.?\d*|\.\d+)|[-+]?(?:PPT|PP|PT|P)\d+", stripped))
