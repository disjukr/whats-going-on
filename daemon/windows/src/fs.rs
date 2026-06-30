use std::fs::{self, FileTimes, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use wgo_daemon_core::rpc::{
    CreateNodeOp, CreateNodeSpec, DeleteMode, FsEntry, FsEntryKind, ReadFileReq,
    TrashItem as RpcTrashItem, TrashItemSize as RpcTrashItemSize, WriteFileChunk, WriteFileMode,
    WriteFileResult, WriteFileStart, MAX_U53,
};
use wgo_daemon_core::traits::{BoxFutureResult, FileService, ServiceError, WriteFileChunkSource};

#[derive(Debug, Default, Clone)]
pub struct WindowsFileService;

impl FileService for WindowsFileService {
    fn roots(&self) -> BoxFutureResult<'_, Vec<FsEntry>> {
        Box::pin(async move {
            let mut roots = Vec::new();
            for drive in b'A'..=b'Z' {
                let path = format!("{}:\\", drive as char);
                if Path::new(&path).exists() {
                    roots.push(FsEntry {
                        name: path.clone(),
                        path,
                        kind: FsEntryKind::Directory,
                        size: None,
                        modified_at_ms: None,
                        readonly: false,
                    });
                }
            }
            Ok(roots)
        })
    }

    fn list_directory(&self, path: String) -> BoxFutureResult<'_, Vec<FsEntry>> {
        Box::pin(async move {
            let mut entries = Vec::new();
            for entry in fs::read_dir(&path).map_err(map_io_error)? {
                let Ok(entry) = entry else {
                    continue;
                };
                let path = entry.path();
                entries.push(match fs::symlink_metadata(&path) {
                    Ok(metadata) => to_fs_entry(path, metadata),
                    Err(_) => fallback_fs_entry(path),
                });
            }
            entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            Ok(entries)
        })
    }

    fn read_file(&self, request: ReadFileReq) -> BoxFutureResult<'_, Vec<u8>> {
        Box::pin(async move {
            let metadata = fs::symlink_metadata(&request.path).map_err(map_io_error)?;
            if !metadata.is_file() {
                return Err(ServiceError::NotFile);
            }
            let mut file = fs::File::open(&request.path).map_err(map_io_error)?;
            file.seek(SeekFrom::Start(request.offset.unwrap_or(0)))
                .map_err(map_io_error)?;
            let mut bytes = Vec::new();
            match request.length {
                Some(length) => {
                    file.take(length)
                        .read_to_end(&mut bytes)
                        .map_err(map_io_error)?;
                }
                None => {
                    file.read_to_end(&mut bytes).map_err(map_io_error)?;
                }
            }
            Ok(bytes)
        })
    }

    fn write_file<'a>(
        &'a self,
        start: WriteFileStart,
        mut chunks: Box<dyn WriteFileChunkSource + 'a>,
    ) -> BoxFutureResult<'a, WriteFileResult> {
        Box::pin(async move {
            let path = PathBuf::from(&start.path);
            ensure_parent_directory_exists(&path)?;
            let bytes_written = write_file_chunks(&path, start.mode, chunks.as_mut()).await?;
            set_modified_time_best_effort(&path, start.modified_at_ms);
            let metadata = fs::metadata(&path).map_err(map_io_error)?;
            let result_size = metadata.len();
            if result_size > MAX_U53 {
                return Err(ServiceError::OperationFailed(
                    "result file size exceeds u53".to_string(),
                ));
            }
            if start
                .expected_result_size
                .is_some_and(|expected| expected != result_size)
            {
                return Err(ServiceError::OperationFailed(
                    "result file size does not match expectedResultSize".to_string(),
                ));
            }
            Ok(WriteFileResult {
                bytes_written,
                result_size,
                modified_at_ms: metadata_modified_at_ms(&metadata),
            })
        })
    }

    fn create_node(&self, op: CreateNodeOp) -> BoxFutureResult<'_, ()> {
        Box::pin(async move {
            let path = PathBuf::from(&op.path);
            match op.spec {
                CreateNodeSpec::Directory => create_directory_node(&path),
                CreateNodeSpec::File => {
                    create_parent_directories(&path)?;
                    OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(path)
                        .map(|_| ())
                        .map_err(map_io_error)
                }
                CreateNodeSpec::Symlink { target } => {
                    create_parent_directories(&path)?;
                    create_symlink_node(&target, &path)
                }
                CreateNodeSpec::Hardlink { target } => {
                    create_parent_directories(&path)?;
                    let target_metadata = fs::symlink_metadata(&target).map_err(map_io_error)?;
                    if !target_metadata.is_file() {
                        return Err(ServiceError::NotFile);
                    }
                    fs::hard_link(target, path).map_err(map_io_error)
                }
            }
        })
    }

    fn rename_path(&self, from: String, to: String) -> BoxFutureResult<'_, ()> {
        Box::pin(async move { fs::rename(from, to).map_err(map_io_error) })
    }

    fn delete_path(&self, path: String, mode: DeleteMode) -> BoxFutureResult<'_, ()> {
        Box::pin(async move {
            match mode {
                DeleteMode::Trash => trash::delete(path)
                    .map_err(|err| ServiceError::OperationFailed(err.to_string())),
                DeleteMode::Permanent => delete_permanently(Path::new(&path)),
            }
        })
    }

    fn trash_items(&self) -> BoxFutureResult<'_, Vec<RpcTrashItem>> {
        Box::pin(async move {
            let mut rows = trash::os_limited::list()
                .map_err(|err| ServiceError::OperationFailed(err.to_string()))?
                .iter()
                .map(to_rpc_trash_item)
                .collect::<Vec<_>>();
            rows.sort_by(|a, b| {
                b.deleted_at_ms
                    .cmp(&a.deleted_at_ms)
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                    .then_with(|| a.id.cmp(&b.id))
            });
            Ok(rows)
        })
    }

    fn restore_trash_item(&self, item_id: String) -> BoxFutureResult<'_, ()> {
        Box::pin(async move {
            let item = find_trash_item(&item_id)?;
            trash::os_limited::restore_all([item])
                .map_err(|err| ServiceError::OperationFailed(err.to_string()))
        })
    }

    fn purge_trash_item(&self, item_id: String) -> BoxFutureResult<'_, ()> {
        Box::pin(async move {
            let item = find_trash_item(&item_id)?;
            trash::os_limited::purge_all([item])
                .map_err(|err| ServiceError::OperationFailed(err.to_string()))
        })
    }
}

fn find_trash_item(item_id: &str) -> Result<trash::TrashItem, ServiceError> {
    trash::os_limited::list()
        .map_err(|err| ServiceError::OperationFailed(err.to_string()))?
        .into_iter()
        .find(|item| item.id.to_string_lossy() == item_id)
        .ok_or(ServiceError::NotFound)
}

fn to_rpc_trash_item(item: &trash::TrashItem) -> RpcTrashItem {
    RpcTrashItem {
        id: item.id.to_string_lossy().to_string(),
        name: item.name.to_string_lossy().to_string(),
        original_parent: item.original_parent.to_string_lossy().to_string(),
        deleted_at_ms: unix_seconds_to_u53_ms(item.time_deleted),
        size: trash::os_limited::metadata(item)
            .ok()
            .and_then(|metadata| to_rpc_trash_item_size(metadata.size)),
    }
}

fn to_rpc_trash_item_size(size: trash::TrashItemSize) -> Option<RpcTrashItemSize> {
    match size {
        trash::TrashItemSize::Bytes(value) if value <= MAX_U53 => {
            Some(RpcTrashItemSize::Bytes { value })
        }
        trash::TrashItemSize::Entries(value) if (value as u64) <= MAX_U53 => {
            Some(RpcTrashItemSize::Entries {
                value: value as u64,
            })
        }
        _ => None,
    }
}

fn unix_seconds_to_u53_ms(seconds: i64) -> Option<u64> {
    if seconds < 0 {
        return None;
    }
    (seconds as u64)
        .checked_mul(1000)
        .filter(|value| *value <= MAX_U53)
}

async fn write_file_chunks(
    path: &Path,
    mode: WriteFileMode,
    chunks: &mut dyn WriteFileChunkSource,
) -> Result<u64, ServiceError> {
    let mut bytes_written = 0u64;
    match mode {
        WriteFileMode::Create => {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .map_err(map_io_error)?;
            write_seekable_chunks(&mut file, chunks, false, &mut bytes_written).await?;
        }
        WriteFileMode::Replace => {
            if let Ok(metadata) = fs::symlink_metadata(path) {
                if !metadata.is_file() {
                    return Err(ServiceError::NotFile);
                }
            }
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(path)
                .map_err(map_io_error)?;
            write_seekable_chunks(&mut file, chunks, false, &mut bytes_written).await?;
        }
        WriteFileMode::Append => {
            ensure_regular_file_exists(path)?;
            let mut file = OpenOptions::new()
                .append(true)
                .open(path)
                .map_err(map_io_error)?;
            while let Some(chunk) = chunks.next_chunk().await? {
                if chunk.offset.is_some() {
                    return Err(ServiceError::InvalidPath);
                }
                file.write_all(&chunk.bytes).map_err(map_io_error)?;
                bytes_written = add_chunk_len(bytes_written, &chunk)?;
            }
        }
        WriteFileMode::Patch => {
            ensure_regular_file_exists(path)?;
            let mut file = OpenOptions::new()
                .write(true)
                .open(path)
                .map_err(map_io_error)?;
            write_seekable_chunks(&mut file, chunks, true, &mut bytes_written).await?;
        }
    }
    Ok(bytes_written)
}

async fn write_seekable_chunks(
    file: &mut fs::File,
    chunks: &mut dyn WriteFileChunkSource,
    require_offset: bool,
    bytes_written: &mut u64,
) -> Result<(), ServiceError> {
    while let Some(chunk) = chunks.next_chunk().await? {
        match chunk.offset {
            Some(offset) => {
                file.seek(SeekFrom::Start(offset)).map_err(map_io_error)?;
            }
            None if require_offset => return Err(ServiceError::InvalidPath),
            None => {}
        }
        file.write_all(&chunk.bytes).map_err(map_io_error)?;
        *bytes_written = add_chunk_len(*bytes_written, &chunk)?;
    }
    Ok(())
}

fn add_chunk_len(current: u64, chunk: &WriteFileChunk) -> Result<u64, ServiceError> {
    current
        .checked_add(chunk.bytes.len() as u64)
        .filter(|value| *value <= MAX_U53)
        .ok_or_else(|| ServiceError::OperationFailed("bytesWritten exceeds u53".to_string()))
}

fn ensure_regular_file_exists(path: &Path) -> Result<(), ServiceError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(ServiceError::NotFile),
        Err(err) => Err(map_io_error(err)),
    }
}

fn set_modified_time_best_effort(path: &Path, modified_at_ms: Option<u64>) {
    let Some(modified_at_ms) = modified_at_ms else {
        return;
    };
    let Some(modified_time) = UNIX_EPOCH.checked_add(Duration::from_millis(modified_at_ms)) else {
        return;
    };
    let Ok(file) = OpenOptions::new().write(true).open(path) else {
        return;
    };
    let times = FileTimes::new().set_modified(modified_time);
    let _ = file.set_times(times);
}

fn to_fs_entry(path: PathBuf, metadata: fs::Metadata) -> FsEntry {
    let file_type = metadata.file_type();
    let kind = if file_type.is_symlink() {
        FsEntryKind::Symlink
    } else if metadata.is_dir() {
        FsEntryKind::Directory
    } else if metadata.is_file() {
        FsEntryKind::File
    } else {
        FsEntryKind::Other
    };
    FsEntry {
        name: fs_entry_name(&path),
        path: path.to_string_lossy().to_string(),
        kind,
        size: metadata.is_file().then_some(metadata.len()),
        modified_at_ms: metadata_modified_at_ms(&metadata),
        readonly: metadata.permissions().readonly(),
    }
}

fn metadata_modified_at_ms(metadata: &fs::Metadata) -> Option<u64> {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as u64)
}

fn fallback_fs_entry(path: PathBuf) -> FsEntry {
    FsEntry {
        name: fs_entry_name(&path),
        path: path.to_string_lossy().to_string(),
        kind: FsEntryKind::Other,
        size: None,
        modified_at_ms: None,
        readonly: true,
    }
}

fn fs_entry_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}

fn create_directory_node(path: &Path) -> Result<(), ServiceError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(ServiceError::AlreadyExists),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(path).map_err(map_io_error)
        }
        Err(err) => Err(map_io_error(err)),
    }
}

fn create_parent_directories(path: &Path) -> Result<(), ServiceError> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(map_io_error)?;
    }
    Ok(())
}

fn ensure_parent_directory_exists(path: &Path) -> Result<(), ServiceError> {
    let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    else {
        return Ok(());
    };
    match fs::symlink_metadata(parent) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(ServiceError::NotDirectory),
        Err(err) => Err(map_io_error(err)),
    }
}

fn delete_permanently(path: &Path) -> Result<(), ServiceError> {
    let metadata = fs::symlink_metadata(path).map_err(map_io_error)?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path).map_err(map_io_error)
    } else {
        fs::remove_file(path).map_err(map_io_error)
    }
}

#[cfg(windows)]
fn create_symlink_node(target: &str, path: &Path) -> Result<(), ServiceError> {
    use std::os::windows::fs::{symlink_dir, symlink_file};

    let target_path = symlink_target_path(target, path);
    if target_path.is_dir() {
        symlink_dir(target, path).map_err(map_io_error)
    } else {
        symlink_file(target, path).map_err(map_io_error)
    }
}

#[cfg(unix)]
fn create_symlink_node(target: &str, path: &Path) -> Result<(), ServiceError> {
    std::os::unix::fs::symlink(target, path).map_err(map_io_error)
}

#[cfg(not(any(windows, unix)))]
fn create_symlink_node(_target: &str, _path: &Path) -> Result<(), ServiceError> {
    Err(ServiceError::Unsupported)
}

#[cfg(windows)]
fn symlink_target_path(target: &str, link_path: &Path) -> PathBuf {
    let target_path = PathBuf::from(target);
    if target_path.is_absolute() {
        target_path
    } else {
        link_path
            .parent()
            .map(|parent| parent.join(&target_path))
            .unwrap_or(target_path)
    }
}

fn map_io_error(err: std::io::Error) -> ServiceError {
    match err.kind() {
        std::io::ErrorKind::NotFound => ServiceError::NotFound,
        std::io::ErrorKind::PermissionDenied => ServiceError::PermissionDenied,
        std::io::ErrorKind::AlreadyExists => ServiceError::AlreadyExists,
        std::io::ErrorKind::InvalidInput | std::io::ErrorKind::InvalidData => {
            ServiceError::InvalidPath
        }
        std::io::ErrorKind::NotADirectory => ServiceError::NotDirectory,
        std::io::ErrorKind::IsADirectory => ServiceError::NotFile,
        _ => ServiceError::OperationFailed(err.to_string()),
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    struct VecWriteFileChunkSource {
        chunks: std::vec::IntoIter<WriteFileChunk>,
    }

    impl WriteFileChunkSource for VecWriteFileChunkSource {
        fn next_chunk(&mut self) -> BoxFutureResult<'_, Option<WriteFileChunk>> {
            Box::pin(async move { Ok(self.chunks.next()) })
        }
    }

    #[tokio::test]
    async fn list_directory_tolerates_profile_entries_without_metadata() {
        let Some(profile) = std::env::var_os("USERPROFILE") else {
            return;
        };

        let rows = WindowsFileService
            .list_directory(PathBuf::from(profile).to_string_lossy().to_string())
            .await
            .unwrap();

        assert!(!rows.is_empty());
    }

    #[test]
    fn fallback_entry_preserves_name_and_marks_unknown_readonly() {
        let entry = fallback_fs_entry(PathBuf::from(r"C:\Users\user\nul"));

        assert_eq!(entry.name, "nul");
        assert_eq!(entry.kind, FsEntryKind::Other);
        assert_eq!(entry.size, None);
        assert_eq!(entry.modified_at_ms, None);
        assert!(entry.readonly);
    }

    #[tokio::test]
    async fn write_file_reports_best_effort_modified_time() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.txt");
        let modified_at_ms = 1_710_000_000_000;

        let result = WindowsFileService
            .write_file(
                WriteFileStart {
                    path: path.to_string_lossy().to_string(),
                    mode: WriteFileMode::Replace,
                    expected_result_size: Some(5),
                    modified_at_ms: Some(modified_at_ms),
                },
                Box::new(VecWriteFileChunkSource {
                    chunks: vec![WriteFileChunk {
                        offset: None,
                        bytes: b"hello".to_vec(),
                    }]
                    .into_iter(),
                }),
            )
            .await
            .unwrap();

        assert_eq!(result.bytes_written, 5);
        assert_eq!(result.result_size, 5);
        let returned = result.modified_at_ms.unwrap();
        assert!(returned.abs_diff(modified_at_ms) <= 1_000);
    }
}
