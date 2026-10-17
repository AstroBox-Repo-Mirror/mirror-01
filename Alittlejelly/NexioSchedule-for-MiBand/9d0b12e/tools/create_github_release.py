"""Create a GitHub Release for NexioSchedule-for-MiBand and upload the RPK assets.

用法：
    python tools/create_github_release.py                 # 默认发布 1.1.0
    python tools/create_github_release.py 1.2.0           # 指定版本
    python tools/create_github_release.py 1.2.0 --draft   # 先建草稿

说明：
    - 资产取自仓库里的 release/*.rpk（已随版本更新），上传时改成便于识别的英文名；
    - Release 说明默认读 tools/release-body.md（存在时），否则用最小默认文案；
    - 凭据优先用 `gh auth token`，取不到再退回 `git credential fill`。
"""

import argparse
import json
import subprocess
import sys
import urllib.error
import urllib.request
from pathlib import Path

REPO = "Alittlejelly/NexioSchedule-for-MiBand"
REPO_ROOT = Path(__file__).resolve().parent.parent

# (仓库内 rpk 路径, 上传后的资产名模板)
ASSETS = [
    ("release/Nexio 课程表（小米手环10Pro）.rpk", "NexioSchedule-MiBand-v{ver}-336x480-pro.rpk"),
    ("release/Nexio 课程表（小米手环 10、11）.rpk", "NexioSchedule-MiBand-v{ver}-212x520.rpk"),
]
BODY_FILE = REPO_ROOT / "tools" / "release-body.md"


def github_token() -> str:
    """优先用 gh CLI 的 token，其次读 git credential helper。"""
    try:
        out = subprocess.run(
            ["gh", "auth", "token"], capture_output=True, check=True
        ).stdout.decode().strip()
        if out:
            return out
    except (OSError, subprocess.CalledProcessError):
        pass

    proc = subprocess.run(
        ["git", "credential", "fill"],
        input=b"protocol=https\nhost=github.com\n\n",
        capture_output=True,
        check=True,
    )
    for line in proc.stdout.decode().splitlines():
        if line.startswith("password="):
            return line.split("=", 1)[1]
    raise SystemExit("找不到 GitHub 凭据：请先 `gh auth login` 或配置 git credential helper")


def request(method: str, url: str, token: str, data=None, content_type=None):
    headers = {
        "Authorization": f"Bearer {token}",
        "User-Agent": "nexio-miband-release",
        "Accept": "application/vnd.github+json",
    }
    if content_type:
        headers["Content-Type"] = content_type
    req = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=120) as resp:
            body = resp.read()
            return json.loads(body) if body else {}
    except urllib.error.HTTPError as e:
        err = e.read().decode(errors="replace")
        raise RuntimeError(f"HTTP {e.code} {method} {url}: {err}") from e


def api(method: str, path: str, token: str, payload=None):
    data = None
    ctype = None
    if payload is not None:
        data = json.dumps(payload).encode()
        ctype = "application/json"
    return request(method, f"https://api.github.com{path}", token, data=data, content_type=ctype)


def main() -> None:
    parser = argparse.ArgumentParser(description="发布 NexioSchedule-for-MiBand Release")
    parser.add_argument("version", nargs="?", default="1.1.0", help="版本号，例如 1.1.0")
    parser.add_argument("--draft", action="store_true", help="建为草稿，不立即公开")
    args = parser.parse_args()

    ver = args.version.lstrip("v")
    tag = f"v{ver}"
    body = BODY_FILE.read_text(encoding="utf-8") if BODY_FILE.is_file() else f"NexioSchedule MiBand v{ver}"

    missing = [p for p, _ in ASSETS if not (REPO_ROOT / p).is_file()]
    if missing:
        raise SystemExit("缺少 rpk：" + "、".join(missing) + "\n请先 `npm run release` 并把产物复制到 release/")

    token = github_token()

    try:
        rel = api("GET", f"/repos/{REPO}/releases/tags/{tag}", token)
        print(f"release 已存在 id={rel['id']} {rel['html_url']}")
    except RuntimeError as e:
        if "404" not in str(e):
            raise
        rel = api(
            "POST",
            f"/repos/{REPO}/releases",
            token,
            payload={
                "tag_name": tag,
                "name": f"NexioSchedule MiBand v{ver}",
                "body": body,
                "draft": args.draft,
                "prerelease": False,
            },
        )
        print(f"已创建 release id={rel['id']} {rel['html_url']}")

    upload_url = rel["upload_url"].split("{", 1)[0]
    for path, name_tpl in ASSETS:
        name = name_tpl.format(ver=ver)
        data = (REPO_ROOT / path).read_bytes()
        asset = request(
            "POST", f"{upload_url}?name={name}", token, data=data,
            content_type="application/octet-stream",
        )
        print(f"已上传 {asset.get('name')} -> {asset.get('browser_download_url')}")

    print(f"完成：https://github.com/{REPO}/releases/tag/{tag}")


if __name__ == "__main__":
    sys.exit(main())
