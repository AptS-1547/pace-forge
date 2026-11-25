import codecs
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from typing import Callable


SCANNER = Path(__file__).with_name("check_secrets.py").resolve()
HOOK = SCANNER.parent.parent / ".githooks" / "pre-commit"
WINDOWS_HOOK = HOOK.parent / "windows" / HOOK.name
FAKE_KEY = "sk-" + "Zz" * 12


class StagedSecretsTest(unittest.TestCase):
    def setUp(self) -> None:
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.repo = Path(directory.name)
        # Keep a parent Git operation from sending commands to its worktree or index.
        inherited = {
            name: value for name, value in os.environ.items() if not name.startswith("GIT_")
        }
        self.env = {
            **inherited,
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_CONFIG_GLOBAL": os.devnull,
            "PYTHONIOENCODING": "utf-8",
        }
        self.git("init", "-q")
        self.path = self.repo / "test file.txt"

    def git(self, *args: str) -> None:
        subprocess.run(
            ["git", *args], cwd=self.repo, env=self.env, check=True, capture_output=True
        )

    def scan(self, *args: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(SCANNER), *args],
            cwd=self.repo,
            env=self.env,
            capture_output=True,
            text=True,
            encoding="utf-8",
        )

    def test_blocks_a_key_removed_only_from_the_working_copy(self) -> None:
        # A sample phone number and hash must not suppress another hit on the line.
        self.path.write_text("13800000000 " + "a" * 24 + " " + FAKE_KEY, encoding="utf-8")
        self.git("add", self.path.name)
        self.path.write_text("clean working copy", encoding="utf-8")

        result = self.scan("--staged")
        self.assertEqual(result.returncode, 1)
        self.assertIn(self.path.name + ":1:", result.stdout)
        self.assertNotIn(FAKE_KEY, result.stdout + result.stderr)
        self.assertEqual(self.scan().returncode, 0)

    def test_unstaged_key_does_not_block_a_clean_index(self) -> None:
        self.path.write_text("clean index", encoding="utf-8")
        self.git("add", self.path.name)
        self.path.write_text(FAKE_KEY, encoding="utf-8")

        self.assertEqual(self.scan("--staged").returncode, 0)
        self.assertEqual(self.scan().returncode, 1)

    def test_detects_fine_grained_github_and_openai_project_keys(self) -> None:
        github = "github_pat_" + "G" * 22 + "_" + "x" * 59
        openai = "sk-proj-" + "Ab_0-" * 20
        self.path.write_text(f"{github}\n{openai}\n", encoding="utf-8")
        self.git("add", self.path.name)

        result = self.scan("--staged")
        self.assertEqual(result.returncode, 1)
        self.assertIn(f"{self.path.name}:1: GitHub token", result.stdout)
        self.assertIn(f"{self.path.name}:2: OpenAI/DeepSeek", result.stdout)
        self.assertFalse(any(key in result.stdout + result.stderr for key in (github, openai)))

    def test_detects_keys_in_utf16_files_with_bom(self) -> None:
        encodings = (("utf-16-le", codecs.BOM_UTF16_LE), ("utf-16-be", codecs.BOM_UTF16_BE))
        for encoding, bom in encodings:
            with self.subTest(encoding=encoding):
                self.path.write_bytes(bom + ("header\n" + FAKE_KEY).encode(encoding))
                self.git("add", self.path.name)

                for mode in ("--staged", "--all"):
                    result = self.scan(mode)
                    self.assertEqual(result.returncode, 1)
                    self.assertIn(f"{self.path.name}:2: OpenAI/DeepSeek", result.stdout)
                    self.assertNotIn(FAKE_KEY, result.stdout + result.stderr)

    def test_git_commit_runs_pre_commit_hook(self) -> None:
        source = WINDOWS_HOOK if os.name == "nt" else HOOK
        if os.name == "nt":
            self.env["PATH"] = os.pathsep.join(
                (str(Path(sys.executable).parent), self.env.get("PATH", ""))
            )
        self.install_hook(source)
        for name, value in (
            ("core.hooksPath", ".githooks"),
            ("user.name", "Scanner test"),
            ("user.email", "scanner@example.invalid"),
            ("commit.gpgSign", "false"),
        ):
            self.git("config", name, value)
        self.assert_hook_round_trip(lambda: self.run_command(
            "git", "commit", "-qm", "Test staged secret hook"
        ))

    def test_windows_python_hook_body(self) -> None:
        hook = self.install_hook(WINDOWS_HOOK)
        self.assert_hook_round_trip(lambda: self.run_command(sys.executable, str(hook)))

    def install_hook(self, source: Path) -> Path:
        hook = self.repo / ".githooks" / HOOK.name
        hook.parent.mkdir()
        shutil.copyfile(source, hook)
        hook.chmod(0o755)
        scanner = self.repo / "tools" / SCANNER.name
        scanner.parent.mkdir()
        shutil.copyfile(SCANNER, scanner)
        return hook

    def assert_hook_round_trip(
        self, run_hook: Callable[[], subprocess.CompletedProcess[str]]
    ) -> None:
        self.path.write_text(FAKE_KEY, encoding="utf-8")
        self.git("add", self.path.name)
        self.path.write_text("clean", encoding="utf-8")
        rejected = run_hook()
        diagnostic = rejected.stdout + rejected.stderr
        self.assertIn(
            self.path.name + ":1:",
            diagnostic.replace(FAKE_KEY, "[test key]"),
            "没有收到扫描器报告，本次无法确认钩子已运行",
        )
        self.assertNotEqual(rejected.returncode, 0)
        self.assertFalse(FAKE_KEY in diagnostic, "钩子打印了测试密钥原文")

        self.git("add", self.path.name)
        accepted = run_hook()
        report = accepted.stdout + accepted.stderr
        self.assertEqual(accepted.returncode, 0, report)
        self.assertIn("未发现疑似凭证", report)

    def run_command(self, *args: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            args,
            cwd=self.repo,
            env=self.env,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
        )


if __name__ == "__main__":
    unittest.main()
