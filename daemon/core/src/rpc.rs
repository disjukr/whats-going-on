use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::generated::rpc::{
    WriteFileReq as GeneratedWriteFileReq, WriteTerminalInputReq as GeneratedWriteTerminalInputReq,
};
use crate::traits::ServiceError;
pub use crate::wire::{RpcErrorCode, RpcErrorPayload};

pub const MAX_U53: u64 = 9_007_199_254_740_991;

pub type RpcCodecError = crate::generated::rpc::CodecError;

pub use crate::generated::rpc::{
    AttachTerminalSessionError, AttachTerminalSessionReq, AvailableShellInfo, AvailableShellKey,
    AvailableShellsTableEvent, BulkMutationItemResult, BulkMutationRes, ClientInfo, ClientKey,
    ClientsTableEvent, CloseTerminalSessionError, CloseTerminalSessionReq, CompletePairingError,
    CompletePairingReq, CompletePairingRes, CreateNodeOp, CreateNodeSpec, CreateNodesReq,
    CreateTerminalSessionError, CreateTerminalSessionReq, DaemonInfo, DeleteMode, DeletePathsReq,
    DirectoryEntryKey, DirectorySubscriptionCloseReason, DirectoryTableEvent, FsEntry, FsEntryKind,
    FsMutationError, FsMutationItemError, GetDaemonInfoError, ProcDefinition, ProcId, ProcStream,
    PurgeTrashItemsReq, ReadFileChunk, ReadFileError, ReadFileReq, RenamePathOp, RenamePathsReq,
    RenewClientCredentialError, RenewClientCredentialRes, RestoreTrashItemsReq, RootEntryKey,
    RootsSubscriptionCloseReason, RootsTableEvent, RpcRequest, RpcRequestDecodeError, RpcResponse,
    StartPairingError, StartPairingReq, StartPairingRes, SubscribeAvailableShellsError,
    SubscribeClientsError, SubscribeDirectoryError, SubscribeDirectoryReq, SubscribeRootsError,
    SubscribeTrashItemsError, TakeTerminalControlError, TakeTerminalControlReq,
    TakeTerminalControlRes, TerminalEvent, TerminalExit, TerminalLaunchSpec,
    TerminalSessionCloseReason, TerminalSessionInfo, TerminalSessionKey,
    TerminalSessionsTableEvent, TrashItem, TrashItemSize, TrashItemsSubscriptionCloseReason,
    TrashItemsTableEvent, WriteFileError, WriteFileMode, WriteFileResult, WriteTerminalInputError,
    PROC_DEFINITIONS,
};

pub type StartPairingRequest = StartPairingReq;
pub type StartPairingResponse = StartPairingRes;
pub type CompletePairingRequest = CompletePairingReq;
pub type CompletePairingResponse = CompletePairingRes;
pub type RenewClientCredentialResponse = RenewClientCredentialRes;

pub const SUPPORTED_PROCS: [ProcId; 22] = [
    ProcId::GetDaemonInfo,
    ProcId::StartPairing,
    ProcId::CompletePairing,
    ProcId::RenewClientCredential,
    ProcId::SubscribeRoots,
    ProcId::SubscribeDirectory,
    ProcId::ReadFile,
    ProcId::WriteFile,
    ProcId::CreateNodes,
    ProcId::RenamePaths,
    ProcId::DeletePaths,
    ProcId::CreateTerminalSession,
    ProcId::SubscribeTerminalSessions,
    ProcId::SubscribeAvailableShells,
    ProcId::AttachTerminalSession,
    ProcId::TakeTerminalControl,
    ProcId::WriteTerminalInput,
    ProcId::CloseTerminalSession,
    ProcId::SubscribeClients,
    ProcId::SubscribeTrashItems,
    ProcId::RestoreTrashItems,
    ProcId::PurgeTrashItems,
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct DaemonProcessInfo {
    supported_proc_ids: Vec<u64>,
    version: String,
    os: String,
    instance_id: String,
    started_at_ms: u64,
}

impl DaemonProcessInfo {
    fn current() -> Self {
        let started_at_ms = current_unix_ms();
        Self {
            supported_proc_ids: SUPPORTED_PROCS.into_iter().map(ProcId::as_u64).collect(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            os: current_os_name(),
            instance_id: format!("{started_at_ms}-{}", std::process::id()),
            started_at_ms,
        }
    }
}

impl DaemonInfo {
    pub fn current() -> Self {
        static PROCESS_INFO: OnceLock<DaemonProcessInfo> = OnceLock::new();

        let process_info = PROCESS_INFO.get_or_init(DaemonProcessInfo::current);
        Self {
            supported_proc_ids: process_info.supported_proc_ids.clone(),
            version: process_info.version.clone(),
            os: process_info.os.clone(),
            instance_id: process_info.instance_id.clone(),
            started_at_ms: process_info.started_at_ms,
            server_time_ms: current_unix_ms(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteFileReq {
    Start(WriteFileStart),
    Chunk(WriteFileChunk),
}

impl WriteFileReq {
    pub fn encode(&self) -> Vec<u8> {
        self.to_generated().encode()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, RpcCodecError> {
        Self::from_generated(GeneratedWriteFileReq::decode(bytes)?)
    }

    fn to_generated(&self) -> GeneratedWriteFileReq {
        match self {
            Self::Start(start) => GeneratedWriteFileReq::WriteFileStart {
                path: start.path.clone(),
                mode: start.mode.clone(),
                expected_result_size: start.expected_result_size,
                modified_at_ms: start.modified_at_ms,
            },
            Self::Chunk(chunk) => GeneratedWriteFileReq::WriteFileChunk {
                offset: chunk.offset,
                bytes: chunk.bytes.clone(),
            },
        }
    }

    fn from_generated(value: GeneratedWriteFileReq) -> Result<Self, RpcCodecError> {
        Ok(match value {
            GeneratedWriteFileReq::WriteFileStart {
                path,
                mode,
                expected_result_size,
                modified_at_ms,
            } => Self::Start(WriteFileStart {
                path,
                mode,
                expected_result_size,
                modified_at_ms,
            }),
            GeneratedWriteFileReq::WriteFileChunk { offset, bytes } => {
                Self::Chunk(WriteFileChunk { offset, bytes })
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteFileStart {
    pub path: String,
    pub mode: WriteFileMode,
    pub expected_result_size: Option<u64>,
    pub modified_at_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteFileChunk {
    pub offset: Option<u64>,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteTerminalInputReq {
    Start {
        terminal_session_id: String,
        attach_id: String,
    },
    Chunk {
        bytes: Vec<u8>,
    },
}

impl WriteTerminalInputReq {
    pub fn encode(&self) -> Vec<u8> {
        self.to_generated().encode()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, RpcCodecError> {
        Ok(match GeneratedWriteTerminalInputReq::decode(bytes)? {
            GeneratedWriteTerminalInputReq::WriteTerminalInputStart {
                terminal_session_id,
                attach_id,
            } => Self::Start {
                terminal_session_id,
                attach_id,
            },
            GeneratedWriteTerminalInputReq::WriteTerminalInputChunk { bytes } => {
                Self::Chunk { bytes }
            }
        })
    }

    fn to_generated(&self) -> GeneratedWriteTerminalInputReq {
        match self {
            Self::Start {
                terminal_session_id,
                attach_id,
            } => GeneratedWriteTerminalInputReq::WriteTerminalInputStart {
                terminal_session_id: terminal_session_id.clone(),
                attach_id: attach_id.clone(),
            },
            Self::Chunk { bytes } => GeneratedWriteTerminalInputReq::WriteTerminalInputChunk {
                bytes: bytes.clone(),
            },
        }
    }
}

impl BulkMutationItemResult {
    pub fn ok(index: usize) -> Self {
        Self::Ok {
            index: index as u64,
        }
    }

    pub fn failed(index: usize, error: ServiceError) -> Self {
        Self::Failed {
            index: index as u64,
            error: FsMutationItemError::from_service_error(error),
        }
    }
}

impl FsMutationItemError {
    fn from_service_error(error: ServiceError) -> Self {
        let message = error.to_string();
        match error {
            ServiceError::PermissionDenied => Self::PermissionDenied { message },
            ServiceError::NotFound => Self::NotFound { message },
            ServiceError::AlreadyExists => Self::AlreadyExists { message },
            ServiceError::NotDirectory => Self::NotDirectory { message },
            ServiceError::NotFile => Self::NotFile { message },
            ServiceError::InvalidPath => Self::InvalidPath { message },
            ServiceError::Unsupported => Self::Unsupported { message },
            ServiceError::OperationFailed(_) => Self::Failed { message },
        }
    }
}

fn current_unix_ms() -> u64 {
    let Ok(duration) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return 0;
    };
    let millis = duration.as_millis();
    if millis > u128::from(MAX_U53) {
        MAX_U53
    } else {
        millis as u64
    }
}

fn current_os_name() -> String {
    platform_os_name()
}

#[cfg(windows)]
fn platform_os_name() -> String {
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY};
    use winreg::RegKey;

    let current_version = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(
            r"SOFTWARE\Microsoft\Windows NT\CurrentVersion",
            KEY_READ | KEY_WOW64_64KEY,
        )
        .ok();

    let product_name = current_version
        .as_ref()
        .and_then(|key| registry_string(key, "ProductName"));
    let build_number = current_version.as_ref().and_then(|key| {
        registry_string(key, "CurrentBuildNumber").or_else(|| registry_string(key, "CurrentBuild"))
    });
    let ubr = current_version
        .as_ref()
        .and_then(|key| registry_u32(key, "UBR"));
    let display_version = current_version.as_ref().and_then(|key| {
        registry_string(key, "DisplayVersion").or_else(|| registry_string(key, "ReleaseId"))
    });
    let service_pack = current_version.as_ref().and_then(|key| {
        registry_string(key, "CSDVersion").or_else(|| registry_string(key, "CSDBuildNumber"))
    });

    let name = normalize_windows_product_name(
        product_name.unwrap_or_else(|| "Windows".to_string()),
        build_number.as_deref(),
    );

    let mut parts = vec![name, machine_bitness()];
    if let Some(display_version) = display_version {
        parts.push(display_version);
    }
    if let Some(build) = windows_build_label(build_number.as_deref(), ubr) {
        parts.push(format!("build {build}"));
    }
    if let Some(service_pack) = service_pack {
        parts.push(service_pack_label(&service_pack));
    }
    parts.join(" ")
}

#[cfg(target_os = "macos")]
fn platform_os_name() -> String {
    let Some(product_version) = macos_sw_vers("-productVersion") else {
        return format!("macOS {}", machine_bitness());
    };

    let display_name = macos_license_product_name()
        .map(|name| macos_product_name_with_version(&name, &product_version))
        .unwrap_or_else(|| format!("macOS {product_version}"));

    format!("{} {}", display_name, machine_bitness())
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn platform_os_name() -> String {
    format!("{} {}", std::env::consts::OS, machine_bitness())
}

#[cfg(target_os = "macos")]
fn macos_sw_vers(argument: &str) -> Option<String> {
    let output = std::process::Command::new("/usr/bin/sw_vers")
        .arg(argument)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    String::from_utf8(output.stdout)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(target_os = "macos")]
fn macos_license_product_name() -> Option<String> {
    std::fs::read_to_string(
        "/System/Library/CoreServices/Setup Assistant.app/Contents/Resources/en.lproj/OSXSoftwareLicense.rtf",
    )
    .ok()
    .and_then(|text| macos_license_product_name_from_text(&text))
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn macos_license_product_name_from_text(text: &str) -> Option<String> {
    const PREFIX: &str = "SOFTWARE LICENSE AGREEMENT FOR macOS ";

    let rest = text.split_once(PREFIX)?.1;
    let label = rest
        .split(['\\', '\r', '\n', '<'])
        .next()
        .unwrap_or(rest)
        .trim()
        .trim_end_matches('.')
        .trim();
    if label.is_empty() {
        None
    } else {
        Some(format!("macOS {label}"))
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn macos_product_name_with_version(product_name: &str, product_version: &str) -> String {
    let Some(major_version) = product_version
        .split('.')
        .next()
        .filter(|value| !value.is_empty())
    else {
        return product_name.to_string();
    };

    let suffix = format!(" {major_version}");
    if let Some(name) = product_name.strip_suffix(&suffix) {
        format!("{name} {product_version}")
    } else {
        format!("{product_name} {product_version}")
    }
}

#[cfg(not(windows))]
fn machine_bitness() -> String {
    format!("{}bit", usize::BITS)
}

#[cfg(windows)]
fn machine_bitness() -> String {
    let arch = std::env::var("PROCESSOR_ARCHITEW6432")
        .or_else(|_| std::env::var("PROCESSOR_ARCHITECTURE"))
        .unwrap_or_else(|_| std::env::consts::ARCH.to_string());
    let arch = arch.to_ascii_uppercase();
    if arch.contains("64") {
        "64bit".to_string()
    } else if arch == "X86" {
        "32bit".to_string()
    } else {
        format!("{}bit", usize::BITS)
    }
}

#[cfg(windows)]
fn registry_string(key: &winreg::RegKey, name: &str) -> Option<String> {
    key.get_value::<String, _>(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(windows)]
fn registry_u32(key: &winreg::RegKey, name: &str) -> Option<u32> {
    key.get_value::<u32, _>(name).ok()
}

#[cfg(windows)]
fn normalize_windows_product_name(product_name: String, build_number: Option<&str>) -> String {
    let Some(build_number) = build_number.and_then(parse_u32) else {
        return product_name;
    };
    if build_number < 22_000 || !product_name.starts_with("Windows 10") {
        return product_name;
    }
    product_name.replacen("Windows 10", "Windows 11", 1)
}

#[cfg(windows)]
fn windows_build_label(build_number: Option<&str>, ubr: Option<u32>) -> Option<String> {
    let build_number = build_number?.trim();
    if build_number.is_empty() {
        return None;
    }
    Some(match ubr {
        Some(ubr) => format!("{build_number}.{ubr}"),
        None => build_number.to_string(),
    })
}

#[cfg(windows)]
fn service_pack_label(service_pack: &str) -> String {
    let service_pack = service_pack.trim();
    let lower = service_pack.to_ascii_lowercase();
    if lower.starts_with("service pack") || lower.starts_with("sp") {
        service_pack.to_string()
    } else {
        format!("service pack {service_pack}")
    }
}

#[cfg(windows)]
fn parse_u32(value: &str) -> Option<u32> {
    value.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use crate::rpc::{
        CompletePairingRequest, CompletePairingResponse, DaemonInfo, DirectoryTableEvent, FsEntry,
        FsEntryKind, ReadFileChunk, RenewClientCredentialResponse, StartPairingRequest,
        WriteFileChunk, WriteFileMode, WriteFileReq, WriteFileStart, SUPPORTED_PROCS,
    };

    #[test]
    fn daemon_info_roundtrip() {
        let daemon_info = DaemonInfo::current();
        assert_eq!(
            daemon_info.supported_proc_ids,
            SUPPORTED_PROCS
                .into_iter()
                .map(crate::rpc::ProcId::as_u64)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            DaemonInfo::decode(&daemon_info.encode()).unwrap(),
            daemon_info
        );
    }

    #[test]
    fn pairing_payloads_roundtrip() {
        let request = StartPairingRequest {
            confirmation_code: "42".to_string(),
            client_label: "test client".to_string(),
            client_id: Some("client-1".to_string()),
        };
        assert_eq!(
            StartPairingRequest::decode(&request.encode()).unwrap(),
            request
        );

        let request = CompletePairingRequest {
            code: "123456".to_string(),
        };
        assert_eq!(
            CompletePairingRequest::decode(&request.encode()).unwrap(),
            request
        );

        let response = CompletePairingResponse {
            client_id: "client-1".to_string(),
            client_secret: "secret".to_string(),
            client_credential_expires_at_unix: 1234,
        };
        assert_eq!(
            CompletePairingResponse::decode(&response.encode()).unwrap(),
            response
        );

        let response = RenewClientCredentialResponse {
            client_credential_expires_at_unix: 5678,
        };
        assert_eq!(
            RenewClientCredentialResponse::decode(&response.encode()).unwrap(),
            response
        );
    }

    #[test]
    fn read_file_chunk_roundtrip() {
        let response = ReadFileChunk {
            offset: 7,
            bytes: vec![1, 2, 3],
        };
        assert_eq!(ReadFileChunk::decode(&response.encode()).unwrap(), response);
    }

    #[test]
    fn write_file_request_roundtrip() {
        let start = WriteFileReq::Start(WriteFileStart {
            path: "/tmp/file.txt".to_string(),
            mode: WriteFileMode::Replace,
            expected_result_size: Some(3),
            modified_at_ms: Some(42),
        });
        assert_eq!(WriteFileReq::decode(&start.encode()).unwrap(), start);

        let chunk = WriteFileReq::Chunk(WriteFileChunk {
            offset: Some(5),
            bytes: vec![1, 2, 3],
        });
        assert_eq!(WriteFileReq::decode(&chunk.encode()).unwrap(), chunk);
    }

    #[test]
    fn filesystem_snapshot_encodes_rows() {
        let rows = vec![FsEntry {
            name: "file.txt".to_string(),
            path: "/tmp/file.txt".to_string(),
            kind: FsEntryKind::File,
            size: Some(12),
            modified_at_ms: Some(34),
            readonly: false,
        }];
        let encoded = DirectoryTableEvent::Snapshot { rows: rows.clone() }.encode();
        assert_eq!(
            DirectoryTableEvent::decode(&encoded).unwrap(),
            DirectoryTableEvent::Snapshot { rows }
        );
    }

    #[test]
    fn macos_license_product_name_from_text_extracts_marketing_name() {
        assert_eq!(
            super::macos_license_product_name_from_text(
                "x SOFTWARE LICENSE AGREEMENT FOR macOS Sequoia\\foo"
            ),
            Some("macOS Sequoia".to_string())
        );
    }

    #[test]
    fn macos_product_name_with_version_replaces_major_suffix() {
        assert_eq!(
            super::macos_product_name_with_version("macOS Sequoia 15", "15.5"),
            "macOS Sequoia 15.5"
        );
    }

    #[test]
    fn macos_product_name_with_version_appends_when_suffix_missing() {
        assert_eq!(
            super::macos_product_name_with_version("macOS Sequoia", "15.5"),
            "macOS Sequoia 15.5"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_product_name_normalizes_windows_10_registry_on_windows_11_builds() {
        assert_eq!(
            super::normalize_windows_product_name("Windows 10 Pro".to_string(), Some("26100")),
            "Windows 11 Pro"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_product_name_keeps_windows_10_for_old_builds() {
        assert_eq!(
            super::normalize_windows_product_name("Windows 10 Pro".to_string(), Some("19045")),
            "Windows 10 Pro"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_build_label_includes_ubr() {
        assert_eq!(
            super::windows_build_label(Some("26100"), Some(4351)),
            Some("26100.4351".to_string())
        );
    }

    #[cfg(windows)]
    #[test]
    fn service_pack_label_adds_prefix_for_numeric_value() {
        assert_eq!(
            super::service_pack_label("1000.26100.315.0"),
            "service pack 1000.26100.315.0"
        );
    }
}
