from __future__ import annotations

from dataclasses import replace
from pathlib import Path

import pytest

from deckellink.domain.models import (
    CommentHandling,
    LineEnding,
    TabHandling,
    TransferSettings,
    WhitespaceHandling,
)
from deckellink.nc.transformer import TransformError, transform

T = TransferSettings()


def test_crlf_normalization() -> None:
    result = transform(b"N10\nN20\rN30\r\nM30\n", T)
    assert result.data == b"N10\r\nN20\r\nN30\r\nM30\r\n"
    assert any("normalisiert" in a for a in result.applied)


def test_lf_target_line_ending() -> None:
    result = transform(b"N10\r\nM30\r\n", replace(T, line_ending=LineEnding.LF))
    assert result.data == b"N10\nM30\n"


def test_no_normalization_keeps_original_endings() -> None:
    result = transform(b"N10\nN20\r\nM30\r", replace(T, normalize_line_endings=False))
    assert result.data == b"N10\nN20\r\nM30\r"


def test_remove_blank_lines() -> None:
    assert transform(b"N10\r\n\r\n   \r\nM30\r\n", T).data == b"N10\r\nM30\r\n"
    kept = transform(b"N10\r\n\r\nM30\r\n", replace(T, remove_blank_lines=False))
    assert kept.data == b"N10\r\n\r\nM30\r\n"


def test_remove_null_bytes() -> None:
    assert transform(b"\x00\x00N10\x00\r\nM30\r\n", T).data == b"N10\r\nM30\r\n"


def test_m30_never_appended_automatically() -> None:
    result = transform(b"N10 X1\r\n", T)
    assert b"M30" not in result.data
    assert not result.appended_program_end
    # profile allows it, but no explicit confirmation -> still not appended
    allowed = replace(T, append_program_end=True)
    assert b"M30" not in transform(b"N10 X1\r\n", allowed).data


def test_m30_appended_after_explicit_confirmation() -> None:
    allowed = replace(T, append_program_end=True)
    result = transform(b"N10 X1", allowed, append_program_end_confirmed=True)
    assert result.data == b"N10 X1\r\nM30\r\n"
    assert result.appended_program_end
    # not appended twice
    again = transform(b"N10 X1\r\nM30\r\n", allowed, append_program_end_confirmed=True)
    assert again.data.count(b"M30") == 1


def test_m82_never_inserted_without_profile_setting() -> None:
    assert b"M82" not in transform(b"N10\r\nM30\r\n", T).data
    with_start = transform(b"N10\r\nM30\r\n", replace(T, program_start="M82"))
    assert with_start.data.startswith(b"M82\r\n")


def test_original_bytes_unchanged(fixtures_dir: Path) -> None:
    path = fixtures_dir / "messy.nc"
    original = path.read_bytes()
    data = bytes(original)
    transform(data, T)
    assert data == original
    assert path.read_bytes() == original


def test_tabs_replace_and_reject() -> None:
    assert transform(b"N10\tX1\r\nM30\r\n", T).data == b"N10 X1\r\nM30\r\n"
    with pytest.raises(TransformError):
        transform(b"N10\tX1\r\n", replace(T, tab_handling=TabHandling.REJECT))
    kept = transform(b"N10\tX1\r\n", replace(T, tab_handling=TabHandling.KEEP, whitespace=WhitespaceHandling.NONE))
    assert kept.data == b"N10\tX1\r\n"


def test_semicolon_lines_removed_on_request() -> None:
    data = b";KOMMENTAR\r\nN10\r\nM30\r\n"
    assert transform(data, T).data == data
    assert transform(data, replace(T, skip_semicolon_lines=True)).data == b"N10\r\nM30\r\n"


def test_comment_removal() -> None:
    settings = replace(T, comment_handling=CommentHandling.REMOVE)
    result = transform(b"(KOPF)\r\nN10 X1 (ANFAHREN)\r\nM30\r\n", settings)
    assert result.data == b"N10 X1\r\nM30\r\n"


def test_whitespace_handling() -> None:
    assert transform(b"  N10  \r\n", T).data == b"  N10\r\n"
    assert transform(b"  N10  \r\n", replace(T, whitespace=WhitespaceHandling.BOTH)).data == b"N10\r\n"
    assert transform(b"  N10  \r\n", replace(T, whitespace=WhitespaceHandling.NONE)).data == b"  N10  \r\n"


def test_final_newline_appended() -> None:
    assert transform(b"N10\r\nM30", T).data == b"N10\r\nM30\r\n"
    assert transform(b"N10\r\nM30", replace(T, ensure_final_newline=False)).data == b"N10\r\nM30"


def test_non_ascii_rejected() -> None:
    with pytest.raises(TransformError):
        transform("N10 Ä\r\n".encode("latin-1"), T)


def test_line_offsets_and_line_lookup() -> None:
    result = transform(b"AB\r\nCDE\r\nM30\r\n", T)
    assert result.line_end_offsets == (4, 9, 14)
    assert result.line_count == 3
    assert result.line_for_offset(1) == 1
    assert result.line_for_offset(4) == 1
    assert result.line_for_offset(5) == 2
    assert result.line_for_offset(14) == 3
    assert result.line_for_offset(99) == 3
