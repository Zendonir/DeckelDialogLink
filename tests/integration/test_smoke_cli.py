"""Headless smoke test through the real CLI entry point."""

from __future__ import annotations

import json
import logging
from pathlib import Path

import pytest

from deckellink.app import main, parse_args
from deckellink.simulation.runner import run_smoke_test
from deckellink.storage.paths import AppPaths


def _drop_file_handlers() -> None:
    root = logging.getLogger()
    for handler in list(root.handlers):
        if getattr(handler, "_deckellink_handler", False):
            root.removeHandler(handler)
            handler.close()


def test_smoke_test_cli(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    report = tmp_path / "report.json"
    try:
        code = main(["--data-dir", str(tmp_path / "data"), "--smoke-test", "--smoke-report", str(report)])
    finally:
        _drop_file_handlers()
    assert code == 0
    data = json.loads(report.read_text(encoding="utf-8"))
    assert data["ok"] is True
    steps = {s["step"]: s for s in data["steps"]}
    assert set(steps) == {
        "profiles_loaded",
        "validated",
        "simulated_send",
        "simulated_receive",
        "stored",
        "original_unchanged",
        "backup",
    }
    assert len(steps["stored"]["sha256"]) == 64
    assert '"ok": true' in capsys.readouterr().out
    assert (tmp_path / "data" / "logs" / "deckellink.log").is_file()
    log_text = (tmp_path / "data" / "logs" / "deckellink.log").read_text(encoding="utf-8")
    assert "N20 G00" not in log_text  # NC content never in standard log


def test_smoke_report_failure(tmp_path: Path) -> None:
    blocker = tmp_path / "blocked"
    blocker.write_text("file instead of directory", encoding="utf-8")
    result = run_smoke_test(AppPaths.from_root(blocker))
    assert not result.ok
    assert result.steps[-1]["step"] == "exception"


def test_cli_args() -> None:
    args = parse_args(["--simulate", "--data-dir", "x"])
    assert args.simulate and args.data_dir == Path("x") and not args.smoke_test
    with pytest.raises(SystemExit):
        parse_args(["--version"])
