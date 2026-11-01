//! Capability-relative download host, migrated from Manager's fd9c78a4 downloader.
//! Only verified, hash-named files are published under `game/.shinki/patch`.
use crate::{PatchError, PatchErrorKind, PatchProgress, PatchVerification, error::check_cancelled};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, DirBuilder, Metadata, OpenOptions};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    future::Future,
    io::{self, Read, Seek, SeekFrom, Write},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::Duration,
};

pub const MAX_DOWNLOAD_BYTES: u64 = 1024 * 1024 * 1024;

pub struct DownloadedFile {
    pub file: File,
    pub sha256: String,
}

/// Download an HTTPS artifact or reuse a size/hash-verified cached file.
/// The URL and declared bounds are checked even when the cache already exists.
///
/// Requires a nonzero size of at most 1GiB and a 64-digit SHA-256. Redirects
/// retain HTTPS; neither URLs nor response headers supply local paths. Cache
/// files use `<lowercase-sha256>.bin` inside the capability-opened private patch
/// directory. Every cache hit is rehashed. Partial files are removed on errors
/// and cancellation; only verified files are atomically renamed into the cache.
/// Call from the synchronous patch worker, outside an async runtime.
pub fn download(
    game: &Dir,
    url: &str,
    sha256: &str,
    size: u64,
    cancelled: &AtomicBool,
    progress: impl Fn(PatchProgress) + Send + Sync,
) -> Result<DownloadedFile, PatchError> {
    let url = validate(url, sha256, size)?;
    let sha256 = sha256.to_ascii_lowercase();
    check_cancelled(cancelled)?;
    let patch = private_patch_dir(game)?;
    report(&progress, &sha256, PatchVerification::Pending, 0, size);
    match cached(&patch, &sha256, size, cancelled) {
        Ok(Some(file)) => {
            report(&progress, &sha256, PatchVerification::Ok, size, size);
            return Ok(DownloadedFile { file, sha256 });
        }
        Ok(None) => {}
        Err(error) => {
            if error.kind() == PatchErrorKind::Integrity {
                report(&progress, &sha256, PatchVerification::Failed, 0, size);
            }
            return Err(error);
        }
    }
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()
        .map_err(|cause| PatchError::with_source(PatchErrorKind::Io, "cannot start download runtime", cause))?;
    runtime.block_on(async {
        let client = client_builder().build().map_err(network)?;
        fetch(&patch, &client, url, &sha256, size, cancelled, &progress).await
    })
}

fn failure(kind: PatchErrorKind, message: &'static str) -> PatchError {
    PatchError::new(kind, message)
}

fn https(url: &reqwest::Url) -> bool {
    url.scheme() == "https" && url.username().is_empty()
        && url.password().is_none() && url.host_str().is_some()
}

/// Apply the production URL/digest/size policy without touching disk or network.
pub(crate) fn validate_request(url: &str, sha256: &str, size: u64) -> Result<(), PatchError> {
    validate(url, sha256, size).map(|_| ())
}

fn validate(url: &str, sha256: &str, size: u64) -> Result<reqwest::Url, PatchError> {
    if size == 0 || size > MAX_DOWNLOAD_BYTES {
        return Err(failure(PatchErrorKind::Bounds, "download size must be between 1 byte and 1GiB"));
    }
    if sha256.len() != 64 || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(failure(PatchErrorKind::InvalidRequest, "download requires a SHA-256 digest"));
    }
    let url = reqwest::Url::parse(url)
        .map_err(|_| failure(PatchErrorKind::InvalidRequest, "invalid download URL"))?;
    if !https(&url) {
        return Err(failure(PatchErrorKind::InvalidRequest, "download URL must use HTTPS without credentials"));
    }
    Ok(url)
}

fn client_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 10 || !https(attempt.url()) {
                attempt.error("unsafe download redirect")
            } else {
                attempt.follow()
            }
        }))
}

fn network(cause: reqwest::Error) -> PatchError {
    PatchError::with_source(PatchErrorKind::Io, "download network request failed", cause.without_url())
}

fn is_link(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use cap_std::fs::MetadataExt;
        use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
    }
    #[cfg(not(windows))]
    { metadata.is_symlink() }
}

fn private_patch_dir(game: &Dir) -> Result<Dir, PatchError> {
    let mut directory = game.try_clone()?;
    for component in [crate::GAME_STATE_DIR, crate::PATCH_DIR] {
        #[cfg(unix)]
        let builder = {
            use cap_std::fs::DirBuilderExt;
            let mut builder = DirBuilder::new();
            builder.mode(0o700);
            builder
        };
        #[cfg(not(unix))]
        let builder = DirBuilder::new();
        // No ambient path reconstruction: each component is opened from its parent.
        match directory.create_dir_with(component, &builder) {
            Ok(()) => {}
            Err(cause) if cause.kind() == io::ErrorKind::AlreadyExists => {}
            Err(cause) => return Err(PatchError::with_source(PatchErrorKind::Path, "cannot create private patch directory", cause)),
        }
        let next = directory.open_dir_nofollow(component)
            .map_err(|cause| PatchError::with_source(PatchErrorKind::Path, "unsafe private patch directory", cause))?;
        let metadata = next.dir_metadata()?;
        if is_link(&metadata) || !metadata.is_dir() {
            return Err(failure(PatchErrorKind::Path, "private patch directory is a link or special file"));
        }
        directory = next;
    }
    Ok(directory)
}

fn read_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use cap_std::fs::OpenOptionsExt;
        use windows::Win32::Storage::FileSystem::{FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT};
        options.custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0);
    }
    options
}

fn cached(patch: &Dir, sha256: &str, size: u64, cancelled: &AtomicBool) -> Result<Option<File>, PatchError> {
    let name = format!("{sha256}.bin");
    let mut file = match patch.open_with(&name, &read_options()) {
        Ok(file) => file.into_std(),
        Err(cause) if cause.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(cause) => return Err(PatchError::with_source(PatchErrorKind::Path, "cannot open ordinary cached artifact", cause)),
    };
    let metadata = Metadata::from_file(&file)?;
    if is_link(&metadata) || !metadata.is_file() || cap_fs_ext::MetadataExt::nlink(&metadata) != 1 {
        return Err(failure(PatchErrorKind::Path, "cached artifact is not an ordinary private file"));
    }
    check_file(&mut file, sha256, size, cancelled)?;
    file.seek(SeekFrom::Start(0))?;
    Ok(Some(file))
}

fn check_file(mut file: impl Read, sha256: &str, size: u64, cancelled: &AtomicBool) -> Result<(), PatchError> {
    let mut hasher = Sha256::new();
    let mut received = 0u64;
    let mut bytes = [0u8; 64 * 1024];
    loop {
        check_cancelled(cancelled)?;
        let count = file.read(&mut bytes)?;
        if count == 0 { break; }
        received += count as u64;
        if received > size {
            return Err(failure(PatchErrorKind::Integrity, "download exceeds declared size"));
        }
        hasher.update(&bytes[..count]);
    }
    verify(received, hasher, sha256, size)
}

fn verify(received: u64, hasher: Sha256, sha256: &str, size: u64) -> Result<(), PatchError> {
    if received != size {
        return Err(failure(PatchErrorKind::Integrity, "download size mismatch"));
    }
    if hex::encode(hasher.finalize()) != sha256 {
        return Err(failure(PatchErrorKind::Integrity, "download SHA-256 mismatch"));
    }
    Ok(())
}

// The original chunk/hash writer now keeps every operation capability-relative.
// Drop closes the handle before unlinking, including on Windows.
struct ArtifactWriter<'a> {
    patch: &'a Dir,
    name: String,
    file: Option<File>,
    received: u64,
    hasher: Sha256,
}

impl<'a> ArtifactWriter<'a> {
    fn new(patch: &'a Dir, sha256: &str) -> Result<Self, PatchError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..128 {
            let name = format!(".{sha256}.{}-{}.part", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
            let mut options = read_options();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use cap_std::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match patch.open_with(&name, &options) {
                Ok(file) => return Ok(Self { patch, name, file: Some(file.into_std()), received: 0, hasher: Sha256::new() }),
                Err(cause) if cause.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(cause) => return Err(cause.into()),
            }
        }
        Err(failure(PatchErrorKind::Io, "cannot allocate temporary artifact"))
    }

    fn chunk(&mut self, bytes: &[u8], size: u64) -> Result<(), PatchError> {
        let received = self.received.saturating_add(bytes.len() as u64);
        if received > size {
            return Err(failure(PatchErrorKind::Integrity, "download exceeds declared size"));
        }
        self.file.as_mut().expect("active writer").write_all(bytes)?;
        self.received = received;
        self.hasher.update(bytes);
        Ok(())
    }

    fn finish(mut self, sha256: &str, size: u64, cancelled: &AtomicBool) -> Result<DownloadedFile, PatchError> {
        verify(self.received, std::mem::take(&mut self.hasher), sha256, size)?;
        let file = self.file.as_mut().expect("active writer");
        file.sync_all()?;
        file.seek(SeekFrom::Start(0))?;
        check_cancelled(cancelled)?;
        // rename never follows the destination entry, so a racing link cannot
        // redirect publication. The returned handle is the verified file itself.
        self.patch.rename(&self.name, self.patch, format!("{sha256}.bin"))?;
        self.name.clear();
        Ok(DownloadedFile { file: self.file.take().expect("published writer"), sha256: sha256.to_owned() })
    }
}

impl Drop for ArtifactWriter<'_> {
    fn drop(&mut self) {
        drop(self.file.take());
        if !self.name.is_empty() { let _ = self.patch.remove_file(&self.name); }
    }
}

fn report(progress: &impl Fn(PatchProgress), sha256: &str, verification: PatchVerification, completed: u64, total: u64) {
    progress(PatchProgress::download(format!("{sha256}.bin"), sha256.to_owned(), verification, completed, Some(total)));
}

async fn cancellable<T>(cancelled: &AtomicBool, operation: impl Future<Output = Result<T, reqwest::Error>>) -> Result<T, PatchError> {
    check_cancelled(cancelled)?;
    tokio::select! {
        result = operation => { check_cancelled(cancelled)?; result.map_err(network) }
        error = async {
            loop {
                tokio::time::sleep(Duration::from_millis(25)).await;
                if let Err(error) = check_cancelled(cancelled) { break error; }
            }
        } => Err(error),
    }
}

async fn fetch(
    patch: &Dir, client: &reqwest::Client, url: reqwest::Url, sha256: &str,
    size: u64, cancelled: &AtomicBool, progress: &impl Fn(PatchProgress),
) -> Result<DownloadedFile, PatchError> {
    let result = async {
        let mut response = cancellable(cancelled, client.get(url).send()).await?;
        if !response.status().is_success() {
            return Err(failure(PatchErrorKind::Io, "download returned an unsuccessful HTTP status"));
        }
        if response.content_length().is_some_and(|length| length != size) {
            return Err(failure(PatchErrorKind::Integrity, "download Content-Length mismatch"));
        }
        let mut writer = ArtifactWriter::new(patch, sha256)?;
        report(progress, sha256, PatchVerification::Pending, 0, size);
        while let Some(bytes) = cancellable(cancelled, response.chunk()).await? {
            writer.chunk(&bytes, size)?;
            report(progress, sha256, PatchVerification::Pending, writer.received, size);
        }
        check_cancelled(cancelled)?;
        writer.finish(sha256, size, cancelled)
    }.await;
    match &result {
        Ok(_) => report(progress, sha256, PatchVerification::Ok, size, size),
        Err(error) if error.kind() == PatchErrorKind::Integrity => {
            report(progress, sha256, PatchVerification::Failed, 0, size);
        }
        Err(_) => {}
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const BODY: &[u8] = b"synthetic patch artifact";

    fn digest(bytes: &[u8]) -> String { hex::encode(Sha256::digest(bytes)) }

    fn game() -> (tempfile::TempDir, Dir) {
        let directory = tempfile::tempdir().unwrap();
        let game = Dir::open_ambient_dir(directory.path(), cap_std::ambient_authority()).unwrap();
        (directory, game)
    }

    fn empty(patch: &Dir) { assert_eq!(patch.entries().unwrap().count(), 0); }

    fn error<T>(result: Result<T, PatchError>) -> PatchErrorKind {
        match result { Ok(_) => panic!("expected rejection"), Err(error) => error.kind() }
    }

    struct Server {
        url: String,
        client: reqwest::Client,
        requests: Arc<AtomicU64>,
        task: tokio::task::JoinHandle<()>,
    }

    impl Drop for Server { fn drop(&mut self) { self.task.abort(); } }

    async fn server() -> Server {
        use tokio_rustls::rustls;
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let certified = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let certificate = reqwest::Certificate::from_pem(certified.cert.pem().as_bytes()).unwrap();
        let key = rustls::pki_types::PrivatePkcs8KeyDer::from(certified.signing_key.serialize_der());
        let config = rustls::ServerConfig::builder().with_no_client_auth()
            .with_single_cert(vec![certified.cert.der().clone()], key.into()).unwrap();
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let url = format!("https://localhost:{}", listener.local_addr().unwrap().port());
        let requests = Arc::new(AtomicU64::new(0));
        let count = requests.clone();
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let acceptor = acceptor.clone();
                let count = count.clone();
                tokio::spawn(async move {
                    let Ok(mut stream) = acceptor.accept(stream).await else { return; };
                    let mut head = Vec::new();
                    while !head.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                        let mut bytes = [0; 1024];
                        let count = stream.read(&mut bytes).await.unwrap_or(0);
                        if count == 0 { return; }
                        head.extend_from_slice(&bytes[..count]);
                        assert!(head.len() <= 16 * 1024);
                    }
                    count.fetch_add(1, Ordering::Relaxed);
                    let path = String::from_utf8(head).unwrap().split_whitespace().nth(1).unwrap().to_owned();
                    let response = match path.as_str() {
                        "/redirect" => b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/forbidden\r\nContent-Length: 0\r\n\r\n".to_vec(),
                        "/oversize" => {
                            let mut bytes = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
                            bytes.extend_from_slice(format!("{:x}\r\n", BODY.len()).as_bytes());
                            bytes.extend_from_slice(BODY);
                            // Never finish the response: the size bound must stop
                            // reading immediately, rather than waiting for EOF.
                            bytes.extend_from_slice(b"\r\n");
                            bytes
                        }
                        "/stall" => format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", BODY.len()).into_bytes(),
                        _ => {
                            let mut bytes = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", BODY.len()).into_bytes();
                            bytes.extend_from_slice(BODY);
                            bytes
                        }
                    };
                    let _ = stream.write_all(&response).await;
                    if path == "/oversize" || path == "/stall" {
                        // Client drops the socket on rejection/cancellation.
                        let mut byte = [0];
                        let _ = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut byte)).await;
                    } else { let _ = stream.shutdown().await; }
                });
            }
        });
        Server { url, client: client_builder().no_proxy().tls_certs_only([certificate]).build().unwrap(), requests, task }
    }

    #[test]
    fn invalid_requests_never_create_cache() {
        let (directory, game) = game();
        let cancelled = AtomicBool::new(false);
        for url in ["http://127.0.0.1:1/file", "file:///tmp/file", "https://user:pass@localhost/file"] {
            assert_eq!(error(download(&game, url, &digest(BODY), BODY.len() as u64, &cancelled, |_| {})), PatchErrorKind::InvalidRequest);
        }
        for size in [0, MAX_DOWNLOAD_BYTES + 1] {
            assert_eq!(error(download(&game, "https://localhost:1", &digest(BODY), size, &cancelled, |_| {})), PatchErrorKind::Bounds);
        }
        for hash in ["", "../outside", &"g".repeat(64)] {
            assert_eq!(error(download(&game, "https://localhost:1", hash, 1, &cancelled, |_| {})), PatchErrorKind::InvalidRequest);
        }
        assert!(!directory.path().join(crate::GAME_STATE_DIR).exists());
    }

    #[test]
    fn pre_cancelled_download_creates_nothing() {
        let (directory, game) = game();
        assert_eq!(error(download(&game, "https://localhost:1", &digest(BODY), BODY.len() as u64, &AtomicBool::new(true), |_| {})), PatchErrorKind::Cancelled);
        assert!(!directory.path().join(crate::GAME_STATE_DIR).exists());
    }

    #[tokio::test]
    async fn verified_download_and_cache_hit_return_complete_file_from_start() {
        let server = server().await;
        let (_directory, game) = game();
        let patch = private_patch_dir(&game).unwrap();
        let sha256 = digest(BODY);
        let events = Mutex::new(Vec::new());
        let mut result = fetch(&patch, &server.client, format!("{}/file", server.url).parse().unwrap(), &sha256,
            BODY.len() as u64, &AtomicBool::new(false), &|event| events.lock().unwrap().push(event.completed)).await.unwrap();
        let mut bytes = Vec::new();
        result.file.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, BODY);
        assert_eq!(result.sha256, sha256);
        assert_eq!(patch.entries().unwrap().count(), 1);
        assert_eq!(events.lock().unwrap().last().copied(), Some(BODY.len() as u64));
        // Public synchronous API must return without starting a runtime/network
        // request, even when invoked from this existing async runtime.
        let mut cached = download(&game, "https://localhost:1/not-contacted", &sha256.to_uppercase(), BODY.len() as u64,
            &AtomicBool::new(false), |_| {}).unwrap();
        bytes.clear();
        cached.file.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, BODY);
        assert_eq!(server.requests.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn hash_size_and_https_redirect_fail_without_publishing() {
        let server = server().await;
        let (_directory, game) = game();
        let patch = private_patch_dir(&game).unwrap();
        for (path, hash, size, expected) in [
            ("/file", "0".repeat(64), BODY.len() as u64, PatchErrorKind::Integrity),
            ("/file", digest(BODY), BODY.len() as u64 + 1, PatchErrorKind::Integrity),
            ("/redirect", digest(BODY), BODY.len() as u64, PatchErrorKind::Io),
        ] {
            assert_eq!(error(fetch(&patch, &server.client, format!("{}{path}", server.url).parse().unwrap(), &hash, size,
                &AtomicBool::new(false), &|_| {}).await), expected);
            empty(&patch);
        }
    }

    #[tokio::test]
    async fn oversized_stream_aborts_before_eof_and_removes_partial_file() {
        let server = server().await;
        let (_directory, game) = game();
        let patch = private_patch_dir(&game).unwrap();
        let cancelled = AtomicBool::new(false);
        let progress = |_| {};
        let result = tokio::time::timeout(Duration::from_secs(2), fetch(&patch, &server.client,
            format!("{}/oversize", server.url).parse().unwrap(), &digest(BODY), 1, &cancelled, &progress)).await.unwrap();
        assert_eq!(error(result), PatchErrorKind::Integrity);
        empty(&patch);
    }

    #[tokio::test]
    async fn cancellation_interrupts_stalled_body_and_removes_partial_file() {
        let server = server().await;
        let (_directory, game) = game();
        let patch = private_patch_dir(&game).unwrap();
        let cancelled = AtomicBool::new(false);
        let (ready, started) = tokio::sync::oneshot::channel();
        let ready = Mutex::new(Some(ready));
        // Signal only after headers and temporary-file creation, then cancel
        // while response.chunk() is pending on the stalled TLS socket.
        let progress = |_: PatchProgress| {
            if let Some(ready) = ready.lock().unwrap().take() { let _ = ready.send(()); }
        };
        let hash = digest(BODY);
        let fetch = fetch(&patch, &server.client, format!("{}/stall", server.url).parse().unwrap(),
            &hash, BODY.len() as u64, &cancelled, &progress);
        let cancel = async {
            started.await.unwrap();
            tokio::time::sleep(Duration::from_millis(50)).await;
            cancelled.store(true, Ordering::Release);
        };
        let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async { tokio::join!(fetch, cancel) }).await.unwrap();
        assert_eq!(error(result), PatchErrorKind::Cancelled);
        empty(&patch);
    }

    #[test]
    fn short_writer_and_corrupt_cache_are_rejected() {
        let (_directory, game) = game();
        let patch = private_patch_dir(&game).unwrap();
        let sha256 = digest(BODY);
        let mut writer = ArtifactWriter::new(&patch, &sha256).unwrap();
        writer.chunk(&BODY[..1], BODY.len() as u64).unwrap();
        assert_eq!(error(writer.finish(&sha256, BODY.len() as u64, &AtomicBool::new(false))), PatchErrorKind::Integrity);
        empty(&patch);
        patch.write(format!("{sha256}.bin"), vec![b'x'; BODY.len()]).unwrap();
        assert_eq!(error(cached(&patch, &sha256, BODY.len() as u64, &AtomicBool::new(false))), PatchErrorKind::Integrity);
    }

    #[test]
    fn server_path_is_never_used_as_a_local_name() {
        let (_directory, game) = game();
        let patch = private_patch_dir(&game).unwrap();
        let sha256 = digest(BODY);
        validate_request("https://localhost/../../outside?filename=../../escape", &sha256, BODY.len() as u64).unwrap();
        let mut writer = ArtifactWriter::new(&patch, &sha256).unwrap();
        writer.chunk(BODY, BODY.len() as u64).unwrap();
        writer.finish(&sha256, BODY.len() as u64, &AtomicBool::new(false)).unwrap();
        let names: Vec<_> = patch.entries().unwrap().map(|entry| entry.unwrap().file_name()).collect();
        assert_eq!(names, [std::ffi::OsString::from(format!("{sha256}.bin"))]);
        assert!(!game.exists("outside"));
        assert!(!game.exists("escape"));
    }

    #[test]
    fn hard_linked_cache_cannot_expose_an_outside_file() {
        let (directory, game) = game();
        let _patch = private_patch_dir(&game).unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("untouched");
        std::fs::write(&target, BODY).unwrap();
        let sha256 = digest(BODY);
        std::fs::hard_link(&target, directory.path().join(crate::GAME_STATE_DIR).join(crate::PATCH_DIR).join(format!("{sha256}.bin"))).unwrap();
        assert_eq!(error(download(&game, "https://localhost:1", &sha256, BODY.len() as u64, &AtomicBool::new(false), |_| {})), PatchErrorKind::Path);
        assert_eq!(std::fs::read(&target).unwrap(), BODY);
    }

    #[cfg(unix)]
    #[test]
    fn private_directory_and_cache_symlinks_cannot_escape() {
        let (_directory, game) = game();
        let outside = tempfile::tempdir().unwrap();
        game.symlink(outside.path(), crate::GAME_STATE_DIR).unwrap();
        assert_eq!(error(private_patch_dir(&game)), PatchErrorKind::Path);
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
        game.remove_file(crate::GAME_STATE_DIR).unwrap();
        let state = { game.create_dir(crate::GAME_STATE_DIR).unwrap(); game.open_dir(crate::GAME_STATE_DIR).unwrap() };
        state.symlink(outside.path(), crate::PATCH_DIR).unwrap();
        assert_eq!(error(private_patch_dir(&game)), PatchErrorKind::Path);
        state.remove_file(crate::PATCH_DIR).unwrap();
        let patch = private_patch_dir(&game).unwrap();
        let sha256 = digest(BODY);
        let target = outside.path().join("untouched");
        std::fs::write(&target, BODY).unwrap();
        patch.symlink(&target, format!("{sha256}.bin")).unwrap();
        assert_eq!(error(cached(&patch, &sha256, BODY.len() as u64, &AtomicBool::new(false))), PatchErrorKind::Path);
        assert_eq!(std::fs::read(&target).unwrap(), BODY);
    }

    #[cfg(windows)]
    #[test]
    fn private_directory_junctions_cannot_escape() {
        // Reuse paths.rs's privilege-independent junction fixture pattern.
        // This fixed test-only command is never exposed to scripts or downloads.
        let (directory, game) = game();
        let outside = tempfile::tempdir().unwrap();
        for link in [directory.path().join(crate::GAME_STATE_DIR),
            directory.path().join(crate::GAME_STATE_DIR).join(crate::PATCH_DIR)] {
            let status = std::process::Command::new("cmd.exe")
                .args(["/D", "/C", "mklink", "/J"])
                .arg(&link).arg(outside.path())
                .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
                .status().unwrap();
            assert!(status.success(), "creating junction fixture failed");
            assert_eq!(error(private_patch_dir(&game)), PatchErrorKind::Path);
            assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
            // Remove the junction itself, never its target tree.
            std::fs::remove_dir(&link).unwrap();
            if !directory.path().join(crate::GAME_STATE_DIR).exists() {
                game.create_dir(crate::GAME_STATE_DIR).unwrap();
            }
        }
    }
}
