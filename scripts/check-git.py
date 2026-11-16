"""Check offline Git CLI packaging/retry without opening a desktop or capture session."""
import argparse
import json
import os
import signal
from pathlib import Path
import subprocess
import sys

from vft_dev import ROOT, evidence_directory, executable, main, run, sha256, target_directory, write_fixture


def check():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--skip-build", action="store_true")
    args = parser.parse_args()
    if not args.skip_build:
        run("cargo", "build", "--release", "-p", "vft-send", "-p", "vft-recv")
    target = target_directory()
    sender, receiver = (executable(target, name) for name in ("vft-send", "vft-recv"))
    root = evidence_directory("git-cli")
    log = (root / "commands.jsonl").open("x", encoding="utf-8")

    def command(cwd, *argv, success=True, env=None):
        options = {"creationflags": subprocess.CREATE_NO_WINDOW} if sys.platform == "win32" else {}
        result = subprocess.run(list(map(str, argv)), cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                                capture_output=True, text=True, encoding="utf-8", errors="replace",
                                timeout=150, **options)
        log.write(json.dumps({"cwd": str(cwd), "args": list(map(str, argv)),
                              "status": result.returncode, "stdout": result.stdout,
                              "stderr": result.stderr}) + "\n")
        log.flush()
        if (result.returncode == 0) != success:
            raise RuntimeError(f"Unexpected exit {result.returncode}: {argv}\n{result.stdout}\n{result.stderr}")
        return result.stdout + result.stderr

    def git(repo, *argv):
        return command(repo, "git", *argv).strip()

    def require(condition, message):
        if not condition:
            raise RuntimeError(message)

    source = root / "source"
    source.mkdir()
    git(source, "init", "--template=", "--initial-branch=main")
    for key, value in (("user.name", "VFT Test"), ("user.email", "vft@example.invalid"),
                       ("commit.gpgsign", "false"), ("core.autocrlf", "false")):
        git(source, "config", key, value)
    (source / "text.txt").write_bytes(b"base\n")
    git(source, "add", ".")
    git(source, "commit", "-qm", "base")
    base = git(source, "rev-parse", "HEAD")
    destination = root / "receiver"
    git(root, "clone", "--no-hardlinks", source, destination)
    git(destination, "config", "core.autocrlf", "false")
    (source / "text.txt").write_bytes(b"target\n")
    (source / ".gitattributes").write_bytes(b"data.bin -text\n")
    write_fixture(source / "data.bin", 65536)
    git(source, "add", ".")
    git(source, "commit", "-qm", "target")
    target_head = git(source, "rev-parse", "HEAD")
    sub = source / "sub"
    sub.mkdir()
    package = root / "update.vftgit"
    command(sub, sender, "--git", "--git-base", base, "--git-package", package,
            "--git-prepare-only", "--no-tui")
    package_hash = sha256(package)
    if sys.platform != "win32":
        cancelled = root / "cancelled.vftgit"
        process = subprocess.Popen([str(sender), "--git", "--git-base", base, "--git-package",
                                    str(cancelled), "--git-prepare-only", "--no-tui"], cwd=source,
                                   stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                   text=True, encoding="utf-8")
        try:
            first_line = process.stdout.readline()
            require("Packing" in first_line, "missing preparation phase before interruption")
            process.send_signal(signal.SIGINT)
            stdout, stderr = process.communicate(timeout=15)
            require(process.returncode != 0 and "cancelled" in stderr, "CLI preparation ignored cancellation")
            require(not cancelled.exists(), "cancelled preparation left an incomplete package")
            log.write(json.dumps({"case": "prepare-interrupt", "stdout": first_line + stdout,
                                  "stderr": stderr, "status": process.returncode}) + "\n")
        finally:
            if process.poll() is None:
                process.kill()
                process.wait()
    command(root, sender, "--git", "--git-repo", source, "--git-base", base,
            "--git-package", package, "--git-prepare-only", "--no-tui", success=False)
    require(sha256(package) == package_hash, "existing package was modified")
    command(root, sender, "--git", "--git-base", base, "--git-prepare-only", "--no-tui", success=False)
    command(source, sender, "--git", "--git-prepare-only", "--no-tui", success=False)
    output = command(root, sender, "--git-repo", source, "--git", "--git-base", base,
                     "--git-prepare-only", "--no-tui")
    generated = Path(next(line.removeprefix("Git package ready: ") for line in output.splitlines()
                          if line.startswith("Git package ready: ")))
    require(generated.is_file(), "prepare-only did not retain its automatic package")
    blocker = root / "not-a-directory"
    blocker.write_bytes(b"file")
    environment = dict(os.environ)
    for key in (("TMP", "TEMP") if sys.platform == "win32" else ("TMPDIR",)):
        environment[key] = str(blocker)
    command(source, sender, "--git", "--git-base", base, "--git-prepare-only", "--no-tui",
            success=False, env=environment)
    (destination / "text.txt").write_bytes(b"keep local changes\n")
    command(destination, receiver, "--git", "--git-apply", package, "--no-tui", success=False)
    require((destination / "text.txt").read_bytes() == b"keep local changes\n", "local changes lost")
    require(sha256(package) == package_hash, "failed apply changed package")
    git(destination, "restore", "text.txt")  # Only this isolated fixture's deliberate edit.
    if sys.platform == "win32":
        command(root, receiver, "--git", "--git-repo", destination, "--git-branch", "main",
                "--git-apply", package, "--no-tui")
    else:
        process = subprocess.Popen([str(receiver), "--git-repo", str(destination), "--git-branch", "main",
                                    "--git-apply", str(package), "--no-tui"], cwd=root,
                                   stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                   text=True, encoding="utf-8")
        try:
            lines = []
            for line in process.stdout:
                lines.append(line)
                if line.startswith("Applying fast-forward"):
                    process.send_signal(signal.SIGINT)
                    break
            require(any(line.startswith("Applying fast-forward") for line in lines), "missing application phase")
            process.wait(timeout=30)
            stdout, stderr = process.stdout.read(), process.stderr.read()
            require(process.returncode == 0 and "COMPLETE" in stdout,
                    "interruption during update did not wait for completion: " + stderr)
            log.write(json.dumps({"case": "apply-interrupt", "stdout": "".join(lines) + stdout,
                                  "stderr": stderr, "status": process.returncode}) + "\n")
        finally:
            if process.poll() is None:
                process.kill()
                process.wait()
    repeat = command(destination, receiver, "--git", "--git-apply", package, "--no-tui")
    require("already current" in repeat, "repeat application was not idempotent")
    require(git(destination, "rev-parse", "HEAD") == target_head, "HEAD mismatch")
    require(git(destination, "rev-parse", "HEAD^{tree}") == git(source, "rev-parse", "HEAD^{tree}"), "tree mismatch")
    require(sha256(destination / "data.bin") == sha256(source / "data.bin"), "binary hash mismatch")
    empty_path = root / "no-update.vftgit"
    empty = command(source, sender, "--git", "--git-base", target_head, "--git-prepare-only",
                    "--git-package", empty_path, "--no-tui")
    require("No Git update needed" in empty and not empty_path.exists(), "empty delta wrote a package")
    report = {"git_version": git(root, "--version"), "base": base, "target": target_head,
              "package": str(package), "package_sha256": package_hash,
              "default_package_retained": str(generated), "binary_sha256": sha256(destination / "data.bin"),
              "validation": "offline CLI; no desktop, capture or remote session"}
    with (root / "report.json").open("x", encoding="utf-8") as stream:
        json.dump(report, stream, indent=2)
    log.close()
    print("Offline Git CLI checks passed; complete fixtures and diagnostics retained.")


if __name__ == "__main__":
    main(check)
