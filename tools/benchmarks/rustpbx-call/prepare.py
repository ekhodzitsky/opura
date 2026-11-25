"""Build pinned RustPBX, a decoder-only variant, and a loopback sipbot client."""
import argparse
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
from urllib.request import urlopen

PACKAGES = {
    'rustpbx-0.5.0': 'd76a646031a7015395a9ce393e2a1421f08f63d58b16f787e2b08a3fe3f083ec',
    'sipbot-0.2.82': 'e9d021aa4dcea35b1bf58ba956afdfae8ac4b8b0792a1743d7dac313aa388559',
    'audio-codec-0.4.6': '81c2b027ac91f3fb0c9c001e43538ea4523c7c3e2bc3feb4f380abac07604490',
    'opura-0.2.8': '5840f326085a1ec444e1996264fad0b47c94405dc04db74f9f0f40f9b367f192',
}
SPEECH = 'https://raw.githubusercontent.com/restsend/rustpbx/339f75b99341bcfc440545d70294f5de55652525/fixtures/hello_book_course_zh_16k.wav'
SPEECH_SHA = 'd9d19ffacfd369cd8996dd302dd9deadb0531fba537aef232cad3fa7f89f45e2'


def download(url, destination, checksum):
    with urlopen(url, timeout=60) as response:
        data = response.read()
    if hashlib.sha256(data).hexdigest() != checksum:
        raise ValueError(f'Checksum mismatch for {destination.name}')
    destination.write_bytes(data)


def replace(path, before, after, count=1):
    text = path.read_text()
    if text.count(before) != count:
        raise ValueError(f'Unexpected upstream source in {path}')
    path.write_text(text.replace(before, after))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path, help='New scratch directory')
    opts = parser.parse_args()
    root = opts.output.resolve()
    root.mkdir(parents=True, exist_ok=False)
    for package, checksum in PACKAGES.items():
        name, _ = package.rsplit('-', 1)
        archive = root / (package + '.crate')
        download(f'https://static.crates.io/crates/{name}/{package}.crate', archive, checksum)
        with tarfile.open(archive, 'r:gz') as tar:
            tar.extractall(root, filter='data')
    download(SPEECH, root / 'speech.wav', SPEECH_SHA)

    # sipbot normally omits loopback when it enumerates ICE candidates.
    replace(root/'sipbot-0.2.82/src/media.rs', 'config.ice_servers = vec![];',
            'config.bind_ips = vec![std::net::Ipv4Addr::LOCALHOST.into()];\n        config.ice_servers = vec![];', 2)
    codec = root / 'audio-codec-0.4.6'
    code = codec / 'src/opus.rs'
    replace(code, 'Application, OpusDecoder as OpusDecoderRaw, OpusEncoder', 'Application, OpusEncoder')
    shim = Path(__file__).with_name('decoder.rs').read_text()
    replace(code, 'pub struct OpusDecoder {', shim + '\npub struct OpusDecoder {')
    with (codec/'Cargo.toml').open('a') as manifest:
        manifest.write('\n[dependencies.opura]\npath = "../opura-0.2.8"\n')

    env = dict(os.environ, CARGO_TARGET_DIR=str(root/'target'), CARGO_PROFILE_RELEASE_LTO='false',
               CARGO_PROFILE_RELEASE_DEBUG='0', CARGO_PROFILE_RELEASE_OPT_LEVEL='2')
    def build(package, binary, patch=False):
        args = ['cargo']
        if patch:
            args += ['--config', f'patch.crates-io.audio-codec.path="{codec.as_posix()}"']
        args += ['build', '--release', '--bin', binary, '--no-default-features', '-j4']
        args += ['--offline' if patch else '--locked']
        subprocess.run(args, cwd=root/package, env=env, check=True)
        return root/'target/release'/binary

    shutil.copy2(build('sipbot-0.2.82', 'sipbot'), root/'sipbot')
    shutil.copy2(build('rustpbx-0.5.0', 'rustpbx'), root/'stock')
    shutil.copy2(build('rustpbx-0.5.0', 'rustpbx', patch=True), root/'opura')


if __name__ == '__main__':
    main()
