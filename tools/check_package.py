#!/usr/bin/env python3
"""Check the distribution archive from the repository on the pinned toolchain."""

import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import tomllib
import wave


ROOT = Path(__file__).resolve().parents[1]


def main():
    package = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]
    prefix = f"{package['name']}-{package['version']}"
    with tempfile.TemporaryDirectory(prefix="opura-package-") as directory:
        work = Path(directory)
        target = work / "target"
        env = {**os.environ, "CARGO_TARGET_DIR": str(target)}

        def cargo(*args):
            subprocess.run(["cargo", *args], cwd=ROOT, env=env, check=True)

        cargo("package", "--locked", "--offline", "--allow-dirty")
        archive = target / "package" / f"{prefix}.crate"
        size = archive.stat().st_size
        if size > 10_000_000:
            raise SystemExit(f"Compressed crate exceeds 10 MB: {size} bytes")

        # Verify the archive itself, independently of Cargo's verification tree.
        with tarfile.open(archive, "r:gz") as bundle:
            bundle.extractall(work, filter="data")
        source = work / prefix
        for name in ("LICENSE", "THIRD_PARTY_NOTICES.md"):
            if (source / name).read_bytes() != (ROOT / name).read_bytes():
                raise SystemExit(f"Packaged {name} differs from the repository")

        manifest = str(source / "Cargo.toml")
        env["RUSTDOCFLAGS"] = "-D warnings"
        cargo("doc", "--manifest-path", manifest, "--offline", "--locked", "--no-deps")
        cargo("test", "--manifest-path", manifest, "--offline", "--locked", "--doc")
        cargo("build", "--manifest-path", manifest, "--offline", "--locked", "--release", "--bin", "opura")
        binary = target / "release" / ("opura.exe" if os.name == "nt" else "opura")
        version = subprocess.check_output([binary, "--version"], text=True).strip()
        if version != f"opura {package['version']}":
            raise SystemExit(f"Unexpected packaged CLI version: {version}")

        fixture = ROOT / "tests/audio/celt-stereo.opus"
        output = work / "decoded.wav"
        subprocess.run([binary, "decode", fixture, output], check=True)
        with wave.open(str(output)) as wav:
            if (wav.getnchannels(), wav.getsampwidth(), wav.getframerate()) != (2, 2, 48_000):
                raise SystemExit("Unexpected PCM format from packaged CLI")
            if wav.readframes(wav.getnframes()) != fixture.with_suffix(".pcm").read_bytes():
                raise SystemExit("Packaged CLI output differs from reference PCM")

        # The example and dependency both come from the unpacked distribution.
        readme = (source / "README.md").read_text()
        example = readme.split("```rust\n", 1)[1].split("\n```", 1)[0]
        consumer = work / "consumer"
        (consumer / "src").mkdir(parents=True)
        (consumer / "src/main.rs").write_text(example + "\n")
        consumer_manifest = consumer / "Cargo.toml"
        consumer_manifest.write_text(
            '[package]\nname = "opura-consumer"\nversion = "0.0.0"\nedition = "2024"\n'
            f'\n[dependencies]\nopura = {{ path = {json.dumps(str(source))} }}\n'
        )
        cargo("run", "--manifest-path", str(consumer_manifest), "--offline", "--release")
        print(f"Verified {prefix}.crate ({size:,} bytes), CLI, rustdoc and external consumer.")


if __name__ == "__main__":
    main()
