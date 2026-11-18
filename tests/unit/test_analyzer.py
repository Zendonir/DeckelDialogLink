from __future__ import annotations

from dataclasses import replace
from pathlib import Path

from deckellink.domain.models import SerialSettings, TabHandling, TransferSettings
from deckellink.domain.results import Severity
from deckellink.nc.analyzer import analyze

T = TransferSettings()


def codes(data: bytes, settings: TransferSettings = T) -> dict[str, Severity]:
    return {f.code: f.severity for f in analyze(data, settings).findings}


def test_valid_program_has_no_findings(fixtures_dir: Path) -> None:
    data = (fixtures_dir / "valid.nc").read_bytes()
    result = analyze(data, T)
    assert result.findings == ()
    assert result.has_program_end
    assert result.line_count == 7
    assert result.line_ending_counts["crlf"] == 7
    assert not result.has_errors and not result.has_warnings


def test_analysis_does_not_modify_input(fixtures_dir: Path) -> None:
    data = (fixtures_dir / "messy.nc").read_bytes()
    copy = bytes(data)
    analyze(data, T)
    assert data == copy


def test_non_printable_and_umlaut_is_error(fixtures_dir: Path) -> None:
    result = analyze((fixtures_dir / "invalid_chars.nc").read_bytes(), T)
    assert result.has_errors
    non_ascii = result.get("NON_ASCII")
    control = result.get("CONTROL_CHARS")
    assert non_ascii is not None and non_ascii.severity is Severity.ERROR
    assert non_ascii.lines == (1,)
    assert control is not None and control.lines == (2,)


def test_seven_data_bits_limit_respected() -> None:
    result = analyze(b"N10 X\x7e\r\nM30\r\n", T, SerialSettings(data_bits=6))
    finding = result.get("NON_ASCII")
    assert finding is not None
    assert "6 Datenbits" in finding.message


def test_null_bytes_warning_when_stripped_error_otherwise() -> None:
    data = b"N10 X1\x00\r\nM30\r\n"
    assert codes(data)["NULL_BYTES"] is Severity.WARNING
    assert codes(data, replace(T, strip_null_bytes=False))["NULL_BYTES"] is Severity.ERROR


def test_mixed_line_endings_detected() -> None:
    result = analyze(b"N10\r\nN20\nN30\rM30\r\n", T)
    assert result.has("MIXED_LINE_ENDINGS")
    assert result.line_ending_counts == {"crlf": 2, "lf": 1, "cr": 1}
    assert (
        codes(b"N10\r\nN20\nM30\r\n", replace(T, normalize_line_endings=False))["MIXED_LINE_ENDINGS"]
        is Severity.WARNING
    )


def test_foreign_consistent_line_endings_info() -> None:
    assert codes(b"N10\nM30\n")["FOREIGN_LINE_ENDINGS"] is Severity.INFO


def test_missing_m30_warning() -> None:
    result = analyze(b"N10 G90\r\nN20 X1\r\n", T)
    assert not result.has_program_end
    assert result.get("MISSING_PROGRAM_END") is not None
    assert result.get("MISSING_PROGRAM_END").severity is Severity.WARNING  # type: ignore[union-attr]


def test_m30_word_boundaries() -> None:
    assert not analyze(b"N10 M300\r\n", T).has_program_end
    assert not analyze(b"N10 XM30\r\n", T).has_program_end
    assert analyze(b"N10 m30\r\n", T).has_program_end
    assert analyze(b"N10M30\r\n", T).has_program_end
    # inside a comment does not count
    assert not analyze(b"(M30)\r\nN10 X1\r\n", T).has_program_end


def test_data_after_m30() -> None:
    result = analyze(b"N10 M30\r\nN20 X5\r\n\r\n", T)
    finding = result.get("DATA_AFTER_PROGRAM_END")
    assert finding is not None
    assert finding.lines == (2,)


def test_empty_file_is_error() -> None:
    result = analyze(b"", T)
    assert result.has_errors
    assert result.findings[0].code == "EMPTY_FILE"


def test_semicolon_lines() -> None:
    data = b";KOMMENTAR\r\nN10 M30\r\n"
    assert codes(data)["SEMICOLON_LINES"] is Severity.WARNING
    assert codes(data, replace(T, skip_semicolon_lines=True))["SEMICOLON_LINES"] is Severity.INFO


def test_tabs_according_to_profile() -> None:
    data = b"N10\tX1\r\nM30\r\n"
    assert codes(data)["TABS"] is Severity.INFO
    assert codes(data, replace(T, tab_handling=TabHandling.REJECT))["TABS"] is Severity.ERROR
    assert codes(data, replace(T, tab_handling=TabHandling.KEEP))["TABS"] is Severity.WARNING


def test_long_lines() -> None:
    data = b"N10 " + b"X1" * 80 + b"\r\nM30\r\n"
    finding = analyze(data, T).get("LONG_LINES")
    assert finding is not None and finding.lines == (1,)
    assert not analyze(data, replace(T, max_line_length=0)).has("LONG_LINES")


def test_very_large_file_warning() -> None:
    data = b"N10 X1\r\n" * 100 + b"M30\r\n"
    assert codes(data, replace(T, max_program_bytes=100))["FILE_TOO_LARGE"] is Severity.WARNING


def test_missing_final_newline() -> None:
    result = analyze(b"N10 X1\r\nM30", T)
    finding = result.get("MISSING_FINAL_NEWLINE")
    assert finding is not None and finding.severity is Severity.INFO
    assert codes(b"M30", replace(T, ensure_final_newline=False))["MISSING_FINAL_NEWLINE"] is Severity.WARNING


def test_blank_lines_and_whitespace() -> None:
    result = analyze(b"N10 X1  \r\n\r\n  \r\nM30\r\n", T)
    blank = result.get("BLANK_LINES")
    assert blank is not None and blank.lines == (2, 3)
    assert result.has("SURROUNDING_WHITESPACE")


def test_start_sequence_warning() -> None:
    assert codes(b"M30\r\n", replace(T, program_start="M82"))["UNVERIFIED_START_SEQUENCE"] is Severity.WARNING


def test_findings_sorted_errors_first(fixtures_dir: Path) -> None:
    result = analyze((fixtures_dir / "invalid_chars.nc").read_bytes() + b"\x00", T)
    ranks = [f.severity.rank for f in result.findings]
    assert ranks == sorted(ranks, reverse=True)


def test_finding_lines_text_truncates() -> None:
    data = b"".join(b"N%d \x00\r\n" % i for i in range(15)) + b"M30\r\n"
    finding = analyze(data, T).get("NULL_BYTES")
    assert finding is not None
    assert "(+5 weitere)" in finding.lines_text
    assert finding.to_dict()["count"] == 15
