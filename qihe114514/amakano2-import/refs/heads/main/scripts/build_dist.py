import json
import shutil
import subprocess
import sys
import zipfile
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
DIST = ROOT / 'dist'
ENTRY = 'amakano2_import.wasm'


def cargo() -> str:
    candidate = Path.home() / '.cargo' / 'bin' / 'cargo.exe'
    return str(candidate) if candidate.exists() else 'cargo'


def packaged_files() -> list[tuple[Path, str]]:
    """随插件一起分发的文件，**以 manifest 的 `additional_files` 为准**。

    章节包（`packs/`）必须放进来：AstroBox 只允许插件用 std::fs 读自身目录，
    所以它们只能作为插件文件随包分发；`assets/` 下的壁纸同理。

    这里刻意不再自己拼一份清单：先前脚本硬编码「packs/*」，
    结果 manifest 里声明的 `assets/wallpaper.webp` 没被打进包 ——
    「声明了却不存在的文件」很可能让宿主直接拒绝安装，所以现在以 manifest 为单一事实来源，
    缺文件就地报错，杜绝两边漂移。
    """
    manifest = json.loads((ROOT / 'manifest.json').read_text(encoding='utf-8'))
    items: list[tuple[Path, str]] = []
    missing: list[str] = []
    for name in manifest.get('additional_files', []):
        candidate = ROOT / name
        if candidate.is_file():
            items.append((candidate, name))
        else:
            missing.append(name)
    if missing:
        raise SystemExit('manifest.additional_files 里声明了不存在的文件：' + '、'.join(missing))
    return items


def main() -> int:
    release = '--release' in sys.argv
    subprocess.run([cargo(), 'build', '--release'] if release else [cargo(), 'build'], cwd=ROOT, check=True)
    profile = 'release' if release else 'debug'
    source = ROOT / 'target' / 'wasm32-wasip2' / profile / 'amakano2_import.wasm'
    DIST.mkdir(exist_ok=True)
    shutil.copy2(ROOT / 'manifest.json', DIST / 'manifest.json')
    shutil.copy2(ROOT / 'icon.png', DIST / 'icon.png')
    shutil.copy2(source, DIST / ENTRY)

    extra = packaged_files()
    if '--package' in sys.argv:
        output = DIST / 'amakano2-import.abp'
        with zipfile.ZipFile(output, 'w', zipfile.ZIP_DEFLATED) as archive:
            for item in [DIST / 'manifest.json', DIST / 'icon.png', DIST / ENTRY]:
                archive.write(item, item.name)
            for path, name in extra:
                archive.write(path, name)
        print(f"{output} · {output.stat().st_size / 1048576:.2f} MB · 内置文件 {len(extra)} 个")
    else:
        print(f"随插件分发的文件 {len(extra)} 个（未打包）")
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
