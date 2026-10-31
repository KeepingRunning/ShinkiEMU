use anyhow::{Context, Result, bail};
use object::{
    Object, ObjectKind,
    read::{
        elf::{Dyn, FileHeader, ProgramHeader},
        macho::MachHeader,
        pe::ImageNtHeaders,
    },
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct BinaryInfo {
    pub dependencies: BTreeSet<String>,
    pub rpaths: Vec<String>,
    pub signed: bool,
}
fn text(bytes: &[u8]) -> Result<String> {
    Ok(std::str::from_utf8(bytes)?.to_owned())
}
fn pe<'a, P: ImageNtHeaders>(
    file: &object::read::pe::PeFile<'a, P>,
    info: &mut BinaryInfo,
) -> Result<()> {
    if let Some(table) = file.import_table()? {
        let mut it = table.descriptors()?;
        while let Some(d) = it.next()? {
            info.dependencies
                .insert(text(table.name(d.name.get(object::LittleEndian))?)?);
        }
    }
    if let Some(table) = file
        .data_directories()
        .delay_load_import_table(file.data(), &file.section_table())?
    {
        let mut it = table.descriptors()?;
        while let Some(d) = it.next()? {
            info.dependencies
                .insert(text(table.name(d.dll_name_rva.get(object::LittleEndian))?)?);
        }
    }
    Ok(())
}
fn elf<'a, E: FileHeader>(
    file: &object::read::elf::ElfFile<'a, E>,
    info: &mut BinaryInfo,
) -> Result<()> {
    let endian = file.endian();
    for segment in file.elf_program_headers() {
        let Some(entries) = segment.dynamic(endian, file.data())? else {
            continue;
        };
        let entries = &entries[..entries
            .iter()
            .position(|entry| entry.tag(endian) == object::elf::DT_NULL)
            .context("ELF dynamic segment lacks DT_NULL")?];
        if !entries.iter().any(|entry| {
            matches!(
                entry.tag(endian),
                object::elf::DT_NEEDED | object::elf::DT_RUNPATH | object::elf::DT_RPATH
            )
        }) {
            continue;
        }
        let address: u64 = entries
            .iter()
            .find(|entry| entry.tag(endian) == object::elf::DT_STRTAB)
            .context("ELF dynamic string table address missing")?
            .d_val(endian)
            .into();
        let size: u64 = entries
            .iter()
            .find(|entry| entry.tag(endian) == object::elf::DT_STRSZ)
            .context("ELF dynamic string table size missing")?
            .d_val(endian)
            .into();
        let mut data = None;
        for load in file.elf_program_headers() {
            if load.p_type(endian) != object::elf::PT_LOAD {
                continue;
            }
            data = load
                .data_range(endian, file.data(), address, size)
                .map_err(|_| anyhow::anyhow!("invalid ELF load segment range"))?;
            if data.is_some() {
                break;
            }
        }
        let data = data.context("ELF dynamic strings are not in a loadable segment")?;
        let strings = object::read::StringTable::new(data, 0, size);
        for entry in entries {
            match entry.tag(endian) {
                object::elf::DT_NEEDED => {
                    info.dependencies
                        .insert(text(entry.string(endian, strings)?)?);
                }
                object::elf::DT_RUNPATH | object::elf::DT_RPATH => {
                    info.rpaths.extend(
                        text(entry.string(endian, strings)?)?
                            .split(':')
                            .map(str::to_owned),
                    );
                }
                _ => {}
            }
        }
    }
    Ok(())
}
fn macho<'a, M: MachHeader>(
    file: &object::read::macho::MachOFile<'a, M>,
    info: &mut BinaryInfo,
) -> Result<()> {
    let e = file.endian();
    let mut it = file.macho_header().load_commands(e, file.data(), 0)?;
    while let Some(c) = it.next()? {
        if c.cmd() != object::macho::LC_ID_DYLIB
            && let Some(d) = c.dylib()?
        {
            info.dependencies.insert(text(c.string(e, d.dylib.name)?)?);
        }
        if c.cmd() == object::macho::LC_RPATH {
            let r: &object::macho::RpathCommand<_> = c.data()?;
            info.rpaths.push(text(c.string(e, r.path)?)?);
        }
        if c.cmd() == object::macho::LC_CODE_SIGNATURE {
            info.signed = true;
        }
    }
    Ok(())
}
pub fn inspect(path: &Path, target: &str, require_query: bool) -> Result<BinaryInfo> {
    crate::package::plain(path)?;
    let bytes = fs::read(path)?;
    let file = object::File::parse(bytes.as_slice())
        .with_context(|| format!("parsing {}", path.display()))?;
    let arch = if target.starts_with("x86_64-") {
        object::Architecture::X86_64
    } else if target.starts_with("aarch64-") {
        object::Architecture::Aarch64
    } else {
        bail!("unsupported architecture {target}");
    };
    if file.architecture() != arch {
        bail!(
            "{}: architecture {:?} does not match {target}",
            path.display(),
            file.architecture()
        );
    }
    let format = if target.contains("windows") {
        object::BinaryFormat::Pe
    } else if target.contains("apple") {
        object::BinaryFormat::MachO
    } else {
        object::BinaryFormat::Elf
    };
    if file.format() != format
        || !matches!(file.kind(), ObjectKind::Dynamic | ObjectKind::Executable)
    {
        bail!("{}: not a target executable/shared library", path.display());
    }
    if require_query {
        let mut has_query = false;
        if file.kind() == ObjectKind::Dynamic {
            for export in file.exports()? {
                let export = export?;
                if let object::read::NameOrOrdinal::Name(name) = export.name() {
                    has_query |= name == b"shinki_emu_query"
                        || (format == object::BinaryFormat::MachO && name == b"_shinki_emu_query");
                }
            }
        }
        if !has_query {
            bail!("{} lacks shinki_emu_query", path.display());
        }
    }
    let mut info = BinaryInfo {
        dependencies: BTreeSet::new(),
        rpaths: Vec::new(),
        signed: false,
    };
    match &file {
        object::File::Pe32(f) => pe(f, &mut info)?,
        object::File::Pe64(f) => pe(f, &mut info)?,
        object::File::Elf32(f) => elf(f, &mut info)?,
        object::File::Elf64(f) => elf(f, &mut info)?,
        object::File::MachO32(f) => macho(f, &mut info)?,
        object::File::MachO64(f) => macho(f, &mut info)?,
        _ => bail!("unsupported binary format"),
    }
    for r in &info.rpaths {
        if !(r == "$ORIGIN"
            || r.starts_with("$ORIGIN/")
            || r == "@loader_path"
            || r.starts_with("@loader_path/")
            || r == "@executable_path"
            || r.starts_with("@executable_path/"))
            || r.split('/').any(|s| s == "..")
        {
            bail!(
                "non-relocatable runtime search path {r} in {}",
                path.display()
            );
        }
    }
    for dependency in &info.dependencies {
        if target.contains("apple") {
            if !system(dependency, target)
                && (!(dependency.starts_with("@rpath/")
                    || dependency.starts_with("@loader_path/")
                    || dependency.starts_with("@executable_path/"))
                    || dependency.split('/').any(|part| part == ".."))
            {
                bail!("non-relocatable Mach-O dependency {dependency}");
            }
        } else if dependency.contains(['/', '\\']) {
            bail!("non-relocatable binary dependency {dependency}");
        }
    }
    Ok(info)
}

pub(crate) fn system(name: &str, target: &str) -> bool {
    if target.contains("windows") {
        let n = name.to_ascii_lowercase();
        return n.starts_with("api-ms-win-")
            || n.starts_with("ext-ms-win-")
            || [
                "kernel32.dll",
                "kernelbase.dll",
                "user32.dll",
                "gdi32.dll",
                "gdiplus.dll",
                "gdi32full.dll",
                "advapi32.dll",
                "shell32.dll",
                "ole32.dll",
                "oleaut32.dll",
                "combase.dll",
                "uuid.dll",
                "ntdll.dll",
                "ws2_32.dll",
                "bcrypt.dll",
                "bcryptprimitives.dll",
                "crypt32.dll",
                "secur32.dll",
                "rpcrt4.dll",
                "shlwapi.dll",
                "winmm.dll",
                "version.dll",
                "imm32.dll",
                "setupapi.dll",
                "cfgmgr32.dll",
                "dwmapi.dll",
                "uxtheme.dll",
                "d3d9.dll",
                "d3d11.dll",
                "d3d12.dll",
                "dxgi.dll",
                "dwrite.dll",
                "d2d1.dll",
                "d3dcompiler_47.dll",
                "opengl32.dll",
                "glu32.dll",
                "winspool.drv",
                "msvcrt.dll",
                "ucrtbase.dll",
                "powrprof.dll",
                "hid.dll",
                "avrt.dll",
                "mmdevapi.dll",
                "avicap32.dll",
                "propsys.dll",
                "mf.dll",
                "mfplat.dll",
                "mfreadwrite.dll",
                "mfuuid.dll",
                "winhttp.dll",
                "wininet.dll",
                "normaliz.dll",
                "iphlpapi.dll",
                "dnsapi.dll",
                "userenv.dll",
                "psapi.dll",
                "dbghelp.dll",
                "dbgcore.dll",
                "msimg32.dll",
                "comdlg32.dll",
                "comctl32.dll",
                "uiautomationcore.dll",
                "wtsapi32.dll",
                "netapi32.dll",
                "wintrust.dll",
                "imagehlp.dll",
                "dhcpcsvc.dll",
                "ncrypt.dll",
                "usp10.dll",
                "winusb.dll",
                "xinput1_4.dll",
                "dsound.dll",
                "msacm32.dll",
                "authz.dll",
                "wldap32.dll",
                "sensapi.dll",
            ]
            .contains(&n.as_str());
    }
    if target.contains("android") {
        let n = name.to_ascii_lowercase();
        return [
            "libc.so",
            "libm.so",
            "libdl.so",
            "liblog.so",
            "libandroid.so",
            "libegl.so",
            "libglesv1_cm.so",
            "libglesv2.so",
            "libglesv3.so",
            "libvulkan.so",
            "libaaudio.so",
            "libcamera2ndk.so",
            "libopensles.so",
            "libmediandk.so",
            "libnativewindow.so",
            "libz.so",
            "libjnigraphics.so",
            "libsync.so",
        ]
        .contains(&n.as_str());
    }
    if target.contains("apple") {
        return name.starts_with("/usr/lib/") || name.starts_with("/System/Library/Frameworks/");
    }
    [
        "libc.so.6",
        "libm.so.6",
        "libdl.so.2",
        "libpthread.so.0",
        "librt.so.1",
        "ld-linux-x86-64.so.2",
        "ld-linux-aarch64.so.1",
        "libgcc_s.so.1",
        // Linux desktop ABI supplied by the supported distribution, not bundled
        // from the developer machine. Keep versioned SONAMEs explicit.
        "libstdc++.so.6",
        "libssl.so.3",
        "libcrypto.so.3",
        "libfontconfig.so.1",
        "libudev.so.1",
        "libasound.so.2",
        // RFVP's Linux video-sys backend uses the distribution media stack.
        "libgstreamer-1.0.so.0",
        "libgstapp-1.0.so.0",
        "libgstbase-1.0.so.0",
        "libgstvideo-1.0.so.0",
        "libglib-2.0.so.0",
        "libgobject-2.0.so.0",
        "libgio-2.0.so.0",
        "libgmodule-2.0.so.0",
        // Manager store sign-in uses the distribution's WebKitGTK 4.1 / GTK 3.
        "libwebkit2gtk-4.1.so.0",
        "libjavascriptcoregtk-4.1.so.0",
        "libsoup-3.0.so.0",
        "libgtk-3.so.0",
        "libgdk-3.so.0",
        "libatk-1.0.so.0",
        "libcairo.so.2",
        "libcairo-gobject.so.2",
        "libpango-1.0.so.0",
        "libpangocairo-1.0.so.0",
        "libgdk_pixbuf-2.0.so.0",
        "libharfbuzz.so.0",
    ]
    .contains(&name)
}
/// Resolve exclusively against explicitly distributable trees, never PATH/System32.
pub fn closure(library: &Path, search: &[PathBuf], target: &str) -> Result<Vec<PathBuf>> {
    let mut found = BTreeMap::<String, PathBuf>::new();
    let mut visited = BTreeSet::new();
    let mut pending = vec![library.to_owned()];
    let root_name = library
        .file_name()
        .context("library filename")?
        .to_string_lossy()
        .to_ascii_lowercase();
    while let Some(path) = pending.pop() {
        let name = path
            .file_name()
            .context("binary filename")?
            .to_string_lossy()
            .to_ascii_lowercase();
        if !visited.insert(name) {
            continue;
        }
        let info = inspect(&path, target, false)?;
        for dep in info.dependencies {
            if system(&dep, target) {
                continue;
            }
            let leaf = dep.rsplit('/').next().context("dependency filename")?;
            crate::package::portable(leaf)?;
            let key = leaf.to_ascii_lowercase();
            if key == root_name || found.contains_key(&key) {
                continue;
            }
            let mut matches = Vec::new();
            for dir in search {
                if !dir.is_dir() {
                    continue;
                }
                crate::package::plain(dir)?;
                for entry in fs::read_dir(dir)? {
                    let entry = entry?;
                    if entry
                        .file_name()
                        .to_string_lossy()
                        .eq_ignore_ascii_case(leaf)
                    {
                        matches.push(entry.path());
                    }
                }
            }
            let resolved=matches.first().with_context(|| format!("{}: missing redistributable dependency {dep}; run cargo xtask deps or provide its build tree",path.display()))?;
            crate::package::plain(resolved)?;
            for other in matches.iter().skip(1) {
                if crate::package::sha256(other)? != crate::package::sha256(resolved)? {
                    bail!("conflicting dependency {dep}");
                }
            }
            found.insert(key, resolved.clone());
            pending.push(resolved.clone());
        }
    }
    Ok(found.into_values().collect())
}
