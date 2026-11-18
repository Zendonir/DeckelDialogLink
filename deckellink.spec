# -*- mode: python ; coding: utf-8 -*-
# PyInstaller build specification for DeckelLink.
#
# Build:  pyinstaller --noconfirm --clean deckellink.spec
# Result: dist/DeckelLink/DeckelLink(.exe)  (onedir, portable folder)
#
# Onedir is used deliberately: onefile builds with PySide6 unpack ~100 MB into a
# temp directory on every start (slow, often flagged by virus scanners) and are
# less reliable on locked-down Windows machines. See docs/INSTALLATION.md.
import sys
from pathlib import Path

ROOT = Path(SPECPATH)  # noqa: F821 - provided by PyInstaller
SRC = ROOT / "src"
sys.path.insert(0, str(SRC))

from deckellink.version import APP_AUTHOR, APP_NAME, __version__  # noqa: E402

RESOURCES = SRC / "deckellink" / "resources"
ICON = RESOURCES / "icons" / "deckellink.ico"

version_info = None
if sys.platform == "win32":
    from PyInstaller.utils.win32.versioninfo import (
        FixedFileInfo,
        StringFileInfo,
        StringStruct,
        StringTable,
        VarFileInfo,
        VarStruct,
        VSVersionInfo,
    )

    numbers = tuple(int(part) for part in __version__.split(".")[:3]) + (0,)
    version_info = VSVersionInfo(
        ffi=FixedFileInfo(filevers=numbers, prodvers=numbers),
        kids=[
            StringFileInfo(
                [
                    StringTable(
                        "040704B0",
                        [
                            StringStruct("CompanyName", APP_AUTHOR),
                            StringStruct("FileDescription", "DeckelLink - NC-Datenuebertragung DECKEL FP2A / DIALOG 11"),
                            StringStruct("FileVersion", __version__),
                            StringStruct("InternalName", APP_NAME),
                            StringStruct("LegalCopyright", "MIT License"),
                            StringStruct("OriginalFilename", f"{APP_NAME}.exe"),
                            StringStruct("ProductName", APP_NAME),
                            StringStruct("ProductVersion", __version__),
                        ],
                    )
                ]
            ),
            VarFileInfo([VarStruct("Translation", [0x0407, 1200])]),
        ],
    )

a = Analysis(  # noqa: F821
    [str(SRC / "deckellink" / "__main__.py")],
    pathex=[str(SRC)],
    binaries=[],
    datas=[
        (str(RESOURCES / "profiles" / "*.json"), "deckellink/resources/profiles"),
        (str(RESOURCES / "icons" / "*"), "deckellink/resources/icons"),
    ],
    hiddenimports=["serial.tools.list_ports"],
    hookspath=[],
    runtime_hooks=[],
    excludes=[
        "tkinter",
        "PySide6.QtWebEngineCore",
        "PySide6.QtWebEngineWidgets",
        "PySide6.QtWebEngineQuick",
        "PySide6.QtWebChannel",
        "PySide6.QtQml",
        "PySide6.QtQuick",
        "PySide6.QtQuickWidgets",
        "PySide6.Qt3DCore",
        "PySide6.Qt3DRender",
        "PySide6.QtMultimedia",
        "PySide6.QtPdf",
        "PySide6.QtCharts",
        "PySide6.QtDataVisualization",
        "PySide6.QtBluetooth",
        "PySide6.QtSql",
        "PySide6.QtTest",
    ],
    noarchive=False,
)
pyz = PYZ(a.pure)  # noqa: F821

exe = EXE(  # noqa: F821
    pyz,
    a.scripts,
    [],
    exclude_binaries=True,
    name=APP_NAME,
    debug=False,
    strip=False,
    upx=False,
    console=False,
    icon=str(ICON) if ICON.is_file() else None,
    version=version_info,
)

coll = COLLECT(  # noqa: F821
    exe,
    a.binaries,
    a.datas,
    strip=False,
    upx=False,
    name=APP_NAME,
)
