//! Scratch space under the system temporary root.
//!
//! Resolve aliases such as macOS `/var` before creating private entries so that
//! package path checks can inspect them. Untrusted input paths must still pass
//! those checks unchanged; staging beside an output must stay on its filesystem.

use std::io;
use tempfile::{NamedTempFile, TempDir};

pub fn directory() -> io::Result<TempDir> {
    tempfile::tempdir_in(dunce::canonicalize(std::env::temp_dir())?)
}

pub fn file() -> io::Result<NamedTempFile> {
    NamedTempFile::new_in(dunce::canonicalize(std::env::temp_dir())?)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::package::{extract_zip, sha256, tree, zip_entries};
    use std::{fs, io::Write, os::unix::fs::symlink, path::Path, process::Command};

    #[test]
    fn linked_system_temp_root_supports_packaging() {
        const CHILD_ROOT: &str = "SHINKI_EMU_XTASK_TEST_TEMP_ROOT";
        if let Some(root) = std::env::var_os(CHILD_ROOT) {
            let root = Path::new(&root);
            let temp = directory().unwrap();
            let mut source = file().unwrap();
            assert!(temp.path().starts_with(root));
            assert!(source.path().starts_with(root));
            let bytes = b"synthetic package resource";
            source.write_all(bytes).unwrap();
            source.flush().unwrap();
            let hash = sha256(source.path()).unwrap();
            let archive = temp.path().join("package.zip");
            zip_entries(&[(source.path().to_owned(), "resource".into())], &archive).unwrap();
            let extracted = temp.path().join("extracted");
            extract_zip(&archive, &extracted).unwrap();
            assert_eq!(fs::read(extracted.join("resource")).unwrap(), bytes);
            assert_eq!(sha256(&extracted.join("resource")).unwrap(), hash);
            assert_eq!(tree(&extracted).unwrap().len(), 1);

            // Resolving the system root must not allow links inside the inputs.
            let link = extracted.join("linked-resource");
            symlink(source.path(), &link).unwrap();
            assert!(sha256(&link).is_err());
            assert!(tree(&extracted).is_err());
            let rejected = temp.path().join("rejected.zip");
            assert!(zip_entries(&[(link, "resource".into())], &rejected).is_err());
            assert!(!rejected.exists());
            return;
        }

        let sandbox = directory().unwrap();
        let root = sandbox.path().join("real");
        fs::create_dir(&root).unwrap();
        let alias = sandbox.path().join("alias");
        symlink(&root, &alias).unwrap();
        // Isolate TMPDIR from other tests without mutating the process environment.
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "temporary::tests::linked_system_temp_root_supports_packaging",
                "--nocapture",
            ])
            .env("TMPDIR", &alias)
            .env(CHILD_ROOT, &root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "child test failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        // Both scratch files and directories retain their automatic cleanup.
        assert!(fs::read_dir(root).unwrap().next().is_none());
    }
}
