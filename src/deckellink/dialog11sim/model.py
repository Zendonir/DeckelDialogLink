"""Result data of a simulation run."""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import StrEnum

from deckellink.dialog11sim.geometry import Vec3


class MoveKind(StrEnum):
    RAPID = "rapid"
    FEED = "feed"


class Severity(StrEnum):
    INFO = "info"
    WARNING = "warning"
    ERROR = "error"

    @property
    def label(self) -> str:
        return {"info": "Hinweis", "warning": "Warnung", "error": "Fehler"}[self.value]


@dataclass(frozen=True)
class ToolData:
    radius: float = 5.0
    length: float = 0.0
    name: str = ""


@dataclass(frozen=True)
class Snapshot:
    """Control state after a block (shown in the position and modal displays)."""

    line: int
    program: str
    position: Vec3  # programmed position in the active program coordinate system
    modal: tuple[tuple[str, str], ...]
    tool: str
    tool_radius: float
    feed: float
    speed: float
    spindle: str
    c_axis: float


@dataclass(frozen=True)
class Move:
    kind: MoveKind
    points: tuple[Vec3, ...]  # workpiece coordinates, first point = start
    line: int
    program: str
    state: int  # index into SimResult.snapshots
    feed: float  # mm/min used for the time estimate
    length: float
    duration: float  # seconds
    note: str = ""
    compensated: bool = False

    @property
    def start(self) -> Vec3:
        return self.points[0]

    @property
    def end(self) -> Vec3:
        return self.points[-1]


@dataclass(frozen=True)
class Message:
    severity: Severity
    text: str
    line: int
    program: str

    def __str__(self) -> str:
        where = f"Zeile {self.line}" if self.line else "Programm"
        return f"{self.severity.label}: {where} ({self.program}): {self.text}"


@dataclass(frozen=True)
class Dwell:
    seconds: float
    line: int
    state: int
    after_move: int  # number of moves emitted before the dwell


@dataclass
class SimConfig:
    """Assumptions of the simulation (not machine data)."""

    rapid_feed: float = 5000.0  # mm/min - FP2A value unknown, simulation assumption
    default_feed: float = 100.0
    start_position: Vec3 = (0.0, 0.0, 100.0)
    tools: dict[int, ToolData] = field(default_factory=dict)
    zero_points: dict[int, Vec3] = field(default_factory=dict)
    parameter_table: dict[str, str] = field(default_factory=dict)  # "P1" -> "50" / "(P2+1)" / "G81 ..."
    default_tool_radius: float = 5.0
    max_blocks: int = 200_000
    max_points: int = 2_000_000
    max_call_depth: int = 32


@dataclass
class SimResult:
    moves: list[Move] = field(default_factory=list)
    programmed: list[Move] = field(default_factory=list)  # contour without radius compensation
    snapshots: list[Snapshot] = field(default_factory=list)
    messages: list[Message] = field(default_factory=list)
    dwells: list[Dwell] = field(default_factory=list)
    executed_blocks: int = 0
    end_reason: str = ""
    parameters: dict[str, str] = field(default_factory=dict)
    program_labels: list[str] = field(default_factory=list)

    @property
    def total_time_s(self) -> float:
        return sum(m.duration for m in self.moves) + sum(d.seconds for d in self.dwells)

    @property
    def feed_length(self) -> float:
        return sum(m.length for m in self.moves if m.kind is MoveKind.FEED)

    @property
    def rapid_length(self) -> float:
        return sum(m.length for m in self.moves if m.kind is MoveKind.RAPID)

    @property
    def errors(self) -> list[Message]:
        return [m for m in self.messages if m.severity is Severity.ERROR]

    @property
    def warnings(self) -> list[Message]:
        return [m for m in self.messages if m.severity is Severity.WARNING]

    def bounds(self) -> tuple[Vec3, Vec3] | None:
        points = [p for m in self.moves for p in m.points]
        if not points:
            return None
        lo = (min(p[0] for p in points), min(p[1] for p in points), min(p[2] for p in points))
        hi = (max(p[0] for p in points), max(p[1] for p in points), max(p[2] for p in points))
        return lo, hi
