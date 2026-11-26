"""Time-based playback of a simulation result (independent of the GUI)."""

from __future__ import annotations

import bisect
import math
from dataclasses import dataclass

from deckellink.dialog11sim.geometry import Vec3, lerp
from deckellink.dialog11sim.model import Move, SimResult, Snapshot


@dataclass(frozen=True)
class Frame:
    """Tool state at a point in time."""

    time: float
    position: Vec3  # workpiece coordinates
    move_index: int  # index of the current move (-1 before the first move)
    fraction: float  # 0..1 within the current move
    line: int
    snapshot: Snapshot
    remaining: Vec3  # distance to the end of the current move per axis
    finished: bool


class Playback:
    """Maps simulation time to tool position, line and control state."""

    def __init__(self, result: SimResult, start_position: Vec3) -> None:
        self.result = result
        self.start_position = start_position
        self.starts: list[float] = []
        self.durations: list[float] = []
        dwell_after: dict[int, float] = {}
        for dwell in result.dwells:
            dwell_after[dwell.after_move] = dwell_after.get(dwell.after_move, 0.0) + dwell.seconds
        t = dwell_after.get(0, 0.0)
        for index, move in enumerate(result.moves):
            self.starts.append(t)
            duration = max(move.duration, 1e-6)
            self.durations.append(duration)
            t += duration + dwell_after.get(index + 1, 0.0)
        self.total = t
        self._cumulative: list[list[float]] = [_cumulative(m) for m in result.moves]

    def frame(self, time: float) -> Frame:
        moves = self.result.moves
        snapshots = self.result.snapshots
        time = min(max(time, 0.0), self.total)
        if not moves:
            snap = snapshots[-1] if snapshots else _empty_snapshot()
            return Frame(time, self.start_position, -1, 0.0, snap.line, snap, (0.0, 0.0, 0.0), True)
        index = bisect.bisect_right(self.starts, time) - 1
        if index < 0:
            snap = snapshots[0]
            return Frame(time, moves[0].start, -1, 0.0, 0, snap, (0.0, 0.0, 0.0), False)
        move = moves[index]
        fraction = min((time - self.starts[index]) / self.durations[index], 1.0)
        position = self.point_on(index, fraction)
        snap_index = min(move.state, len(snapshots) - 1)
        snap = snapshots[snap_index]
        end = move.end
        remaining = (end[0] - position[0], end[1] - position[1], end[2] - position[2])
        finished = time >= self.total - 1e-9
        return Frame(time, position, index, fraction, move.line, snap, remaining, finished)

    def point_on(self, index: int, fraction: float) -> Vec3:
        move = self.result.moves[index]
        cumulative = self._cumulative[index]
        target = cumulative[-1] * fraction
        k = bisect.bisect_left(cumulative, target)
        if k <= 0:
            return move.points[0]
        if k >= len(cumulative):
            return move.points[-1]
        seg = cumulative[k] - cumulative[k - 1]
        t = 0.0 if seg <= 0 else (target - cumulative[k - 1]) / seg
        return lerp(move.points[k - 1], move.points[k], t)

    def time_of_move_end(self, index: int) -> float:
        if not self.starts:
            return 0.0
        index = min(max(index, 0), len(self.starts) - 1)
        return self.starts[index] + self.durations[index]

    def next_block_end(self, time: float) -> float:
        """End time of the block that is active at ``time`` (single block mode)."""
        moves = self.result.moves
        if not moves:
            return self.total
        index = max(bisect.bisect_right(self.starts, time) - 1, 0)
        if time >= self.time_of_move_end(index) - 1e-9:
            index += 1
        if index >= len(moves):
            return self.total
        key = (moves[index].line, moves[index].state)
        while index + 1 < len(moves) and (moves[index + 1].line, moves[index + 1].state) == key:
            index += 1
        return self.time_of_move_end(index)

    def time_until_line(self, line: int) -> float:
        """End of the last move before the program first moves past ``line`` (transfer follow-up)."""
        reached = 0.0
        for index, move in enumerate(self.result.moves):
            if move.line > line:
                break
            reached = self.time_of_move_end(index)
        return reached

    def time_of_line(self, line: int) -> float | None:
        for index, move in enumerate(self.result.moves):
            if move.line == line:
                return self.starts[index]
        return None


def _cumulative(move: Move) -> list[float]:
    out = [0.0]
    for a, b in zip(move.points, move.points[1:], strict=False):
        out.append(out[-1] + math.dist(a, b))
    return out


def _empty_snapshot() -> Snapshot:
    return Snapshot(0, "", (0.0, 0.0, 0.0), (), "T0", 0.0, 0.0, 0.0, "M5", 0.0)


def format_duration(seconds: float) -> str:
    seconds = max(seconds, 0.0)
    hours, rest = divmod(round(seconds), 3600)
    minutes, secs = divmod(rest, 60)
    return f"{hours:d}:{minutes:02d}:{secs:02d}"
