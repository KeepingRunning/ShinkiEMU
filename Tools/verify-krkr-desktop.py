"""Audit the actual KiriKiri desktop package without opening a game/session."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import zipfile


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def run(*args, env=None):
    result = subprocess.run([str(value) for value in args], capture_output=True, env=env)
    text = (result.stdout + result.stderr).decode(errors='replace')
    if result.returncode:
        raise RuntimeError((args, result.returncode, text))
    return text


def plain_member(name):
    assert not name.startswith(('/', '\\')) and '\\' not in name
    assert '..' not in Path(name).parts and ':' not in name, name


def query_abi(root, path, temporary):
    # A clean native process cannot inherit Python hashlib's loaded OpenSSL ABI.
    include = root / 'ThirdParty/kirikiri2/shinki/native/include'
    source = root / 'Tools/krkr-query-probe.c'
    probe = temporary / ('query-probe.exe' if sys.platform == 'win32' else 'query-probe')
    if sys.platform == 'win32':
        run('cl.exe', '/nologo', '/O2', '/TC', '/std:c17', '/I' + str(include), source,
            '/Fo' + str(temporary / 'query-probe.obj'), '/Fe' + str(probe))
    else:
        libraries = ['-ldl'] if sys.platform.startswith('linux') else []
        run('cc', '-std=c11', '-O2', '-I', include, source, '-o', probe, *libraries)
    environment = dict(os.environ)
    environment.pop('LD_LIBRARY_PATH', None)
    return json.loads(run(probe, path, env=environment))


def main(args):
    root = args.repository.resolve()
    releases = list((root / '.work/release').glob('core-kirikiroid2-web-*/' + args.target + '/release.json'))
    assert len(releases) == 1, releases
    release_path = releases[0]
    record = json.loads(release_path.read_bytes())
    product = run('git', '-C', root, 'rev-parse', 'HEAD').strip()
    assert record['source_commit'] == product
    assert record['identity']['release_sequence'] == 2
    binary = release_path.parent / record['archive']['name']
    assert binary.stat().st_size == record['archive']['size']
    assert digest(binary) == record['archive']['sha256']
    receipt = json.loads((root / '.work/build' / args.target / 'kirikiroid2-web.json').read_bytes())
    assert receipt['source_commit'] == record['core_source']['rev'] and not receipt['source_dirty']
    temp_root = root / '.work/krkr-desktop-audit'
    temp_root.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(dir=temp_root) as temporary:
        stage = Path(temporary)
        with zipfile.ZipFile(binary) as archive:
            assert archive.testzip() is None
            names = archive.namelist()
            assert len(names) == len(set(names))
            for name in names:
                plain_member(name)
            archive.extractall(stage)
        core = json.loads((stage / 'core.json').read_bytes())
        provenance = json.loads((stage / 'build.json').read_bytes())
        assert core['identity'] == record['identity'] and core['abi'] == 'shinki.emu.native.c.v3'
        assert provenance['source_commit'] == record['core_source']['rev'] and not provenance['source_dirty']
        indexed = {item['path']: item['sha256'] for item in core['files']}
        assert len(indexed) == len(core['files'])
        actual = {path.relative_to(stage).as_posix() for path in stage.rglob('*') if path.is_file() and path.name != 'core.json'}
        assert actual == set(indexed)
        for name, sha in indexed.items():
            assert digest(stage / name) == sha, name
        assets = {}
        for group in ('resources', 'licenses'):
            for asset in receipt['core_build'][group]:
                source = Path(asset['source'])
                destination = stage / asset['dest']
                files = [source] if source.is_file() else [path for path in source.rglob('*') if path.is_file()]
                for path in files:
                    out = destination if source.is_file() else destination / path.relative_to(source)
                    assert out.is_file() and digest(path) == digest(out), ('original asset', path, out)
                    assets[out.relative_to(stage).as_posix()] = digest(out)
        original_ui = root / 'ThirdParty/kirikiri2/ui/cocos-studio'
        ui_files = [path for path in original_ui.rglob('*') if path.is_file()]
        for path in ui_files:
            assert digest(path) == digest(stage / 'Resources' / path.relative_to(original_ui)), path
        assert sum(name.startswith('Resources/') for name in indexed) == len(ui_files)
        native = []
        loader_environment = dict(os.environ)
        loader_environment.pop('LD_LIBRARY_PATH', None)
        for path in sorted(stage.rglob('*')):
            if not path.is_file():
                continue
            with path.open('rb') as stream:
                magic = stream.read(4)
            if magic == b'\x7fELF':
                dynamic = run('readelf', '-d', path)
                closure = run('ldd', '-r', path, env=loader_environment)
                assert not re.search(r'not found|undefined symbol|version .*not found', closure), (path, closure)
                assert all(part == '$ORIGIN' or part.startswith('$ORIGIN/') for value in re.findall(r'\((?:RUNPATH|RPATH)\).*?\[(.*?)\]', dynamic) for part in value.split(':'))
                symbols = run('nm', '-a', '-C', path)
                assert not re.search(r'spine::|\bsp(?:Skeleton|Animation|Atlas|Bone|Slot|Attachment|IkConstraint|TransformConstraint|PathConstraint|Skin)\w*', symbols), path
                native.append(dict(path=path.relative_to(stage).as_posix(), sha256=digest(path), runtime_closure=closure, spine_matches=0))
            elif magic in (b'\xcf\xfa\xed\xfe', b'\xfe\xed\xfa\xcf', b'\xca\xfe\xba\xbe', b'\xbe\xba\xfe\xca'):
                run('codesign', '--verify', '--strict', path)
                architectures = run('lipo', '-archs', path).split()
                assert architectures == [('arm64' if args.target.startswith('aarch64') else 'x86_64')], architectures
                commands = run('otool', '-l', path)
                paths = re.findall(r'cmd LC_RPATH\n\s+cmdsize \d+\n\s+path (.*?) \(offset', commands)
                dependencies = []
                for line in run('otool', '-L', path).splitlines()[1:]:
                    dependency = line.strip().split(' (', 1)[0]
                    if not dependency:
                        continue
                    if dependency.startswith(('/usr/lib/', '/System/Library/')):
                        dependencies.append(dict(name=dependency, system=True))
                        continue
                    if dependency.startswith('@loader_path/'):
                        candidates = [path.parent / dependency.removeprefix('@loader_path/')]
                    elif dependency.startswith('@rpath/'):
                        candidates = [Path(value.replace('@loader_path', str(path.parent))) / dependency.removeprefix('@rpath/') for value in paths]
                    else:
                        raise AssertionError(('absolute/nonrelocatable dependency', path, dependency))
                    assert any(candidate.is_file() and (candidate.resolve() == path.resolve() or stage.resolve() in candidate.resolve().parents) for candidate in candidates), (path, dependency, candidates)
                    dependencies.append(dict(name=dependency, system=False))
                native.append(dict(path=path.relative_to(stage).as_posix(), sha256=digest(path), architectures=architectures, strict_codesign=True, dependencies=dependencies))
            elif magic[:2] == b'MZ':
                # The real loader resolves all imports below when loading the core.
                native.append(dict(path=path.relative_to(stage).as_posix(), sha256=digest(path), authenticode='not configured; unsigned distribution contract'))
        assert native
        abi = query_abi(root, stage / core['library'], stage)
        result = dict(product=product, fork=record['core_source']['rev'], target=args.target,
                      archive_sha256=digest(binary), indexed_files=len(indexed),
                      original_asset_hashes=assets, original_ui_resources=len(ui_files),
                      native_objects=native, actual_abi_query=abi)
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(result, indent=2), encoding='utf-8')
        print(json.dumps(dict(product=product, target=args.target, native_objects=len(native), resources=len(ui_files), actual_query='PASS', report=str(args.report))))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repository', type=Path, default=Path('.'))
    parser.add_argument('--target', required=True)
    parser.add_argument('--report', type=Path, required=True)
    main(parser.parse_args())
