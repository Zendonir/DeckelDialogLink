"""Transformation of NC data into transfer bytes.

The transformer works on a copy. The original bytes and the original file are
never modified. The result is a separate :class:`TransformResult`.
"""

from __future__ import annotations

from deckellink.domain.errors import DeckelLinkError
from deckellink.domain.models import (
    CommentHandling,
    TabHandling,
    TransferSettings,
    WhitespaceHandling,
)
from deckellink.domain.results import TransformResult
from deckellink.nc.checks import (
    is_blank,
    is_semicolon_line,
    primary_program_end,
    program_end_pattern,
    remove_comments,
    split_eot_trailer,
    split_lines,
)


class TransformError(DeckelLinkError):
    """The data cannot be transformed according to the profile."""


def transform(
    data: bytes,
    settings: TransferSettings,
    *,
    append_program_end_confirmed: bool = False,
) -> TransformResult:
    """Create transfer bytes from ``data``.

    ``append_program_end_confirmed`` must be ``True`` *and* the profile option
    ``append_program_end`` enabled before a missing program end is appended.
    """
    applied: list[str] = []
    work, trailer = split_eot_trailer(data)
    if settings.strip_null_bytes and b"\x00" in work:
        work = work.replace(b"\x00", b"")
        applied.append("Nullbytes entfernt")

    target_ending = settings.line_ending.sequence
    out_lines: list[tuple[bytes, bytes]] = []
    removed_blank = removed_semicolon = replaced_tabs = trimmed = removed_comments = 0
    changed_endings = 0

    for line in split_lines(work):
        content = line.content
        if b"\t" in content:
            if settings.tab_handling is TabHandling.REJECT:
                raise TransformError(f"Zeile {line.number} enthält Tabulatoren (laut Profil unzulässig).")
            if settings.tab_handling is TabHandling.REPLACE:
                content = content.replace(b"\t", b" ")
                replaced_tabs += 1
        if settings.comment_handling is CommentHandling.REMOVE:
            stripped = remove_comments(content, settings.comment_open, settings.comment_close)
            if stripped != content:
                removed_comments += 1
                if is_blank(stripped) and not is_blank(content):
                    continue
                content = stripped
        if settings.whitespace is WhitespaceHandling.TRAILING:
            new = content.rstrip(b" \t")
        elif settings.whitespace is WhitespaceHandling.BOTH:
            new = content.strip(b" \t")
        else:
            new = content
        if new != content:
            trimmed += 1
            content = new
        if settings.skip_semicolon_lines and is_semicolon_line(content):
            removed_semicolon += 1
            continue
        if is_blank(content):
            if line.ending == b"" and content == b"":
                continue
            if settings.remove_blank_lines:
                removed_blank += 1
                continue
        ending = line.ending
        if settings.normalize_line_endings and ending and ending != target_ending:
            ending = target_ending
            changed_endings += 1
        out_lines.append((content, ending))

    for count, text in (
        (replaced_tabs, "Tabulatoren durch Leerzeichen ersetzt"),
        (removed_comments, "Kommentare entfernt"),
        (trimmed, "Leerzeichen am Zeilenrand entfernt"),
        (removed_semicolon, "Semikolonzeilen entfernt"),
        (removed_blank, "Leerzeilen entfernt"),
        (changed_endings, f"Zeilenenden auf {settings.line_ending.value.upper()} normalisiert"),
    ):
        if count:
            applied.append(f"{text} ({count})")

    start = settings.program_start.strip()
    if start:
        start_b = start.encode("ascii", errors="strict")
        if not out_lines or out_lines[0][0].strip() != start_b:
            out_lines.insert(0, (start_b, target_ending))
            applied.append(f"Startsequenz '{start}' eingefügt")

    appended = False
    end_re = program_end_pattern(settings.program_end)
    if end_re is not None and settings.append_program_end and append_program_end_confirmed:
        has_end = any(
            end_re.search(remove_comments(c, settings.comment_open, settings.comment_close)) for c, _ in out_lines
        )
        if not has_end:
            if out_lines and out_lines[-1][1] == b"":
                out_lines[-1] = (out_lines[-1][0], target_ending)
            end_word = primary_program_end(settings.program_end)
            out_lines.append((end_word.encode("ascii"), target_ending))
            appended = True
            applied.append(f"Programmende '{end_word}' ergänzt (bestätigt)")

    if settings.ensure_final_newline and out_lines and out_lines[-1][1] == b"":
        out_lines[-1] = (out_lines[-1][0], target_ending)
        applied.append("Abschließenden Zeilenumbruch ergänzt")

    buffer = bytearray()
    offsets: list[int] = []
    for content, ending in out_lines:
        buffer += content
        buffer += ending
        offsets.append(len(buffer))
    if trailer:
        if settings.send_eot_trailer:
            buffer += b"\x04"
            if b"\n" in trailer or b"\r" in trailer:
                buffer += target_ending
            applied.append("Übertragungsende EOT am Dateiende wird mitgesendet")
        else:
            applied.append("Übertragungsende EOT am Dateiende entfernt")
    result = bytes(buffer)

    if settings.encoding.lower() == "ascii":
        bad = next((i for i, b in enumerate(result) if b > 0x7F), None)
        if bad is not None:
            raise TransformError(f"Byte 0x{result[bad]:02X} an Position {bad} ist kein 7-Bit-ASCII-Zeichen.")

    return TransformResult(
        data=result,
        line_count=len(out_lines),
        line_end_offsets=tuple(offsets),
        applied=tuple(applied),
        appended_program_end=appended,
    )
