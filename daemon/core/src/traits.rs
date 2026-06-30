use std::future::Future;
use std::pin::Pin;

use crate::rpc::{
    CreateNodeOp, DeleteMode, FsEntry, ReadFileReq, TrashItem, WriteFileChunk, WriteFileResult,
    WriteFileStart,
};

pub type BoxFutureResult<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, ServiceError>> + Send + 'a>>;

pub trait WriteFileChunkSource: Send {
    fn next_chunk(&mut self) -> BoxFutureResult<'_, Option<WriteFileChunk>>;
}

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("permission denied")]
    PermissionDenied,
    #[error("not found")]
    NotFound,
    #[error("already exists")]
    AlreadyExists,
    #[error("not a directory")]
    NotDirectory,
    #[error("not a regular file")]
    NotFile,
    #[error("invalid path")]
    InvalidPath,
    #[error("unsupported operation")]
    Unsupported,
    #[error("operation failed: {0}")]
    OperationFailed(String),
}

pub trait FileService: Send + Sync {
    fn roots(&self) -> BoxFutureResult<'_, Vec<FsEntry>>;
    fn list_directory(&self, path: String) -> BoxFutureResult<'_, Vec<FsEntry>>;
    fn read_file(&self, request: ReadFileReq) -> BoxFutureResult<'_, Vec<u8>>;
    fn write_file<'a>(
        &'a self,
        start: WriteFileStart,
        chunks: Box<dyn WriteFileChunkSource + 'a>,
    ) -> BoxFutureResult<'a, WriteFileResult>;
    fn create_node(&self, op: CreateNodeOp) -> BoxFutureResult<'_, ()>;
    fn rename_path(&self, from: String, to: String) -> BoxFutureResult<'_, ()>;
    fn delete_path(&self, path: String, mode: DeleteMode) -> BoxFutureResult<'_, ()>;
    fn trash_items(&self) -> BoxFutureResult<'_, Vec<TrashItem>>;
    fn restore_trash_item(&self, item_id: String) -> BoxFutureResult<'_, ()>;
    fn purge_trash_item(&self, item_id: String) -> BoxFutureResult<'_, ()>;
}
