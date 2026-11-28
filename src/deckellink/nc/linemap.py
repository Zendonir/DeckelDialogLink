"""Mapping of transfer data lines back to the lines of the original file."""

from __future__ import annotations

SEARCH_WINDOW = 200


def _lines(data: bytes) -> list[str]:
    text = data.decode("ascii", errors="replace").replace("\r\n", "\n").replace("\r", "\n")
    return [line.strip(" \t\x00") for line in text.split("\n")]


def map_payload_lines(original: bytes, payload: bytes) -> list[int]:
    """``result[i]`` = 1-based line number in ``original`` for payload line ``i + 1``.

    Lines are matched in order (the transformer only removes, trims or inserts
    lines). Inserted lines map to the previous original line.
    """
    source = _lines(original)
    mapping: list[int] = []
    position = 0
    last = 0
    for content in _lines(payload):
        found = None
        for index in range(position, min(position + SEARCH_WINDOW, len(source))):
            if source[index] == content:
                found = index
                break
        if found is not None:
            last = found + 1
            position = found + 1
        mapping.append(max(last, 1))
    return mapping
