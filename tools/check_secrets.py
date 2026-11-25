#!/usr/bin/env python3
"""扫描仓库里疑似凭证与个人信息，CI 与 pre-commit 共用。

为什么需要：这个仓被 issue 讨论过「把明文 key 贴进公开仓库」的事，
而且代码注释里曾经写着具体校区和真机坐标。机器扫一遍比靠人记着便宜。

用法：
    python tools/check_secrets.py           # 扫 git 跟踪的文件
    python tools/check_secrets.py --staged   # 扫即将写入提交的 Git 索引内容
    python tools/check_secrets.py --all      # 连未跟踪也扫（发布前用）

命中返回 1，干净返回 0。
"""

import argparse
import codecs
import os
import re
import subprocess
import sys
from pathlib import Path

# 只扫可能是文本的文件；二进制直接跳过（证据包里的图片、zip 不算泄露面）
SKIP_SUFFIX = {
    ".png", ".jpg", ".jpeg", ".gif", ".ico", ".zip", ".gz", ".tar", ".exe",
    ".so", ".dylib", ".a", ".o", ".rlib", ".wasm", ".pdf", ".bin",
}

PATTERNS = [
    # GitHub 老式 PAT / 细粒度 token
    (
        re.compile(r"gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}"),
        "GitHub token",
    ),
    # 常见云厂商 key
    (re.compile(r"AKIA[0-9A-Z]{16}"), "AWS access key"),
    (
        re.compile(r"sk-(?:proj-[A-Za-z0-9_-]{20,}|[A-Za-z0-9]{20,})"),
        "OpenAI/DeepSeek 风格 key",
    ),
    (re.compile(r"glpat-[A-Za-z0-9_-]{20,}"), "GitLab token"),
    # 私钥块
    (re.compile(r"-----BEGIN (?:RSA |EC |OPENSSH |PGP )?PRIVATE KEY-----"), "私钥"),
    # 11 位手机号：前后都不能是数字，避免把 13 位毫秒戳拦下来
    (re.compile(r"(?<!\d)1[3-9]\d{9}(?!\d)"), "手机号"),
]

# 只放行命中的示例号码。同一行出现示例值时，仍要检查其他命中项。
EXAMPLE_PHONE_NUMBERS = {"13812348888", "13800000000", "13900000000"}

# 校验和、内容哈希这类长十六进制串里会出现「像手机号」的子串，先跳过
HEX_TOKEN = re.compile(r"[0-9a-f]{24,}")


# 全量扫描时跳过的目录：依赖树、构建产物、虚拟环境都不属于「本仓的代码」
SKIP_DIRS = {
    ".git", "target", "node_modules", "vendor", "dist", "build",
    ".venv", "venv", "__pycache__", ".cargo", ".idea", ".vscode",
}


def tracked_files() -> list[Path] | None:
    """git 跟踪的文件；不在 git 仓库或没装 git 时返回 None。

    release 的 zip/tar 包里没有 `.git`，而脚本是随包发出去的；pre-commit
    也直接调它。以前这里让 `FileNotFoundError` / `CalledProcessError` 一路
    冒到顶，用户看到的是 Python 堆栈，而不是「换个目录跑」。
    """
    try:
        out = git_output("ls-files", "-z")
    except (FileNotFoundError, subprocess.CalledProcessError, OSError):
        return None
    return [Path(os.fsdecode(p)) for p in out.split(b"\0") if p]


def git_output(*args: str) -> bytes:
    return subprocess.run(["git", *args], capture_output=True, check=True).stdout


def staged_files() -> dict[Path, str]:
    blobs = {}
    for entry in git_output("ls-files", "--stage", "-z").split(b"\0"):
        if not entry:
            continue
        header, path = entry.split(b"\t", 1)
        mode, oid, stage = header.split()
        if stage != b"0":
            raise ValueError("Git 索引含有尚未解决的冲突")
        if mode != b"160000":  # 子模块在索引中引用提交，不是文件内容。
            blobs[Path(os.fsdecode(path))] = oid.decode("ascii")
    return blobs


def walk_all() -> list[Path]:
    return [
        p
        for p in Path(".").rglob("*")
        if not any(part in SKIP_DIRS for part in p.parts)
    ]


def scan(paths: list[Path], staged: dict[Path, str] | None = None) -> list[str]:
    hits: list[str] = []
    for p in paths:
        if p.suffix.lower() in SKIP_SUFFIX:
            continue
        try:
            if staged is not None:
                content = git_output("cat-file", "blob", staged[p])
            elif p.is_file():
                content = p.read_bytes()
            else:
                continue
            is_utf16 = content.startswith((codecs.BOM_UTF16_LE, codecs.BOM_UTF16_BE))
            encoding = "utf-16" if is_utf16 else "utf-8"
            text = content.decode(encoding)
        except UnicodeDecodeError:
            continue
        except OSError:
            if staged is not None:
                raise
            continue
        for lineno, line in enumerate(text.splitlines(), 1):
            for pat, label in PATTERNS:
                for m in pat.finditer(line):
                    if label == "手机号" and (
                        m.group(0) in EXAMPLE_PHONE_NUMBERS
                        or any(h.start() <= m.start() and m.end() <= h.end() for h in HEX_TOKEN.finditer(line))
                    ):
                        continue
                    # 只报位置和类型，不把命中的原文打出来（日志本身也会被贴进 issue）
                    hits.append(f"{p}:{lineno}: {label}（{len(m.group(0))} 字符）")
                    break
    return hits


def main() -> int:
    parser = argparse.ArgumentParser(description="检查文件中疑似凭证或个人信息。")
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument("--staged", action="store_true", help="读取 Git 索引中的文件内容")
    modes.add_argument("--all", action="store_true", help="扫描工作目录，包括未跟踪的文件")
    args = parser.parse_args()

    if args.staged:
        try:
            staged = staged_files()
            paths = list(staged)
            hits = scan(paths, staged)
        except (OSError, subprocess.CalledProcessError, ValueError):
            print("无法读取 Git 索引，提交前检查未完成。", file=sys.stderr)
            return 1
    elif args.all:
        paths = walk_all()
        hits = scan(paths)
    else:
        got = tracked_files()
        if got is None:
            print("不在 git 仓库里（或没装 git），退回全量扫描；跳过的目录见 SKIP_DIRS。")
            paths = walk_all()
        else:
            paths = got
        hits = scan(paths)
    if hits:
        print("发现疑似凭证/个人信息，先处理再提交：\n")
        for h in hits:
            print("  " + h)
        print("\n确认是误报的话，改写成示例值，别留原文。")
        return 1
    print(f"扫描 {len(paths)} 个路径，未发现疑似凭证/个人信息。")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
