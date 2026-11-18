"""Low-level byte helpers shared by analyzer, transformer and receiver."""

from __future__ import annotations

import re
from dataclasses import dataclass

_LINE_RE = re.compile(rb"([^\r\n]*)(\r\n|\r|\n)")

CR = 0x0D
LF = 0x0A
TAB = 0x09
NUL = 0x00
DEL = 0x7F


@dataclass(frozen=True)
class RawLine:
    number: int
    content: bytes
    ending: bytes


def split_lines(data: bytes) -> list[RawLine]:
    """Split ``data`` into lines, preserving the exact line ending of each line.

    CRLF, lone CR and lone LF are all recognised. A trailing fragment without a
    line ending becomes a last line with an empty ``ending``.
    """
    lines: list[RawLine] = []
    pos = 0
    for match in _LINE_RE.finditer(data):
        lines.append(RawLine(len(lines) + 1, match.group(1), match.group(2)))
        pos = match.end()
    if pos < len(data):
        lines.append(RawLine(len(lines) + 1, data[pos:], b""))
    return lines


def ending_name(ending: bytes) -> str:
    return {b"\r\n": "crlf", b"\n": "lf", b"\r": "cr"}.get(ending, "none")


def count_line_endings(lines: list[RawLine]) -> dict[str, int]:
    counts = {"crlf": 0, "lf": 0, "cr": 0}
    for line in lines:
        name = ending_name(line.ending)
        if name in counts:
            counts[name] += 1
    return counts


def program_end_pattern(token: str) -> re.Pattern[bytes] | None:
    """Regex that finds ``token`` (e.g. ``M30``) as a stand-alone NC word.

    ``M30`` matches in ``N100 M30`` and ``N100M30`` but not in ``M300`` or ``XM30``.
    """
    cleaned = token.strip()
    if not cleaned:
        return None
    escaped = re.escape(cleaned.encode("ascii", errors="replace"))
    return re.compile(rb"(?<![A-Za-z.])" + escaped + rb"(?![0-9])", re.IGNORECASE)


def remove_comments(content: bytes, open_char: str, close_char: str) -> bytes:
    """Remove comment spans such as ``(TEXT)``. Unterminated comments run to line end."""
    if not open_char:
        return content
    open_b = open_char.encode("ascii", errors="replace")
    close_b = close_char.encode("ascii", errors="replace")
    result = bytearray()
    i = 0
    while i < len(content):
        start = content.find(open_b, i)
        if start < 0:
            result += content[i:]
            break
        result += content[i:start]
        if not close_b:
            break
        end = content.find(close_b, start + len(open_b))
        if end < 0:
            break
        i = end + len(close_b)
    return bytes(result)


def is_semicolon_line(content: bytes) -> bool:
    return content.lstrip(b" \t").startswith(b";")


def is_blank(content: bytes) -> bool:
    return content.strip(b" \t\x00") == b""


def max_value_for_data_bits(data_bits: int) -> int:
    """Highest byte value that fits in a character of ``data_bits`` (capped at ASCII)."""
    return min(0x7F, (1 << data_bits) - 1)


def is_disallowed_control(byte: int) -> bool:
    """Control characters other than CR, LF, TAB and NUL (handled separately)."""
    return (byte < 0x20 and byte not in (CR, LF, TAB, NUL)) or byte == DEL


def format_ascii(data: bytes) -> str:
    """Printable ASCII representation with visible control characters."""
    names = {
        CR: "<CR>",
        LF: "<LF>",
        TAB: "<TAB>",
        NUL: "<NUL>",
        0x02: "<STX>",
        0x03: "<ETX>",
        0x04: "<EOT>",
        0x05: "<ENQ>",
        0x10: "<DLE>",
        0x15: "<NAK>",
    }
    out = []
    for b in data:
        if b in names:
            out.append(names[b])
        elif 0x20 <= b < 0x7F:
            out.append(chr(b))
        else:
            out.append(f"<{b:02X}>")
    return "".join(out)


def format_hex(data: bytes) -> str:
    return " ".join(f"{b:02X}" for b in data)
