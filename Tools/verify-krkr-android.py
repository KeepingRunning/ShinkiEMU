#!/usr/bin/env python3
"""Verify the real universal APK's Krkr Java/JNI/static/resource contract.

No emulator or device execution is performed. Build receipts must belong to
this APK build. The staged receipts must describe target-specific native static
archives rather than downloadable shared cores.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import struct
import sys
import tempfile
import zipfile


def command(*args):
    return subprocess.run([str(arg) for arg in args], check=True, text=True,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE).stdout


def dex_methods(text):
    methods = {}
    owner = name = signature = None
    for line in text.splitlines():
        found = re.search(r"Class descriptor\s*:\s*'([^']+)'", line)
        if found:
            owner = found.group(1)
            name = signature = None
            continue
        found = re.search(r"\bname\s*:\s*'([^']+)'", line)
        if found:
            name = found.group(1)
            signature = None
        found = re.search(r"\btype\s*:\s*'([^']+)'", line)
        if found:
            signature = found.group(1)
        if owner and name and signature and signature.startswith('(') and re.search(r"\baccess\s*:", line):
            methods[(owner, name, signature)] = 'NATIVE' in line
    return methods


def symbols(nm, binary):
    result = set()
    for line in command(nm, '--defined-only', '--extern-only', '--format=posix', binary).splitlines():
        fields = line.split()
        if len(fields) >= 2 and len(fields[1]) == 1:
            result.add(fields[0])
    return result


def digest(path):
    with path.open('rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()


def registration_tables(ar, library, contracts):
    """Read actual ELF64 RELA pointers in SDL's compiled JNINativeMethod arrays."""
    members = command(ar, 't', library).splitlines()
    if len(members) != 1:
        raise ValueError('Krkr closure must contain its one resolved native object')
    data = subprocess.run([str(ar), 'p', str(library), members[0]], check=True,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE).stdout
    if data[:6] != b'\x7fELF\x02\x01':
        raise ValueError('Krkr closure is not a little-endian ELF64 object')
    header = struct.unpack_from('<16sHHIQQQIHHHHHH', data)
    sections = [struct.unpack_from('<IIQQQQIIQQ', data, header[6] + i * header[11])
                for i in range(header[12])]

    def string(section, offset):
        start = sections[section][4] + offset
        return data[start:data.index(b'\0', start)].decode('utf-8')

    tables = {}
    relocations = {}
    for index, section in enumerate(sections):
        if section[1] != 2:  # SHT_SYMTAB
            continue
        symbols = [struct.unpack_from('<IBBHQQ', data, offset)
                   for offset in range(section[4], section[4] + section[5], section[9])]
        for symbol in symbols:
            name = string(section[6], symbol[0])
            if name in {'SDLActivity_tab', 'SDLInputConnection_tab',
                        'SDLAudioManager_tab', 'SDLControllerManager_tab'}:
                tables[name] = symbol
        for relocation in sections:
            if relocation[1] != 4 or relocation[6] != index:  # SHT_RELA
                continue
            for offset in range(relocation[4], relocation[4] + relocation[5], relocation[9]):
                address, info, addend = struct.unpack_from('<QQq', data, offset)
                target = symbols[info >> 32]
                relocations[(relocation[7], address)] = (target, addend, section[6])
    if len(tables) != 4:
        raise ValueError(f'Actual SDL RegisterNatives arrays missing: {sorted(tables)}')
    proof = {}
    for name, symbol in sorted(tables.items()):
        if symbol[5] % 24:
            raise ValueError(f'Invalid JNINativeMethod array size: {name}')
        methods = []
        for offset in range(symbol[4], symbol[4] + symbol[5], 24):
            fields = [relocations[(symbol[3], offset + field)] for field in (0, 8, 16)]
            method = string(fields[0][0][3], fields[0][0][4] + fields[0][1])
            signature = string(fields[1][0][3], fields[1][0][4] + fields[1][1])
            implementation = string(fields[2][2], fields[2][0][0])
            if not fields[2][0][3] or not implementation.startswith('shinki_krkr_'):
                raise ValueError(f'SDL registration does not reference its private implementation: {method}')
            expected = {'class': f'Lorg/krkrsd/app/{name.removesuffix("_tab")};',
                        'name': method, 'signature': signature, 'native': True}
            if expected not in contracts:
                raise ValueError(f'Compiled SDL registration differs from staged DEX contract: {expected}')
            methods.append({'name': method, 'signature': signature, 'implementation': implementation})
        proof[name] = methods
    return proof


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    selection = parser.add_mutually_exclusive_group(required=True)
    selection.add_argument('--apk', type=Path)
    selection.add_argument('--ci-report', type=Path)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    apk = args.apk
    if args.ci_report:
        report = json.loads(args.ci_report.read_text(encoding='utf-8'))
        if report['status'] != 'built' or not report['artifact']:
            raise ValueError('Manager CI has not packaged its actual APK')
        apk = Path(report['artifact'])
    sdk = Path(os.environ['ANDROID_HOME'])
    ndk = Path(os.environ.get('ANDROID_NDK_HOME', sdk / 'ndk/30.0.16248370'))
    host = 'windows-x86_64' if sys.platform == 'win32' else ('darwin-x86_64' if sys.platform == 'darwin' else 'linux-x86_64')
    suffix = '.exe' if sys.platform == 'win32' else ''
    nm = ndk / 'toolchains/llvm/prebuilt' / host / 'bin' / ('llvm-nm' + suffix)
    dexdump = sdk / 'build-tools/37.0.0' / ('dexdump' + suffix)
    aapt = sdk / 'build-tools/37.0.0' / ('aapt2' + suffix)
    readelf = nm.with_name('llvm-readelf' + suffix)
    ar = nm.with_name('llvm-ar' + suffix)
    objdump = nm.with_name('llvm-objdump' + suffix)
    prefix = 'assets/cores/kirikiroid2-web/Resources/'
    proof = {'apk': str(apk.resolve()), 'sha256': digest(apk), 'device_launch': 'not run (waived)', 'abis': {}}
    with zipfile.ZipFile(apk) as archive, tempfile.TemporaryDirectory(prefix='krkr-apk-') as temporary:
        names = set(archive.namelist())
        embedded = json.loads(archive.read('assets/embedded-cores.json'))
        expected = {'rfvp', 'siglus_rs', 'shinki.emu.musica', 'art3m1s-core', 'kirikiroid2-web'}
        ids = {core['id'] for core in embedded}
        if not expected.issubset(ids):
            raise ValueError(f'APK lacks required static consumers: {sorted(expected - ids)}')
        proof['embedded_cores'] = sorted(ids)
        core = next(core for core in embedded if core['id'] == 'kirikiroid2-web')
        proof['fork'] = core['source']['rev']
        contracts = json.loads(archive.read(prefix + 'android-host/jni-contract.json'))
        methods = {}
        for name in sorted(names):
            if re.fullmatch(r'classes\d*\.dex', name):
                dex = Path(temporary) / name
                dex.write_bytes(archive.read(name))
                methods.update(dex_methods(command(dexdump, dex)))
        for contract in contracts:
            key = (contract['class'], contract['name'], contract['signature'])
            if key not in methods or methods[key] != contract['native']:
                raise ValueError(f'APK DEX/JNI signature mismatch: {contract}')
        proof['dex_jni_contracts'] = len(contracts)
        for suffix_required in ('.csb', '.ttf'):
            if not any(name.startswith(prefix) and name.endswith(suffix_required) for name in names):
                raise ValueError(f'APK lacks original Cocos resource {suffix_required}')
        original_resources = args.root / 'ThirdParty/kirikiri2/ui/cocos-studio'
        resource_hashes = {}
        for resource in sorted(original_resources.rglob('*')):
            if resource.is_file():
                relative = resource.relative_to(original_resources).as_posix()
                content = archive.read(prefix + relative)
                if hashlib.sha256(content).hexdigest() != digest(resource):
                    raise ValueError(f'APK changed an original Cocos resource: {relative}')
                resource_hashes[relative] = digest(resource)
        if not resource_hashes:
            raise ValueError('Exact Krkr source resources are unavailable')
        proof['original_resources_sha256'] = resource_hashes
        for notice in ('LICENSE', 'FMOD.txt', 'libbpg-MIT.txt', 'UnRAR.txt', 'minizip-zlib.txt',
                       'LICENSE_cocos2d-x.txt', 'LICENSE_cocosdenshion.txt',
                       'LICENSE_com.android.vending.expansion.zipfile.txt', 'SDL-LICENSE.txt'):
            if not any(name.startswith('assets/licenses/') and name.endswith('/' + notice) and archive.getinfo(name).file_size for name in names):
                raise ValueError(f'APK lacks authentic native/Java notice: {notice}')
        manifest = command(aapt, 'dump', 'xmltree', apk, '--file', 'AndroidManifest.xml')
        activities = []
        activity_indent = None
        for line in manifest.splitlines():
            indent = len(line) - len(line.lstrip())
            if re.search(r'E: activity(?:\s|$)', line):
                activity_indent = indent
            elif activity_indent is not None and indent <= activity_indent and line.strip():
                activity_indent = None
            if activity_indent is not None and 'android:name' in line:
                found = re.search(r'=\s*"([^"]+)"', line)
                if found:
                    activities.append(found.group(1))
        if activities != ['org.shinkirou.shinkiemu.MainActivity']:
            raise ValueError(f'APK entry Activities changed: {activities}')
        proof['entry_activities'] = activities
        for target, abi in [('aarch64-linux-android', 'arm64-v8a'), ('x86_64-linux-android', 'x86_64')]:
            if f'lib/{abi}/libshinki_emu_manager.so' not in names:
                raise ValueError(f'APK lacks actual Manager ABI: {abi}')
            if any(name.startswith(f'lib/{abi}/') and 'kirikiroid2' in name for name in names):
                raise ValueError('Krkr was packaged as a dynamic core rather than embedded')
            built = json.loads((args.root / '.work/build' / target / 'manager.json').read_text(encoding='utf-8'))
            if built['source_dirty']:
                raise ValueError('Manager build receipt is dirty')
            commits = {entry['source_commit'] for entry in proof['abis'].values()}
            if commits and built['source_commit'] not in commits:
                raise ValueError('APK ABI build receipts name different product commits')
            packaged_library = Path(temporary) / f'manager-{abi}.so'
            packaged_library.write_bytes(archive.read(f'lib/{abi}/libshinki_emu_manager.so'))
            build_id = lambda path: re.search(r'Build ID:\s*(\w+)', command(readelf, '--notes', path))
            original_id, packaged_id = build_id(Path(built['library'])), build_id(packaged_library)
            if not original_id or not packaged_id or original_id.group(1) != packaged_id.group(1):
                raise ValueError(f'APK Manager ELF differs from its actual build receipt: {abi}')
            native = next(core for core in built['embedded_core_builds'] if core['plugin_id'] == 'kirikiroid2-web')
            if native['artifact_kind'] != 'native-static' or native['target'] != target:
                raise ValueError('Krkr lacks an actual target native-static receipt')
            library = Path(native['native_library'])
            defined = symbols(nm, library)
            registrations = registration_tables(ar, library, contracts)
            public = {'shinki_emu_kirikiroid2_web_query', 'shinki_emu_kirikiroid2_web_set_resource_root',
                      'shinki_emu_kirikiroid2_web_android_initialize'}
            if not public.issubset(defined) or any(name not in public and not name.startswith('shinki_krkr_') for name in defined):
                raise ValueError(f'Krkr native closure export/namespace failure: {target}')
            for marker in ('shinki_krkr_SDL_Init', 'shinki_krkr_shinki_krkr_sdl_on_load'):
                if marker not in defined:
                    raise ValueError(f'Krkr original SDL implementation missing: {marker}')
            if not any(name.startswith('shinki_krkr_avcodec_') for name in defined):
                raise ValueError('Krkr private FFmpeg implementation missing')
            for implementation in ('_ZN7cocos2d8Director', '_ZN4oboe', 'alSourcePlay'):
                if not any(implementation in name for name in defined):
                    raise ValueError(f'Krkr real renderer/audio implementation missing: {implementation}')
            if any(name.endswith('_JNI_OnLoad') for name in defined):
                raise ValueError('Krkr retained a second JNI_OnLoad instead of its explicit private loader')
            spine = sorted(name for name in defined if re.search(r'spAnimation|spSkeleton|spine|SkeletonRenderer', name, re.IGNORECASE))
            if spine:
                raise ValueError(f'Actual Krkr static closure includes Spine runtime; applicable grant or unused-module exclusion required: {spine[:8]}')
            exit_symbols = [name for name in defined if 'TVPExitApplication' in name]
            env_symbols = [name for name in defined if name.endswith('9JniHelper6getEnvEv')]
            if len(exit_symbols) != 1 or len(env_symbols) != 1:
                raise ValueError('Krkr exit/environment implementations are missing or ambiguous')
            exit_code = command(objdump, '--reloc', '--disassemble-symbols=' + exit_symbols[0], library)
            env_code = command(objdump, '--reloc', '--disassemble-symbols=' + env_symbols[0], library)
            if re.search(r'\b(exit|_exit|quick_exit|abort)\b', exit_code):
                raise ValueError('Embedded Krkr exit implementation still references process termination')
            if 'pthread_getspecific' in env_code:
                raise ValueError('Cocos getEnv still reuses a cached environment across host attachment scopes')
            linked = symbols(nm, Path(built['library']))
            if not public.issubset(linked):
                raise ValueError(f'Manager did not link the actual Krkr query/host: {target}')
            proof['abis'][abi] = {'source_commit': built['source_commit'], 'archive': str(library),
                                  'archive_sha256': digest(library), 'private_symbols': len(defined) - len(public),
                                  'compiled_register_natives': registrations, 'spine_runtime_symbols': spine,
                                  'embedded_exit_no_process_fallback': True, 'cocos_getenv_no_cached_environment': True}
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(proof, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(proof, indent=2))


if __name__ == '__main__':
    main()
