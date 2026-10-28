#!/usr/bin/env python3
"""Enforce the zero-third-party-dependency contract, including unused kinds."""
import json
import subprocess

metadata = json.loads(subprocess.check_output(
    ["cargo", "metadata", "--offline", "--locked", "--format-version", "1"], text=True))
packages = metadata["packages"]
if len(packages) != 1 or packages[0]["name"] != "opura" or packages[0]["dependencies"]:
    raise SystemExit("opura must be a single package with no Cargo dependencies")
print("Verified: opura has zero Cargo dependencies of any kind.")
