"""Execution of DIALOG 11 program text into tool paths (offline simulation)."""

from __future__ import annotations

import math
import re
from dataclasses import dataclass, field, replace

from deckellink.dialog11sim import cycles
from deckellink.dialog11sim.expressions import ExpressionError, evaluate, is_arithmetic
from deckellink.dialog11sim.geometry import (
    PLANES,
    Plane,
    Transform,
    Vec3,
    arc_points,
    circle_center_from_radius,
    direction,
    offset_polyline,
)
from deckellink.dialog11sim.model import (
    Dwell,
    Message,
    Move,
    MoveKind,
    Severity,
    SimConfig,
    SimResult,
    Snapshot,
    ToolData,
)
from deckellink.dialog11sim.syntax import (
    Block,
    ProgramText,
    SubBlock,
    SubprogramRef,
    SyntaxProblem,
    Word,
    parse_statement,
    split_programs,
)

_COMPARE_RE = re.compile(r"^(.*?)(>=|<=|<>|==|=|>|<)(.*)$")
COORD_AXES = ("X", "Y", "Z")
CENTER_AXES = ("I", "J", "K")
POLAR_POLE = ("PX", "PY", "PZ")
MOTION_ADDRESSES = frozenset(
    {f"{a}{s}" for a in ("X", "Y", "Z", "C", "I", "J", "K", "PX", "PY", "PZ", "PW", "W") for s in ("", "A", "I")}
    | {"PL", "R"}
)
KNOWN_M = {
    2: "Programmende",
    3: "Spindel rechts",
    4: "Spindel links",
    5: "Spindel halt",
    6: "Werkzeugwechsel",
    17: "Unterprogramm-Ende",
    30: "Programmende mit Rücksprung",
    60: "konstanter Vorschub an der Kontur",
    61: "konstanter Vorschub an Innenradien",
    62: "konstanter Vorschub des Fräsermittelpunkts",
    79: "Ende Konturtasche",
}
CORNER_MODES = {
    (60, None): "G60 Übergangskreis",
    (60, 1): "G60.1 Genauhalt",
    (60, 2): "G60.2 verkürzter Weg",
    (64, None): "G64 Verschleifen innen",
    (64, 1): "G64.1 Verschleifen innen+außen",
}


class SimulationStop(Exception):
    """Program end or abort."""

    def __init__(self, reason: str) -> None:
        super().__init__(reason)
        self.reason = reason


class _Return(Exception):
    """M17 - leave the current subprogram."""


class _Jump(Exception):
    def __init__(self, number: int) -> None:
        super().__init__(number)
        self.number = number


@dataclass
class _Element:
    """Contour element collected while radius compensation is active."""

    kind: MoveKind
    points: list[Vec3]  # workpiece coordinates, without start point
    line: int
    program: str
    feed: float


@dataclass
class _Compensation:
    left: bool
    approach: int | None  # 45/46/47
    approach_r: float
    tool_start: Vec3
    entry_kind: MoveKind
    entry_line: int
    entry_program: str
    contour_start: Vec3
    elements: list[_Element] = field(default_factory=list)


@dataclass
class _Context:
    program: ProgramText
    label: str


class Interpreter:
    """Runs a program text and records moves, states and messages."""

    def __init__(self, text: str, config: SimConfig | None = None) -> None:
        self.config = config or SimConfig()
        self.result = SimResult()
        self.main, self.programs = split_programs(text)
        self.result.program_labels = [self.main.label, *[p.label for p in self.programs.values()]]
        self.plane: Plane = PLANES[17]
        self.absolute = True
        self.motion = 0
        self.rapid_variant = 0
        self.comp_mode = 40
        self.corner_mode = "G60 Übergangskreis"
        self.feed_mode = "M60"
        self.base: Vec3 = (0.0, 0.0, 0.0)
        self.base_label = "G51.0"
        self.shift: Vec3 = (0.0, 0.0, 0.0)
        self.rotation = 0.0
        self.mirror: list[float | None] = [None, None, None]
        self.scale: list[float] = [1.0, 1.0, 1.0]
        self.tool_label = "T0"
        self.tool = ToolData(radius=0.0)
        self.feed = 0.0
        self.speed = 0.0
        self.spindle = "M5"
        self.c_axis = 0.0
        self.other_m: list[int] = []
        start = self.config.start_position
        self.tool_w: Vec3 = start
        self.contour_w: Vec3 = start
        self.memory: dict[str, float | str] = {}
        self.comp: _Compensation | None = None
        self.contour_pocket: tuple[float, float] | None = None  # (depth level, retract level) in tool axis
        self.blocks_run = 0
        self.call_depth = 0
        self.points_total = 0
        self._ctx: _Context = _Context(self.main, self.main.label)
        self._line = 0
        self._warned: set[str] = set()
        self._feed_warned = False

    # ------------------------------------------------------------------ public
    def run(self) -> SimResult:
        self._record_state()
        try:
            self._run_program(self.main)
            self.result.end_reason = "Programmende ohne M30/M2 erreicht"
            self._msg(Severity.WARNING, "Programm endet ohne M30 oder M2.", line=0)
        except SimulationStop as stop:
            self.result.end_reason = stop.reason
        if self.comp is not None:
            self._msg(Severity.WARNING, "Bahnkorrektur am Programmende nicht mit G40 abgewählt.")
            self._flush_compensation(None, None)
        self.result.executed_blocks = self.blocks_run
        self.result.parameters = {k: (f"{v:g}" if isinstance(v, float) else v) for k, v in self.memory.items()}
        return self.result

    # ------------------------------------------------------------------ messages / state
    def _msg(self, severity: Severity, text: str, *, line: int | None = None, once: bool = False) -> None:
        if once:
            if text in self._warned:
                return
            self._warned.add(text)
        self.result.messages.append(Message(severity, text, self._line if line is None else line, self._ctx.label))

    def note(self, text: str) -> None:
        """Simplification note (shown once per text)."""
        self._msg(Severity.INFO, text, once=True)

    def warn(self, text: str) -> None:
        self._msg(Severity.WARNING, text)

    @property
    def transform(self) -> Transform:
        offset = (self.base[0] + self.shift[0], self.base[1] + self.shift[1], self.base[2] + self.shift[2])
        return Transform(
            offset,
            self.rotation,
            self.plane,
            (self.mirror[0], self.mirror[1], self.mirror[2]),
            (
                self.scale[0],
                self.scale[1],
                self.scale[2],
            ),
        )

    @property
    def position(self) -> Vec3:
        """Programmed position in the active program coordinate system."""
        return self.transform.invert(self.contour_w)

    def _modal(self) -> tuple[tuple[str, str], ...]:
        motion = f"G{self.motion}"
        if self.motion == 0 and self.rapid_variant:
            motion += f".{self.rapid_variant}"
        comp = {40: "G40 aus", 41: "G41 links", 42: "G42 rechts"}[self.comp_mode]
        zero = self.base_label
        if any(abs(v) > 1e-9 for v in self.shift):
            zero += f" + Verschiebung {self.shift[0]:g}/{self.shift[1]:g}/{self.shift[2]:g}"
        if abs(self.rotation) > 1e-9:
            zero += f", Drehung {self.rotation:g}°"
        mirror = ", ".join(f"{'XYZ'[i]}{m:g}" for i, m in enumerate(self.mirror) if m is not None) or "aus"
        scale = ", ".join(f"{'XYZ'[i]}{s:g}" for i, s in enumerate(self.scale) if s != 1.0) or "aus"
        items = [
            ("Wegbedingung", motion),
            ("Ebene", self.plane.label),
            ("Maßangabe", "G90 absolut" if self.absolute else "G91 inkremental"),
            ("Bahnkorrektur", comp),
            ("Eckenverhalten", self.corner_mode),
            ("Vorschubverhalten", self.feed_mode),
            ("Nullpunkt", zero),
            ("Spiegeln G58", mirror),
            ("Maßstab G59", scale),
            ("Spindel", f"{self.spindle} {KNOWN_M.get(int(self.spindle[1:]), '')}".strip()),
        ]
        if self.contour_pocket is not None:
            items.append(("Zyklus", "G87.9 Konturtasche aktiv"))
        if self.other_m:
            items.append(("Weitere M", " ".join(f"M{m}" for m in self.other_m[-6:])))
        return tuple(items)

    def _record_state(self) -> int:
        snap = Snapshot(
            line=self._line,
            program=self._ctx.label,
            position=self.position,
            modal=self._modal(),
            tool=self.tool_label,
            tool_radius=self.tool.radius,
            feed=self.feed,
            speed=self.speed,
            spindle=self.spindle,
            c_axis=self.c_axis,
        )
        self.result.snapshots.append(snap)
        return len(self.result.snapshots) - 1

    # ------------------------------------------------------------------ program flow
    def _run_program(self, program: ProgramText, label: str | None = None) -> None:
        previous = self._ctx
        self._ctx = _Context(program, label or program.label)
        try:
            self._run_range(0, len(program.blocks))
        except _Return:
            pass
        except _Jump as jump:
            raise SimulationStop(f"Sprungziel N{jump.number} nicht im Programm {program.label}") from None
        finally:
            self._ctx = previous

    def _run_range(self, start: int, end: int) -> None:
        blocks = self._ctx.program.blocks
        pc = start
        while pc < end:
            try:
                self._run_block(blocks[pc])
                pc += 1
            except _Jump as jump:
                target = self._ctx.program.index_of(jump.number)
                if target is None:
                    raise SimulationStop(f"Sprungziel N{jump.number} nicht gefunden") from None
                pc = target
                if pc >= end or pc < start:
                    raise

    def _run_block(self, block: Block) -> None:
        self.blocks_run += 1
        if self.blocks_run > self.config.max_blocks:
            self._line = block.line
            self._msg(Severity.ERROR, f"Mehr als {self.config.max_blocks} Sätze ausgeführt - Endlosschleife?")
            raise SimulationStop("Abbruch: zu viele Sätze (Endlosschleife?)")
        self._line = block.line
        if block.problem:
            self._msg(Severity.ERROR, f"Satz nicht lesbar: {block.problem}")
            return
        stop: SimulationStop | None = None
        returning = False
        for statement in block.statements:
            try:
                self._run_statement(statement)
            except SimulationStop as exc:
                stop = exc
                break
            except _Return:
                returning = True
                break
            except (ExpressionError, SyntaxProblem, ValueError) as exc:
                self._msg(Severity.ERROR, str(exc))
        self._record_state()
        if stop is not None:
            raise stop
        if returning:
            raise _Return

    def _run_statement(self, statement: SubBlock) -> None:
        if statement.kind == "assign":
            self._assign(statement.target, statement.expression)
        elif statement.kind == "calc":
            self._calculate(statement.target)
        elif statement.kind == "if":
            branch = statement.then_text if self._condition(statement.expression) else statement.else_text
            for part in branch.split(";"):
                sub = parse_statement(part)
                if sub is not None:
                    self._run_statement(sub)
        else:
            self._run_words(statement)

    # ------------------------------------------------------------------ parameters
    def resolve(self, name: str) -> float:
        value = self._lookup(name)
        if isinstance(value, str):
            if is_arithmetic(value):
                return evaluate(value, self.resolve)
            raise ExpressionError(f"{name} enthält keinen Zahlenwert ('{value}')")
        return value

    def _lookup(self, name: str) -> float | str:
        table_only = name.startswith(("PT", "PPT"))
        key = name.replace("PPT", "PP").replace("PT", "P") if table_only else name
        if not table_only and key in self.memory:
            return self.memory[key]
        if key in self.config.parameter_table:
            return self.config.parameter_table[key]
        raise ExpressionError(f"Parameter {key} ist nicht belegt")

    def _assign(self, target: str, expression: str) -> None:
        if is_arithmetic(expression):
            self.memory[target] = evaluate(expression, self.resolve)
        else:
            self.memory[target] = expression.strip()

    def _calculate(self, target: str) -> None:
        if target not in self.config.parameter_table:
            raise ExpressionError(f"{target}! - keine Zuweisung in der Parametertabelle")
        self.memory[target] = evaluate(self.config.parameter_table[target], self.resolve)

    def _condition(self, text: str) -> bool:
        match = _COMPARE_RE.match(text.replace(" ", ""))
        if match is None:
            raise ExpressionError(f"IF-Bedingung nicht lesbar: {text}")
        left = evaluate(match.group(1), self.resolve)
        right = evaluate(match.group(3), self.resolve)
        op = match.group(2)
        return {
            ">": left > right,
            "<": left < right,
            ">=": left >= right - 1e-9,
            "<=": left <= right + 1e-9,
            "<>": abs(left - right) > 1e-9,
            "==": abs(left - right) <= 1e-9,
            "=": abs(left - right) <= 1e-9,
        }[op]

    def value(self, word: Word) -> float:
        text = word.value
        if not text:
            raise ExpressionError(f"Adresse {word.address} ohne Wert")
        if "P" in text:
            negative = text.startswith("-")
            number = self.resolve(text.lstrip("+-"))
            return -number if negative else number
        return float(text)

    # ------------------------------------------------------------------ words
    def _run_words(self, statement: SubBlock) -> None:
        words = list(statement.words)
        is_pattern = any(w.address == "G" and w.value == "89" for w in words)
        if statement.call is not None and not is_pattern:
            count = 1
            for w in words:
                if w.address == "L":
                    count = int(self.value(w))
            self._call(statement.call, count)
            return
        if len(words) == 1 and words[0].address in ("P", "PP"):
            self._execute_parameter(f"{words[0].address}{words[0].value}")
            return
        numbers = [w for w in words if w.address == "N"]
        if any(w.address == "L" for w in words) and numbers:
            count = int(self.value(next(w for w in words if w.address == "L")))
            first = int(self.value(numbers[0]))
            if count == 0:
                raise _Jump(first)
            last = int(self.value(numbers[-1]))
            self._repeat(first, last, count)
            return
        g_words = [w for w in words if w.address == "G"]
        g_numbers = {int(float(w.value)) for w in g_words if w.value}
        cycle = next((w for w in g_words if w.value and int(float(w.value)) in (81, 82, 83, 84, 85, 86, 87, 89)), None)
        self._apply_modal(words, g_words)
        if cycle is not None:
            number = int(float(cycle.value))
            by_address = {w.address: w for w in words if w.address != "G"}
            cycles.run_cycle(self, number, cycle.variant, by_address, words, statement.call)
            self._apply_m_after(words)
            return
        if g_numbers & {53, 54, 55, 56, 58, 59} or any(w.address == "G" and w.value == "51" for w in g_words):
            self._apply_coordinate_system(words, g_words)
            self._apply_m_after(words)
            return
        if 4 in g_numbers:
            f = next((w for w in words if w.address == "F"), None)
            seconds = self.value(f) if f is not None else 0.0
            self.result.dwells.append(Dwell(seconds, self._line, len(self.result.snapshots), len(self.result.moves)))
        self._motion(words, g_numbers)
        self._apply_m_after(words)

    def _execute_parameter(self, name: str) -> None:
        content = self._lookup(name)
        if not isinstance(content, str):
            raise ExpressionError(f"{name} enthält einen Zahlenwert und keinen Satzinhalt")
        self.call_depth += 1
        if self.call_depth > self.config.max_call_depth:
            raise SimulationStop("Abbruch: Parameter-Aufrufe zu tief verschachtelt")
        try:
            for part in content.split(";"):
                sub = parse_statement(part)
                if sub is not None:
                    self._run_statement(sub)
        finally:
            self.call_depth -= 1

    def execute_text(self, text: str) -> None:
        """Run a word chain or subprogram reference (used by point patterns)."""
        text = text.strip()
        if re.fullmatch(r"(PP|P)\d+", text):
            self._execute_parameter(text)
            return
        for part in text.split(";"):
            sub = parse_statement(part)
            if sub is not None:
                self._run_statement(sub)

    def _call(self, ref: SubprogramRef, count: int) -> None:
        program = self.programs.get(ref)
        if program is None:
            self._msg(Severity.ERROR, f"{ref.label} ist in der Datei nicht enthalten - Aufruf übersprungen")
            return
        self.call_depth += 1
        if self.call_depth > self.config.max_call_depth:
            raise SimulationStop("Abbruch: Unterprogramm-Aufrufe zu tief verschachtelt")
        try:
            for _ in range(max(count, 0)):
                self._run_program(program)
        finally:
            self.call_depth -= 1

    def _repeat(self, first: int, last: int, count: int) -> None:
        program = self._ctx.program
        start = program.index_of(first)
        end = program.index_of(last)
        if start is None or end is None or end < start:
            self._msg(Severity.ERROR, f"Programmteil N{first} bis N{last} nicht gefunden")
            return
        self.call_depth += 1
        if self.call_depth > self.config.max_call_depth:
            raise SimulationStop("Abbruch: Wiederholungen zu tief verschachtelt")
        try:
            for _ in range(max(count, 0)):
                self._run_range(start, end + 1)
        finally:
            self.call_depth -= 1

    # ------------------------------------------------------------------ modal functions
    def _apply_modal(self, words: list[Word], g_words: list[Word]) -> None:
        for w in g_words:
            if not w.value:
                continue
            g = int(float(w.value))
            if g in (0, 1, 2, 3):
                self.motion = g
                if g == 0:
                    self.rapid_variant = w.variant or 0
            elif g in (17, 18, 19):
                if self.comp is not None:
                    self.warn("Ebenenwechsel bei aktiver Bahnkorrektur")
                self.plane = PLANES[g]
                if w.variant:
                    self.note(f"G{g}.{w.variant}: Variante der Ebene wird wie G{g} dargestellt")
            elif g == 90:
                self.absolute = True
            elif g == 91:
                self.absolute = False
            elif g in (60, 64) and (g, w.variant) in CORNER_MODES:
                self.corner_mode = CORNER_MODES[(g, w.variant)]
            elif g in (61, 62):
                self.note("G61/G62 (Radius-/Fasenübergang an Ecken) wird in der Grafik nicht berechnet")
            elif g == 9 or g in (40, 41, 42, 45, 46, 47, 4) or g in (51, 53, 54, 55, 56, 58, 59) or g >= 81:
                continue
            else:
                self.note(f"G{g} wird im Simulator nicht ausgewertet")
        for w in words:
            if w.address in ("T", "TT"):
                number = int(self.value(w))
                self._select_tool(number, general=w.address == "TT")
            elif w.address == "S":
                self.speed = self.value(w)
            elif w.address == "M":
                m = int(self.value(w))
                if m in (3, 4, 5):
                    self.spindle = f"M{m}"
                elif m in (60, 61, 62):
                    self.feed_mode = f"M{m}"
                elif m not in (2, 30, 17, 6, 79):
                    self.other_m.append(m)
                    if m not in KNOWN_M:
                        self.note(f"M{m}: Maschinenfunktion - im Simulator ohne Wirkung")
        if not any(w.address == "G" and w.value in ("4", "04") for w in g_words):
            for w in words:
                if w.address == "F":
                    self.feed = self.value(w)

    def _select_tool(self, number: int, general: bool) -> None:
        self.tool_label = f"{'TT' if general else 'T'}{number}"
        if number == 0:
            self.tool = ToolData(radius=0.0)
            return
        tool = self.config.tools.get(number)
        if tool is None:
            tool = ToolData(radius=self.config.default_tool_radius)
            self.note(
                f"{self.tool_label}: nicht in der Simulator-Werkzeugtabelle - Radius "
                f"{self.config.default_tool_radius:g} mm angenommen"
            )
        self.tool = tool

    def _apply_m_after(self, words: list[Word]) -> None:
        for w in words:
            if w.address != "M":
                continue
            m = int(self.value(w))
            if m in (2, 30):
                if self.comp is not None:
                    self._flush_compensation(None, None)
                self._record_state()
                raise SimulationStop(f"M{m} {KNOWN_M[m]}")
            if m == 17:
                if self._ctx.program is self.main:
                    self.warn("M17 im Hauptprogramm")
                else:
                    raise _Return
            if m == 79:
                if self.contour_pocket is None:
                    self.warn("M79 ohne aktive Konturtasche")
                else:
                    if self.comp is not None:
                        self._flush_compensation(None, None)
                    _depth, retract = self.contour_pocket
                    self.contour_pocket = None
                    u, v, _w = self.plane.to_plane(self.position)
                    self.rapid_to(self.plane.from_plane(u, v, retract), note="Ende Konturtasche")

    # ------------------------------------------------------------------ coordinate systems
    def _apply_coordinate_system(self, words: list[Word], g_words: list[Word]) -> None:
        coords: dict[int, float] = {}
        rotation: float | None = None
        for w in words:
            base = w.address.rstrip("AI") if w.address not in ("A", "I") else w.address
            if base in COORD_AXES:
                coords[COORD_AXES.index(base)] = self.value(w)
            elif w.address in ("W", "WA", "WI"):
                rotation = self.value(w)
        for w in g_words:
            g = int(float(w.value))
            if g == 51:
                number = w.variant or 0
                self.base = self.config.zero_points.get(number, (0.0, 0.0, 0.0))
                if number not in self.config.zero_points and number != 0:
                    self.note(f"G51.{number}: kein Eintrag in der Simulator-Nullpunkttabelle - 0/0/0 angenommen")
                self.base_label = f"G51.{number}"
            elif g == 53:
                self.shift = (0.0, 0.0, 0.0)
                self.rotation = 0.0
            elif g == 54:
                current = self.position
                shift = list(self.shift)
                for axis, val in coords.items():
                    shift[axis] += (current[axis] - val) * self.scale[axis]
                self.shift = (shift[0], shift[1], shift[2])
            elif g == 55:
                plain = Transform(
                    (self.base[0] + self.shift[0], self.base[1] + self.shift[1], self.base[2] + self.shift[2]),
                    self.rotation,
                    self.plane,
                )
                target = plain.apply((coords.get(0, 0.0), coords.get(1, 0.0), coords.get(2, 0.0)))
                self.shift = (target[0] - self.base[0], target[1] - self.base[1], target[2] - self.base[2])
                if rotation is not None:
                    self.rotation += rotation
            elif g == 56:
                self.shift = (coords.get(0, 0.0), coords.get(1, 0.0), coords.get(2, 0.0))
                self.rotation = rotation or 0.0
            elif g == 58:
                self.mirror = [coords.get(a) for a in range(3)]
            elif g == 59:
                self.scale = [coords.get(a, 1.0) for a in range(3)]
                if any(abs(s) < 1e-9 for s in self.scale):
                    self.warn("G59 mit Maßstabsfaktor 0")

    # ------------------------------------------------------------------ motion
    def _target(self, words: list[Word], start: Vec3) -> tuple[Vec3, bool]:
        """Cartesian target in program coordinates; second value: any axis programmed."""
        target = list(start)
        programmed = False
        for w in words:
            for axis, name in enumerate(COORD_AXES):
                if w.address in (name, name + "A", name + "I"):
                    val = self.value(w)
                    incremental = w.address.endswith("I") or (w.address == name and not self.absolute)
                    target[axis] = start[axis] + val if incremental else val
                    programmed = True
            if w.address in ("C", "CA", "CI"):
                val = self.value(w)
                self.c_axis = (
                    self.c_axis + val if (w.address == "CI" or (w.address == "C" and not self.absolute)) else val
                )
                self.note("C-Achse (Rundtisch): Stellung wird angezeigt, die Bahn aber ohne Drehung dargestellt")
        return (target[0], target[1], target[2]), programmed

    def _words_by_address(self, words: list[Word]) -> dict[str, Word]:
        return {w.address: w for w in words}

    def _motion(self, words: list[Word], g_numbers: set[int]) -> None:
        by = self._words_by_address(words)
        start = self.position
        target, programmed = self._target(words, start)
        approach = next((g for g in (45, 46, 47) if g in g_numbers), None)
        approach_r = self.value(by["R"]) if approach is not None and "R" in by else 0.0
        polar = any(a in by for a in ("PW", "PWA", "PWI", "PL", "PX", "PY", "PXA", "PYA", "PXI", "PYI"))
        g9 = 9 in g_numbers
        for address in ("UW", "UR", "FA"):
            if address in by:
                self.note("Konturzug-Angaben (UW, UR, FA) werden im Simulator nicht berechnet")
        if polar and self.motion in (0, 1):
            target = self._polar_target(by, start, target)
            programmed = True
        if g9 and self.motion in (0, 1):
            target = self._g9_line_target(by, start, target)
            programmed = True
        if 40 in g_numbers:
            self.comp_mode = 40
            if self.comp is not None:
                self._flush_compensation(approach, approach_r)
            if programmed:
                self.straight(target, MoveKind.RAPID if self.motion == 0 else MoveKind.FEED)
            return
        if 41 in g_numbers or 42 in g_numbers:
            left = 41 in g_numbers
            if self.comp is not None:
                self._flush_compensation(None, None)
            self.comp_mode = 41 if left else 42
            if self.tool.radius <= 0:
                self.warn("Bahnkorrektur ohne Werkzeugradius (kein Werkzeug aufgerufen?)")
            entry_kind = MoveKind.RAPID if self.motion == 0 else MoveKind.FEED
            contour_start = self.transform.apply(target) if programmed else self.contour_w
            entry_feed = self.config.rapid_feed if entry_kind is MoveKind.RAPID else self._effective_feed(None)
            self._add_move(self.result.programmed, entry_kind, [self.contour_w, contour_start], entry_feed, "")
            self.comp = _Compensation(
                left=left != self.transform.reverses_direction,
                approach=approach,
                approach_r=approach_r,
                tool_start=self.tool_w,
                entry_kind=entry_kind,
                entry_line=self._line,
                entry_program=self._ctx.label,
                contour_start=contour_start,
            )
            self.contour_w = contour_start
            return
        if self.motion in (2, 3):
            has_center = any(a in by for a in ("I", "J", "K", "IA", "JA", "KA", "II", "JI", "KI"))
            if programmed or has_center or g9:
                self._arc(by, start, target, programmed, g9)
            return
        if not programmed:
            return
        if self.motion == 0:
            self.rapid_to(target)
        else:
            self.straight(target, MoveKind.FEED)

    def _polar_target(self, by: dict[str, Word], start: Vec3, target: Vec3) -> Vec3:
        pu, pv, _ = self.plane.to_plane(start)
        pole = [start[0], start[1], start[2]]
        for axis, name in enumerate(POLAR_POLE):
            for suffix in ("", "A", "I"):
                w = by.get(name + suffix)
                if w is not None:
                    incremental = suffix == "I" or (suffix == "" and not self.absolute)
                    pole[axis] = start[axis] + self.value(w) if incremental else self.value(w)
        cu, cv, _ = self.plane.to_plane((pole[0], pole[1], pole[2]))
        angle_w = by.get("PW") or by.get("PWA") or by.get("PWI")
        length_w = by.get("PL")
        angle = self.value(angle_w) if angle_w is not None else math.degrees(math.atan2(pv - cv, pu - cu))
        length = self.value(length_w) if length_w is not None else math.hypot(pu - cu, pv - cv)
        _, _, tw = self.plane.to_plane(target)
        return self.plane.from_plane(
            cu + length * math.cos(math.radians(angle)), cv + length * math.sin(math.radians(angle)), tw
        )

    def _center(self, by: dict[str, Word], start: Vec3) -> tuple[float, float] | None:
        center = list(start)
        found = False
        for axis, name in enumerate(CENTER_AXES):
            for suffix in ("", "A", "I"):
                w = by.get(name + suffix)
                if w is None:
                    continue
                incremental = suffix == "I" or (suffix == "" and not self.absolute)
                center[axis] = start[axis] + self.value(w) if incremental else self.value(w)
                if axis in (self.plane.main, self.plane.second):
                    found = True
        if not found:
            return None
        u, v, _ = self.plane.to_plane((center[0], center[1], center[2]))
        return u, v

    def _g9_line_target(self, by: dict[str, Word], start: Vec3, target: Vec3) -> Vec3:
        center = self._center(by, start)
        su, sv, _ = self.plane.to_plane(start)
        cu, cv = center if center is not None else (su, sv)
        angle_w = by.get("W") or by.get("WA") or by.get("WI")
        angle = self.value(angle_w) if angle_w is not None else 0.0
        length = self.value(by["R"]) if "R" in by else math.hypot(su - cu, sv - cv)
        _, _, tw = self.plane.to_plane(target)
        return self.plane.from_plane(
            cu + length * math.cos(math.radians(angle)), cv + length * math.sin(math.radians(angle)), tw
        )

    def _arc(self, by: dict[str, Word], start: Vec3, target: Vec3, programmed: bool, g9: bool) -> None:
        clockwise = self.motion == 2
        center = self._center(by, start)
        su, sv, _sw = self.plane.to_plane(start)
        sweep: float | None = None
        full = False
        if g9:
            if center is None:
                self._msg(Severity.ERROR, "G9 im Kreissatz ohne Mittelpunkt (I, J, K)")
                return
            cu, cv = center
            radius = math.hypot(su - cu, sv - cv)
            _, _, tw = self.plane.to_plane(target)
            if "WI" in by:
                sweep = abs(self.value(by["WI"])) * (-1 if clockwise else 1)
                angle = math.degrees(math.atan2(sv - cv, su - cu)) + sweep
            else:
                angle_w = by.get("W") or by.get("WA")
                angle = self.value(angle_w) if angle_w is not None else math.degrees(math.atan2(sv - cv, su - cu))
            target = self.plane.from_plane(
                cu + radius * math.cos(math.radians(angle)), cv + radius * math.sin(math.radians(angle)), tw
            )
        elif center is None:
            if "R" not in by or not programmed:
                self._msg(Severity.ERROR, "Kreissatz ohne Mittelpunkt (I, J, K) bzw. Radius")
                return
            eu, ev, _ = self.plane.to_plane(target)
            found = circle_center_from_radius((su, sv), (eu, ev), self.value(by["R"]), clockwise)
            if found is None:
                self._msg(Severity.ERROR, "Kreis mit Radius R: Zielpunkt mit diesem Radius nicht erreichbar")
                return
            self.note("Kreis über Radius R (Konturrechner): positives R = Bogen bis 180° (Annahme)")
            center = found
        else:
            eu, ev, _ = self.plane.to_plane(target)
            if not programmed or (abs(eu - su) < 1e-9 and abs(ev - sv) < 1e-9):
                full = True
            else:
                r1 = math.hypot(su - center[0], sv - center[1])
                r2 = math.hypot(eu - center[0], ev - center[1])
                if abs(r1 - r2) > 0.01:
                    self.warn(f"Kreis: Start- und Zielradius weichen ab ({r1:.3f} / {r2:.3f} mm)")
        assert center is not None
        points = arc_points(self.plane, start, target, center, clockwise, full_circle=full, sweep_override=sweep)
        if sweep is not None:
            points[-1] = target
        self._emit_program_points(points, MoveKind.FEED)

    # ------------------------------------------------------------------ emitting moves
    def rapid_to(self, target: Vec3, note: str = "") -> None:
        """Rapid move according to the active G0 variant (program coordinates)."""
        start = self.position
        if self.rapid_variant == 2 or self.comp is not None:
            points = [target]
        elif self.rapid_variant == 1:
            su, sv, sw = self.plane.to_plane(start)
            tu, tv, tw = self.plane.to_plane(target)
            if tw >= sw:
                points = [self.plane.from_plane(su, sv, tw), target]
            else:
                points = [self.plane.from_plane(tu, tv, sw), target]
        else:
            points = _dog_leg(start, target)
        self._emit_program_points(points, MoveKind.RAPID, note)

    def straight(self, target: Vec3, kind: MoveKind, note: str = "") -> None:
        if kind is MoveKind.RAPID:
            self.rapid_to(target, note)
        else:
            self._emit_program_points([target], kind, note)

    def feed_to(self, target: Vec3, note: str = "", feed: float | None = None) -> None:
        self._emit_program_points([target], MoveKind.FEED, note, feed)

    def arc_to(
        self,
        target: Vec3,
        center_uv: tuple[float, float],
        clockwise: bool,
        note: str = "",
        *,
        full_circle: bool = False,
        feed: float | None = None,
    ) -> None:
        points = arc_points(self.plane, self.position, target, center_uv, clockwise, full_circle=full_circle)
        self._emit_program_points(points, MoveKind.FEED, note, feed)

    def dwell(self, seconds: float) -> None:
        self.result.dwells.append(Dwell(seconds, self._line, len(self.result.snapshots), len(self.result.moves)))

    def _emit_program_points(
        self, points: list[Vec3], kind: MoveKind, note: str = "", feed: float | None = None
    ) -> None:
        if self.contour_pocket is not None and kind is MoveKind.FEED:
            level = self.contour_pocket[0]
            flat: list[Vec3] = []
            for p in points:
                u, v, _w = self.plane.to_plane(p)
                flat.append(self.plane.from_plane(u, v, level))
            points = flat
        transform = self.transform
        w_points = [transform.apply(p) for p in points]
        use_feed = self._effective_feed(feed) if kind is MoveKind.FEED else self.config.rapid_feed
        if self.comp is not None:
            self.comp.elements.append(_Element(kind, w_points, self._line, self._ctx.label, use_feed))
            self._add_move(self.result.programmed, kind, [self.contour_w, *w_points], use_feed, note)
            self.contour_w = w_points[-1]
            return
        self._add_move(self.result.programmed, kind, [self.contour_w, *w_points], use_feed, note)
        self._add_move(self.result.moves, kind, [self.tool_w, *w_points], use_feed, note)
        self.contour_w = self.tool_w = w_points[-1]

    def _effective_feed(self, feed: float | None) -> float:
        value = feed if feed is not None else self.feed
        if value <= 0:
            if not self._feed_warned:
                self._feed_warned = True
                self.warn(f"Kein Vorschub F programmiert - {self.config.default_feed:g} mm/min angenommen")
            return self.config.default_feed
        return value

    def _add_move(
        self,
        target: list[Move],
        kind: MoveKind,
        points: list[Vec3],
        feed: float,
        note: str,
        *,
        compensated: bool = False,
        line: int | None = None,
        program: str | None = None,
    ) -> None:
        length = sum(math.dist(points[i], points[i + 1]) for i in range(len(points) - 1))
        if length < 1e-9:
            return
        self.points_total += len(points)
        if self.points_total > self.config.max_points:
            raise SimulationStop("Abbruch: zu viele Bahnpunkte")
        duration = length / feed * 60.0 if feed > 0 else 0.0
        target.append(
            Move(
                kind,
                tuple(points),
                self._line if line is None else line,
                self._ctx.label if program is None else program,
                len(self.result.snapshots),
                feed,
                length,
                duration,
                note,
                compensated,
            )
        )

    # ------------------------------------------------------------------ radius compensation
    def _flush_compensation(self, approach: int | None, approach_r: float | None) -> None:
        comp = self.comp
        self.comp = None
        self.comp_mode = 40
        if comp is None:
            return
        first_point = comp.contour_start
        points: list[Vec3] = [first_point]
        sources: list[int] = [0]
        for index, element in enumerate(comp.elements, start=1):
            points.extend(element.points)
            sources.extend([index] * len(element.points))
        radius = self.tool.radius
        if len(points) < 2:
            self.warn("Bahnkorrektur ohne Konturelemente")
            self._add_move(self.result.moves, comp.entry_kind, [self.tool_w, first_point], self.config.rapid_feed, "")
            self.tool_w = first_point
            return
        path, path_src = offset_polyline(points, sources, self.plane, radius, comp.left)
        plane = self.plane
        # approach
        d0 = direction(plane, points[0], points[1])
        approach_pts = _approach(plane, path[0], d0, comp.left, comp.approach, comp.approach_r)
        entry_feed = self.config.rapid_feed if comp.entry_kind is MoveKind.RAPID else self._effective_feed(None)
        if comp.approach is not None:
            self.note("An- und Wegfahren (G45/G46/G47) vereinfacht dargestellt")
        entry = [comp.tool_start, approach_pts[0]] if approach_pts else [comp.tool_start, path[0]]
        self._add_move(
            self.result.moves,
            comp.entry_kind,
            entry,
            entry_feed,
            "Anfahren",
            compensated=True,
            line=comp.entry_line,
            program=comp.entry_program,
        )
        if approach_pts:
            self._add_move(
                self.result.moves,
                MoveKind.FEED,
                [*approach_pts, path[0]],
                self._effective_feed(None),
                f"Anfahren G{comp.approach}",
                compensated=True,
                line=comp.entry_line,
                program=comp.entry_program,
            )
        # contour elements
        current = path[0]
        group: list[Vec3] = [current]
        group_src = path_src[0] if path_src[0] > 0 else 1
        for p, raw_src in zip(path[1:], path_src[1:], strict=True):
            s = max(raw_src, 1)
            if s != group_src and len(group) > 1:
                self._emit_comp_group(comp, group_src, group)
                group = [group[-1]]
            group_src = s
            group.append(p)
        if len(group) > 1:
            self._emit_comp_group(comp, group_src, group)
        end = path[-1]
        # departure
        if approach is not None and approach_r:
            d_last = direction(plane, points[-2], points[-1])
            leave = _departure(plane, end, d_last, comp.left, approach, approach_r)
            self._add_move(
                self.result.moves,
                MoveKind.FEED,
                [end, *leave],
                self._effective_feed(None),
                f"Wegfahren G{approach}",
                compensated=True,
            )
            end = leave[-1]
        self.tool_w = end
        self.contour_w = end

    def _emit_comp_group(self, comp: _Compensation, source: int, points: list[Vec3]) -> None:
        element = comp.elements[source - 1]
        self._add_move(
            self.result.moves,
            element.kind,
            points,
            element.feed,
            "",
            compensated=True,
            line=element.line,
            program=element.program,
        )


def _dog_leg(start: Vec3, target: Vec3) -> list[Vec3]:
    """Rapid without interpolation: all axes move at the same speed and stop independently."""
    deltas = [target[i] - start[i] for i in range(3)]
    distances = sorted({abs(d) for d in deltas if abs(d) > 1e-9})
    points: list[Vec3] = []
    for travelled in distances:
        p = [start[i] + math.copysign(min(abs(deltas[i]), travelled), deltas[i]) for i in range(3)]
        points.append((p[0], p[1], p[2]))
    if not points:
        return [target]
    points[-1] = target
    return points


def _approach(plane: Plane, first: Vec3, d0: tuple[float, float], left: bool, kind: int | None, r: float) -> list[Vec3]:
    """Start point(s) of the approach to ``first`` (tool centre on the contour start)."""
    if kind is None or r <= 0:
        return []
    u, v, w = plane.to_plane(first)
    side = 1.0 if left else -1.0
    n = (-d0[1] * side, d0[0] * side)  # towards the tool side
    if kind == 45:
        return [plane.from_plane(u - d0[0] * r, v - d0[1] * r, w)]
    if kind == 47:
        cu, cv = u + n[0] * r, v + n[1] * r
        start = plane.from_plane(cu - d0[0] * r, cv - d0[1] * r, w)
        return [start, *arc_points(plane, start, first, (cu, cv), clockwise=not left)[:-1]]
    cu, cv = u + n[0] * r, v + n[1] * r
    start = plane.from_plane(u + 2 * n[0] * r, v + 2 * n[1] * r, w)
    return [start, *arc_points(plane, start, first, (cu, cv), clockwise=not left)[:-1]]


def _departure(plane: Plane, last: Vec3, d: tuple[float, float], left: bool, kind: int, r: float) -> list[Vec3]:
    u, v, w = plane.to_plane(last)
    side = 1.0 if left else -1.0
    n = (-d[1] * side, d[0] * side)
    if kind == 45:
        return [plane.from_plane(u + d[0] * r, v + d[1] * r, w)]
    cu, cv = u + n[0] * r, v + n[1] * r
    if kind == 47:
        end = plane.from_plane(cu + d[0] * r, cv + d[1] * r, w)
    else:
        end = plane.from_plane(u + 2 * n[0] * r, v + 2 * n[1] * r, w)
    return arc_points(plane, last, end, (cu, cv), clockwise=not left)


def simulate(text: str, config: SimConfig | None = None) -> SimResult:
    """Simulate ``text`` (program text of one file)."""
    return Interpreter(text, config).run()


def with_tools(config: SimConfig, tools: dict[int, ToolData]) -> SimConfig:
    return replace(config, tools=dict(tools))
