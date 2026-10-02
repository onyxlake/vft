"""Run the shared workspace checks on Windows or Linux."""

import argparse
import sys
from vft_dev import ROOT, main, run


def check():
    argparse.ArgumentParser(description=__doc__).parse_args()
    run(sys.executable, "-m", "unittest", "discover", "-s", ROOT / "scripts", "-p", "test_*.py")
    run("cargo", "fmt", "--all", "--", "--check")
    run("cargo", "check", "--workspace")
    run("cargo", "clippy", "--workspace", "--all-targets", "--", "-D", "warnings")
    run("cargo", "test", "--workspace")


if __name__ == "__main__":
    main(check)
