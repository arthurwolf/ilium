#!/usr/bin/env python3
"""Run an explicit release unittest pattern through a whole-script dispatcher."""
import argparse
from pathlib import Path
import sys
import unittest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("pattern", help="unittest discovery filename pattern")
    arguments = parser.parse_args()
    directory = Path(__file__).resolve().parent
    sys.path.insert(0, str(directory))
    suite = unittest.defaultTestLoader.discover(str(directory), pattern=arguments.pattern)
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    return 0 if result.wasSuccessful() and result.testsRun else 1


if __name__ == "__main__":
    raise SystemExit(main())
