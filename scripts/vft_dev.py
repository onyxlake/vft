"""Standard-library helpers for the repository's Python 3.9+ development tools."""

import csv
import hashlib
from pathlib import Path
import subprocess
import sys
import tempfile
from datetime import datetime, timezone

ROOT = Path(__file__).resolve().parent.parent


def run(*args):
    """Run without a shell, from the repository root, preserving failure status."""
    print("+ " + " ".join(map(str, args)), flush=True)
    subprocess.run([str(arg) for arg in args], cwd=ROOT, check=True)


def main(action):
    try:
        action()
    except subprocess.CalledProcessError as error:
        print(f"Command failed with exit code {error.returncode}", file=sys.stderr)
        sys.exit(error.returncode if error.returncode > 0 else 1)
    except (OSError, RuntimeError, ValueError) as error:
        print(f"Error: {error}", file=sys.stderr)
        sys.exit(1)
    except KeyboardInterrupt:
        sys.exit(130)


def require_windows():
    if sys.platform != "win32":
        raise RuntimeError("This check requires an unlocked interactive Windows desktop; "
                           "use check.py and check-web.py for Linux development.")


def target_directory():
    # Honor Cargo's target-dir configuration as well as CARGO_TARGET_DIR.
    import json
    try:
        result = subprocess.run(
            ["cargo", "metadata", "--no-deps", "--format-version", "1"],
            cwd=ROOT, check=True, capture_output=True, text=True, encoding="utf-8",
        )
    except subprocess.CalledProcessError as error:
        if error.stderr:
            print(error.stderr, file=sys.stderr, end="")
        raise
    return Path(json.loads(result.stdout)["target_directory"])


def executable(target, name, configuration="release"):
    return target / configuration / (name + (".exe" if sys.platform == "win32" else ""))


def evidence_directory(prefix):
    output = ROOT / "output"
    output.mkdir(exist_ok=True)
    stamp = datetime.now(timezone.utc).strftime("%Y%m%d-%H%M%S")
    path = Path(tempfile.mkdtemp(prefix=f"{prefix}-{stamp}-", dir=output))
    print(f"Evidence: {path}", flush=True)
    return path


def write_fixture(path, size):
    """Portable deterministic bytes in 64 KiB chunks; distinct from old .NET fixtures."""
    if size < 0:
        raise ValueError("File size must be nonnegative")
    with path.open("xb") as stream:
        for offset in range(0, size, 65536):
            block = hashlib.shake_256(f"VFT-python-fixture-v1:{offset}".encode("ascii"))
            stream.write(block.digest(min(65536, size - offset)))


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(65536), b""):
            digest.update(block)
    return digest.hexdigest().upper()


def read_stats(path):
    with path.open(encoding="utf-8-sig", newline="") as stream:
        rows = list(csv.DictReader(stream))
    if len(rows) != 1:
        raise RuntimeError(f"Expected one statistics row: {path}")
    return rows[0]


def write_csv(path, rows):
    with path.open("x", encoding="utf-8", newline="") as stream:
        writer = csv.DictWriter(stream, fieldnames=list(rows[0]))
        writer.writeheader()
        writer.writerows(rows)


class LoggedProcess:
    """Own only the child we launch, keeping logs on failure and interruption."""

    def __init__(self, command, directory, name, *, cwd=None):
        self.command = list(map(str, command))
        self.directory = directory
        self.name = name
        self.cwd = ROOT if cwd is None else cwd

    def __enter__(self):
        self.stdout = (self.directory / f"{self.name}.log").open("xb")
        try:
            self.stderr = (self.directory / f"{self.name}.err").open("xb")
            try:
                options = {}
                if sys.platform == "win32":
                    startup = subprocess.STARTUPINFO()
                    startup.dwFlags |= subprocess.STARTF_USESHOWWINDOW
                    startup.wShowWindow = 0  # Hide console helpers; sender explicitly shows its GUI.
                    options["startupinfo"] = startup
                self.process = subprocess.Popen(
                    self.command, cwd=self.cwd, stdin=subprocess.DEVNULL,
                    stdout=self.stdout, stderr=self.stderr, **options,
                )
            except BaseException:
                self.stderr.close()
                raise
        except BaseException:
            self.stdout.close()
            raise
        return self.process

    def __exit__(self, *_):
        try:
            if self.process.poll() is None:
                self.process.terminate()
                try:
                    self.process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    self.process.kill()
                    self.process.wait()
        finally:
            self.stdout.close()
            self.stderr.close()


def nonnegative(value):
    import argparse
    number = int(value)
    if number < 0:
        raise argparse.ArgumentTypeError("must be nonnegative")
    return number


def positive(value):
    import argparse
    number = int(value)
    if number <= 0:
        raise argparse.ArgumentTypeError("must be positive")
    return number
