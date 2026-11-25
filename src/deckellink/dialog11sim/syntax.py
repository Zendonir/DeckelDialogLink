"""Lexical structure of DIALOG 11 program text.

Observed syntax (training documentation, not a formal specification):

* block ``N10 G1 X10 YI-5 F200; M3`` - several sub-blocks separated by ``;``,
* word = address letters + value; ``A``/``I`` after a coordinate address means
  absolute/incremental for this word (``XI-40``, ``XA50``),
* values may be parameters: ``XP1``, ``Z-P3``, ``XPP4``, ``XPT3``,
* G functions with a variant: ``G87 • 2`` (printed with a dot operator; written
  here as ``G87.2``, ``G87*2`` or ``G87+2``),
* subprograms ``%*1`` (of a main program), ``%%4`` (macro), ``%%*1``,
* parameter assignment ``P1 = (P2 + 5)``, calculation ``P4!``,
  ``IF P1 >= 3 THEN L0 N90 ELSE G0 Z100``,
* comments in round brackets.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field

# Address names (longest match wins). Coordinate-like addresses also exist with
# the A (absolute) / I (incremental) suffix.
_BASE_SUFFIXABLE = (
    "X",
    "Y",
    "Z",
    "C",
    "I",
    "J",
    "K",
    "PX",
    "PY",
    "PZ",
    "PW",
    "W",
    "O",
    "T",
    "S",
    "E",
    "LX",
    "LY",
    "BW",
    "EW",
    "M",
    "D",
)
_PLAIN = (
    "N",
    "G",
    "F",
    "TT",
    "L",
    "R",
    "PL",
    "UW",
    "UR",
    "FA",
    "AM",
    "FE",
    "FZ",
    "LI",
    "BI",
    "DW",
    "RL",
    "RB",
    "RT",
    "DR",
    "DN",
    "ST",
    "LL",
    "LW",
    "LA",
    "LB",
    "LC",
    "BL",
    "BX",
    "BY",
    "BA",
    "BB",
    "BC",
    "B",
    "SL",
    "SH",
    "AW",
    "TW",
    "A",
    "H",
    "Q",
    "U",
    "V",
)
ADDRESSES: frozenset[str] = frozenset(
    set(_PLAIN) | set(_BASE_SUFFIXABLE) | {a + s for a in _BASE_SUFFIXABLE for s in ("A", "I")}
)

_COMMENT_RE = re.compile(r"\([^()]*\)")
_G_VARIANT_RE = re.compile(r"G\s*(\d+)\s*[.*+•·∙×]\s*(\d+)")
_SUB_RE = re.compile(r"(%%?)\s*([*+•·∙×.])?\s*(\d+)")
_WORD_RE = re.compile(
    r"([A-Z]+)"
    r"([-+]?(?:PPT|PP|PT|P)\d+"  # parameter reference
    r"|[-+]?(?:\d+\.?\d*|\.\d+))?"  # number
)
_PARAM_SUFFIXES = ("PPT", "PP", "PT", "P")
_ASSIGN_RE = re.compile(r"^(PP|P)(\d+)\s*=\s*(.*)$")
_CALC_RE = re.compile(r"^(PP|P)(\d+)\s*!$")
_IF_RE = re.compile(r"^IF\s*(.+?)\s*THEN\s*(.+?)(?:\s*ELSE\s*(.+))?$")
HEADER_RE = re.compile(r"^\s*(%%?)\s*([*+•·∙×.])?\s*(\d+)\s*$")


class SyntaxProblem(ValueError):
    """A sub-block could not be tokenized."""


@dataclass(frozen=True)
class SubprogramRef:
    """``%*1`` -> ("sub", 1); ``%%4`` -> ("macro", 4); ``%%*1`` -> ("macro_sub", 1); ``%3`` -> ("main", 3)."""

    kind: str
    number: int

    @property
    def label(self) -> str:
        return {
            "sub": f"%*{self.number}",
            "macro": f"%%{self.number}",
            "macro_sub": f"%%*{self.number}",
            "main": f"%{self.number}",
        }[self.kind]


def subprogram_ref(percent: str, marker: str | None, number: str) -> SubprogramRef:
    kinds = {("%%", True): "macro_sub", ("%%", False): "macro", ("%", True): "sub", ("%", False): "main"}
    kind = kinds[(percent, bool(marker))]
    return SubprogramRef(kind, int(number))


@dataclass(frozen=True)
class Word:
    address: str
    value: str  # number text, parameter reference ("-P3") or "" (no value)
    variant: int | None = None  # G87.2 -> address "G", value "87", variant 2

    @property
    def is_parameter(self) -> bool:
        return "P" in self.value

    def __str__(self) -> str:
        base = f"{self.address}{self.value}"
        return base if self.variant is None else f"{base}.{self.variant}"


@dataclass(frozen=True)
class SubBlock:
    """One statement of a block (blocks may hold several, separated by ';')."""

    kind: str  # "words", "assign", "calc", "if"
    words: tuple[Word, ...] = ()
    call: SubprogramRef | None = None
    target: str = ""  # assign/calc: "P1"/"PP3"
    expression: str = ""  # assign: right-hand side; if: condition
    then_text: str = ""
    else_text: str = ""


@dataclass(frozen=True)
class Block:
    line: int  # 1-based line number in the source text
    text: str  # original line
    number: int | None  # N number
    statements: tuple[SubBlock, ...] = field(default_factory=tuple)
    problem: str = ""


def strip_comments(text: str) -> str:
    previous = None
    while previous != text:
        previous = text
        text = _COMMENT_RE.sub(" ", text)
    return text


_EXPRESSION_LINE_RE = re.compile(r"(?:^|[;\s])P{1,2}\d+\s*=|(?:^|[;\s\d])IF\b")


def normalize(text: str) -> str:
    """Upper-case, comments removed, G variants written as ``G87#2``.

    Brackets in parameter assignments and IF statements are arithmetic, not comments.
    """
    upper = text.upper()
    if not _EXPRESSION_LINE_RE.search(upper):
        upper = strip_comments(upper)
    text = upper.replace("\t", " ")
    return _G_VARIANT_RE.sub(lambda m: f"G{m.group(1)}#{m.group(2)}", text)


def tokenize_words(text: str) -> tuple[tuple[Word, ...], SubprogramRef | None]:
    """Split a sub-block into words; ``%`` references are returned separately."""
    call: SubprogramRef | None = None
    sub = _SUB_RE.search(text)
    if sub is not None:
        call = subprogram_ref(sub.group(1), sub.group(2), sub.group(3))
        text = text[: sub.start()] + " " + text[sub.end() :]
    compact = re.sub(r"\s+", "", text)
    words: list[Word] = []
    pos = 0
    while pos < len(compact):
        match = _WORD_RE.match(compact, pos)
        if match is None or match.end() == pos:
            raise SyntaxProblem(f"Unerwartetes Zeichen '{compact[pos]}'")
        letters, value = match.group(1), match.group(2) or ""
        pos = match.end()
        words.append(_make_word(letters, value))
    return tuple(words), call


def _make_word(letters: str, value: str) -> Word:
    if letters == "G":
        return Word("G", value)
    if letters in ADDRESSES:
        return Word(letters, value)
    # "XP1" = X with parameter P1, "TIPP2" = TI with PP2, "PP3" alone = execute parameter
    if letters in ("P", "PP", "PT", "PPT"):
        return Word(letters, value)
    for suffix in _PARAM_SUFFIXES:
        head = letters[: -len(suffix)]
        if letters.endswith(suffix) and head in ADDRESSES and value and value[0].isdigit():
            return Word(head, suffix + value)
        if letters.endswith(suffix) and head in ADDRESSES and value and value[0] in "+-":
            return Word(head, value[0] + suffix + value[1:])
    raise SyntaxProblem(f"Unbekannte Adresse '{letters}'")


def parse_statement(text: str) -> SubBlock | None:
    text = text.strip()
    if not text:
        return None
    match = _ASSIGN_RE.match(text)
    if match:
        return SubBlock("assign", target=f"{match.group(1)}{match.group(2)}", expression=match.group(3).strip())
    match = _CALC_RE.match(text)
    if match:
        return SubBlock("calc", target=f"{match.group(1)}{match.group(2)}")
    match = _IF_RE.match(text)
    if match:
        return SubBlock(
            "if",
            expression=match.group(1).strip(),
            then_text=match.group(2).strip(),
            else_text=(match.group(3) or "").strip(),
        )
    words, call = _tokenize_with_variants(text)
    return SubBlock("words", words=words, call=call)


def _tokenize_with_variants(text: str) -> tuple[tuple[Word, ...], SubprogramRef | None]:
    variants: list[tuple[str, int]] = []

    def mark(match: re.Match[str]) -> str:
        variants.append((match.group(1), int(match.group(2))))
        return f"G{match.group(1)}"

    plain = re.sub(r"G(\d+)#(\d+)", lambda m: mark(m) + "\x00", text)
    # "\x00" marks a G word with variant; split there to keep words separate
    words: list[Word] = []
    call: SubprogramRef | None = None
    for index, part in enumerate(plain.split("\x00")):
        part_words, part_call = tokenize_words(part)
        call = call or part_call
        words.extend(part_words)
        if index < len(variants):
            number, variant = variants[index]
            # the last G word of this part carries the variant
            for pos in range(len(words) - 1, -1, -1):
                if words[pos].address == "G" and words[pos].value == number:
                    words[pos] = Word("G", number, variant)
                    break
    return tuple(words), call


def parse_line(line_no: int, raw: str) -> Block:
    """Parse one program line (comments removed, ``;`` sub-blocks split)."""
    text = normalize(raw)
    number: int | None = None
    match = re.match(r"^\s*N\s*(\d+)", text)
    if match:
        number = int(match.group(1))
        text = text[match.end() :]
    statements: list[SubBlock] = []
    try:
        if re.match(r"^\s*IF", text):
            statement = parse_statement(text)
            if statement is not None:
                statements.append(statement)
        else:
            for part in text.split(";"):
                statement = parse_statement(part)
                if statement is not None:
                    statements.append(statement)
    except SyntaxProblem as exc:
        return Block(line_no, raw, number, tuple(statements), str(exc))
    return Block(line_no, raw, number, tuple(statements))


@dataclass
class ProgramText:
    """A main program, macro or subprogram as a list of blocks."""

    ref: SubprogramRef | None  # None = main program of the file
    blocks: list[Block] = field(default_factory=list)

    @property
    def label(self) -> str:
        return "Hauptprogramm" if self.ref is None else self.ref.label

    def index_of(self, number: int) -> int | None:
        return next((i for i, b in enumerate(self.blocks) if b.number == number), None)


def split_programs(text: str) -> tuple[ProgramText, dict[SubprogramRef, ProgramText]]:
    """Split a file into the main program and further programs.

    A line holding only ``%*1``, ``%%4`` ... (without N number) opens a new
    program section; this file layout is a simulator convention. Lines that
    continue an IF statement with ``ELSE`` are joined to the previous line.
    """
    main = ProgramText(None)
    others: dict[SubprogramRef, ProgramText] = {}
    current = main
    pending: tuple[int, str] | None = None
    raw_lines = text.replace("\r\n", "\n").replace("\r", "\n").split("\n")

    def flush() -> None:
        nonlocal pending
        if pending is not None:
            current.blocks.append(parse_line(*pending))
            pending = None

    for index, raw in enumerate(raw_lines, start=1):
        clean = raw.replace("\x04", "").rstrip()
        stripped = strip_comments(clean).strip()
        if not stripped:
            continue
        header = HEADER_RE.match(stripped)
        if header:
            flush()
            ref = subprogram_ref(header.group(1), header.group(2), header.group(3))
            if ref.kind == "main" and not main.blocks and current is main:
                continue  # "%1" at the top names the main program
            current = ProgramText(ref)
            others[ref] = current
            continue
        if stripped in ("%", "%%"):
            continue
        if stripped.upper().startswith("ELSE") and pending is not None:
            pending = (pending[0], pending[1] + " " + clean)
            continue
        flush()
        pending = (index, clean)
    flush()
    return main, others
