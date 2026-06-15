use std::collections::BTreeMap;
use std::sync::OnceLock;

use thiserror::Error;

use crate::cbor::{CborError, Value};
use crate::traits::ServiceError;

pub const MAX_U53: u64 = 9_007_199_254_740_991;

#[derive(Debug, Error)]
pub enum RpcCodecError {
    #[error("cbor error: {0}")]
    Cbor(#[from] CborError),
    #[error("expected CBOR map")]
    ExpectedMap,
    #[error("expected CBOR array")]
    ExpectedArray,
    #[error("missing field {0}")]
    MissingField(u64),
    #[error("unexpected field type for field {0}")]
    WrongFieldType(u64),
    #[error("integer is out of range for field {0}")]
    IntegerOutOfRange(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum ProcId {
    GetDaemonInfo = 1,
    StartPairing = 2,
    CompletePairing = 3,
    RenewClientCredential = 4,
    SubscribeRoots = 5,
    SubscribeDirectory = 6,
    ReadFile = 7,
    WriteFile = 8,
    CreateNodes = 9,
    RenamePaths = 10,
    DeletePaths = 11,
}

impl ProcId {
    pub const SUPPORTED: [Self; 11] = [
        Self::GetDaemonInfo,
        Self::StartPairing,
        Self::CompletePairing,
        Self::RenewClientCredential,
        Self::SubscribeRoots,
        Self::SubscribeDirectory,
        Self::ReadFile,
        Self::WriteFile,
        Self::CreateNodes,
        Self::RenamePaths,
        Self::DeletePaths,
    ];

    pub fn as_u64(self) -> u64 {
        self as u64
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpcErrorPayload {
    pub code: RpcErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum RpcErrorCode {
    BadMessage = 1,
    Unauthorized = 2,
    MissingPayload = 3,
    NotImplemented = 4,
    PermissionDenied = 6,
    NotFound = 7,
    OperationFailed = 8,
    MalformedPayload = 9,
}

impl RpcErrorCode {
    pub fn from_u64(value: u64) -> Option<Self> {
        match value {
            1 => Some(Self::BadMessage),
            2 => Some(Self::Unauthorized),
            3 => Some(Self::MissingPayload),
            4 => Some(Self::NotImplemented),
            6 => Some(Self::PermissionDenied),
            7 => Some(Self::NotFound),
            8 => Some(Self::OperationFailed),
            9 => Some(Self::MalformedPayload),
            _ => None,
        }
    }

    pub fn as_u64(self) -> u64 {
        self as u64
    }
}

impl RpcErrorPayload {
    pub fn encode(&self) -> Vec<u8> {
        Value::Map(BTreeMap::from([
            (1, Value::U64(self.code.as_u64())),
            (2, Value::Text(self.message.clone())),
        ]))
        .encode()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, RpcCodecError> {
        let map = expect_map(Value::decode(bytes)?)?;
        Ok(Self {
            code: RpcErrorCode::from_u64(expect_u64(&map, 1)?)
                .ok_or(RpcCodecError::WrongFieldType(1))?,
            message: expect_text(&map, 2)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonInfo {
    pub supported_proc_ids: Vec<u64>,
    pub version: String,
    pub os: String,
}

impl DaemonInfo {
    pub fn current() -> Self {
        static CURRENT: OnceLock<DaemonInfo> = OnceLock::new();

        CURRENT
            .get_or_init(|| Self {
                supported_proc_ids: ProcId::SUPPORTED.into_iter().map(ProcId::as_u64).collect(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                os: current_os_name(),
            })
            .clone()
    }

    pub fn encode(&self) -> Vec<u8> {
        Value::Map(BTreeMap::from([
            (
                1,
                Value::Array(
                    self.supported_proc_ids
                        .iter()
                        .copied()
                        .map(Value::U64)
                        .collect(),
                ),
            ),
            (2, Value::Text(self.version.clone())),
            (3, Value::Text(self.os.clone())),
        ]))
        .encode()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, RpcCodecError> {
        let map = expect_map(Value::decode(bytes)?)?;
        Ok(Self {
            supported_proc_ids: expect_array(&map, 1)?
                .iter()
                .map(expect_u53_value)
                .collect::<Result<Vec<_>, _>>()?,
            version: expect_text(&map, 2)?,
            os: expect_text(&map, 3)?,
        })
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartPairingRequest {
    pub confirmation_code: String,
    pub client_label: String,
    pub client_id: Option<String>,
}

impl StartPairingRequest {
    pub fn encode(&self) -> Vec<u8> {
        let mut fields = BTreeMap::from([
            (1, Value::Text(self.confirmation_code.clone())),
            (2, Value::Text(self.client_label.clone())),
        ]);
        if let Some(client_id) = &self.client_id {
            fields.insert(3, Value::Text(client_id.clone()));
        }
        Value::Map(fields).encode()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, RpcCodecError> {
        let map = expect_map(Value::decode(bytes)?)?;
        Ok(Self {
            confirmation_code: expect_text(&map, 1)?,
            client_label: expect_text(&map, 2)?,
            client_id: optional_text(&map, 3)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartPairingResponse {
    pub pairing_code_expires_at_unix: i64,
}

impl StartPairingResponse {
    pub fn encode(&self) -> Vec<u8> {
        Value::Map(BTreeMap::from([(
            1,
            Value::I64(self.pairing_code_expires_at_unix),
        )]))
        .encode()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletePairingRequest {
    pub code: String,
}

impl CompletePairingRequest {
    pub fn encode(&self) -> Vec<u8> {
        Value::Map(BTreeMap::from([(1, Value::Text(self.code.clone()))])).encode()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, RpcCodecError> {
        let map = expect_map(Value::decode(bytes)?)?;
        Ok(Self {
            code: expect_text(&map, 1)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletePairingResponse {
    pub client_id: String,
    pub client_secret: String,
    pub client_credential_expires_at_unix: i64,
}

impl CompletePairingResponse {
    pub fn encode(&self) -> Vec<u8> {
        Value::Map(BTreeMap::from([
            (1, Value::Text(self.client_id.clone())),
            (2, Value::Text(self.client_secret.clone())),
            (3, Value::I64(self.client_credential_expires_at_unix)),
        ]))
        .encode()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, RpcCodecError> {
        let map = expect_map(Value::decode(bytes)?)?;
        Ok(Self {
            client_id: expect_text(&map, 1)?,
            client_secret: expect_text(&map, 2)?,
            client_credential_expires_at_unix: expect_i53(&map, 3)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenewClientCredentialResponse {
    pub client_credential_expires_at_unix: i64,
}

impl RenewClientCredentialResponse {
    pub fn encode(&self) -> Vec<u8> {
        Value::Map(BTreeMap::from([(
            1,
            Value::I64(self.client_credential_expires_at_unix),
        )]))
        .encode()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, RpcCodecError> {
        let map = expect_map(Value::decode(bytes)?)?;
        Ok(Self {
            client_credential_expires_at_unix: expect_i53(&map, 1)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubscribeDirectoryReq {
    pub path: String,
}

impl SubscribeDirectoryReq {
    pub fn encode(&self) -> Vec<u8> {
        Value::Map(BTreeMap::from([(1, Value::Text(self.path.clone()))])).encode()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, RpcCodecError> {
        let map = expect_map(Value::decode(bytes)?)?;
        Ok(Self {
            path: expect_text(&map, 1)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadFileReq {
    pub path: String,
    pub offset: Option<u64>,
    pub length: Option<u64>,
}

impl ReadFileReq {
    pub fn encode(&self) -> Vec<u8> {
        let mut map = BTreeMap::from([(1, Value::Text(self.path.clone()))]);
        if let Some(offset) = self.offset {
            map.insert(2, Value::U64(offset));
        }
        if let Some(length) = self.length {
            map.insert(3, Value::U64(length));
        }
        Value::Map(map).encode()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, RpcCodecError> {
        let map = expect_map(Value::decode(bytes)?)?;
        Ok(Self {
            path: expect_text(&map, 1)?,
            offset: optional_u53(&map, 2)?,
            length: optional_u53(&map, 3)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadFileChunk {
    pub offset: u64,
    pub bytes: Vec<u8>,
}

impl ReadFileChunk {
    pub fn encode(&self) -> Vec<u8> {
        Value::Map(BTreeMap::from([
            (1, Value::U64(self.offset)),
            (2, Value::Bytes(self.bytes.clone())),
        ]))
        .encode()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, RpcCodecError> {
        let map = expect_map(Value::decode(bytes)?)?;
        Ok(Self {
            offset: expect_u53(&map, 1)?,
            bytes: expect_bytes(&map, 2)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteFileReq {
    Start(WriteFileStart),
    Chunk(WriteFileChunk),
}

impl WriteFileReq {
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Self::Start(start) => start.to_value().encode(),
            Self::Chunk(chunk) => chunk.to_value().encode(),
        }
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, RpcCodecError> {
        let value = Value::decode(bytes)?;
        Self::from_value(&value)
    }

    pub fn from_value(value: &Value) -> Result<Self, RpcCodecError> {
        let (variant, fields) = expect_union(value)?;
        match variant {
            1 => Ok(Self::Start(WriteFileStart::from_fields(fields)?)),
            2 => Ok(Self::Chunk(WriteFileChunk::from_fields(fields)?)),
            _ => Err(RpcCodecError::WrongFieldType(0)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteFileStart {
    pub path: String,
    pub mode: WriteFileMode,
    pub expected_result_size: Option<u64>,
    pub modified_at_ms: Option<u64>,
}

impl WriteFileStart {
    fn to_value(&self) -> Value {
        let mut fields = BTreeMap::from([
            (1, Value::Text(self.path.clone())),
            (2, Value::U64(self.mode.as_u64())),
        ]);
        if let Some(expected_result_size) = self.expected_result_size {
            fields.insert(3, Value::U64(expected_result_size));
        }
        if let Some(modified_at_ms) = self.modified_at_ms {
            fields.insert(4, Value::U64(modified_at_ms));
        }
        union_value(1, fields)
    }

    fn from_fields(fields: &BTreeMap<u64, Value>) -> Result<Self, RpcCodecError> {
        Ok(Self {
            path: expect_text(fields, 1)?,
            mode: WriteFileMode::from_u64(expect_u64(fields, 2)?)
                .ok_or(RpcCodecError::WrongFieldType(2))?,
            expected_result_size: optional_u53(fields, 3)?,
            modified_at_ms: optional_u53(fields, 4)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteFileChunk {
    pub offset: Option<u64>,
    pub bytes: Vec<u8>,
}

impl WriteFileChunk {
    fn to_value(&self) -> Value {
        let mut fields = BTreeMap::from([(2, Value::Bytes(self.bytes.clone()))]);
        if let Some(offset) = self.offset {
            fields.insert(1, Value::U64(offset));
        }
        union_value(2, fields)
    }

    fn from_fields(fields: &BTreeMap<u64, Value>) -> Result<Self, RpcCodecError> {
        Ok(Self {
            offset: optional_u53(fields, 1)?,
            bytes: expect_bytes(fields, 2)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteFileResult {
    pub bytes_written: u64,
    pub result_size: u64,
    pub modified_at_ms: Option<u64>,
}

impl WriteFileResult {
    pub fn encode(&self) -> Vec<u8> {
        let mut fields = BTreeMap::from([
            (1, Value::U64(self.bytes_written)),
            (2, Value::U64(self.result_size)),
        ]);
        if let Some(modified_at_ms) = self.modified_at_ms {
            fields.insert(3, Value::U64(modified_at_ms));
        }
        Value::Map(fields).encode()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, RpcCodecError> {
        let map = expect_map(Value::decode(bytes)?)?;
        Ok(Self {
            bytes_written: expect_u53(&map, 1)?,
            result_size: expect_u53(&map, 2)?,
            modified_at_ms: optional_u53(&map, 3)?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum WriteFileMode {
    Create = 1,
    Replace = 2,
    Append = 3,
    Patch = 4,
}

impl WriteFileMode {
    pub fn from_u64(value: u64) -> Option<Self> {
        match value {
            1 => Some(Self::Create),
            2 => Some(Self::Replace),
            3 => Some(Self::Append),
            4 => Some(Self::Patch),
            _ => None,
        }
    }

    pub fn as_u64(self) -> u64 {
        self as u64
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum FsEntryKind {
    File = 1,
    Directory = 2,
    Symlink = 3,
    Other = 4,
}

impl FsEntryKind {
    pub fn from_u64(value: u64) -> Option<Self> {
        match value {
            1 => Some(Self::File),
            2 => Some(Self::Directory),
            3 => Some(Self::Symlink),
            4 => Some(Self::Other),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsEntry {
    pub name: String,
    pub path: String,
    pub kind: FsEntryKind,
    pub size: Option<u64>,
    pub modified_at_ms: Option<u64>,
    pub readonly: bool,
}

impl FsEntry {
    pub fn to_value(&self) -> Value {
        let mut map = BTreeMap::from([
            (1, Value::Text(self.name.clone())),
            (2, Value::Text(self.path.clone())),
            (3, Value::U64(self.kind as u64)),
            (6, Value::Bool(self.readonly)),
        ]);
        if let Some(size) = self.size {
            map.insert(4, Value::U64(size));
        }
        if let Some(modified_at_ms) = self.modified_at_ms {
            map.insert(5, Value::U64(modified_at_ms));
        }
        Value::Map(map)
    }

    pub fn from_value(value: &Value) -> Result<Self, RpcCodecError> {
        let Value::Map(map) = value else {
            return Err(RpcCodecError::ExpectedMap);
        };
        Ok(Self {
            name: expect_text(map, 1)?,
            path: expect_text(map, 2)?,
            kind: FsEntryKind::from_u64(expect_u64(map, 3)?)
                .ok_or(RpcCodecError::WrongFieldType(3))?,
            size: optional_u64(map, 4)?,
            modified_at_ms: optional_u64(map, 5)?,
            readonly: optional_bool(map, 6)?.unwrap_or(false),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootsTableEvent {
    Snapshot {
        rows: Vec<FsEntry>,
    },
    Patch {
        removes: Vec<RootEntryKey>,
        upserts: Vec<FsEntry>,
    },
    Closed {
        reason: RootsSubscriptionCloseReason,
    },
}

impl RootsTableEvent {
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Self::Snapshot { rows } => {
                union_value(1, BTreeMap::from([(1, fs_entries_value(rows))])).encode()
            }
            Self::Patch { removes, upserts } => union_value(
                2,
                BTreeMap::from([
                    (
                        1,
                        Value::Array(removes.iter().map(RootEntryKey::to_value).collect()),
                    ),
                    (2, fs_entries_value(upserts)),
                ]),
            )
            .encode(),
            Self::Closed { reason } => {
                union_value(3, BTreeMap::from([(1, reason.to_value())])).encode()
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectoryTableEvent {
    Snapshot {
        rows: Vec<FsEntry>,
    },
    Patch {
        removes: Vec<DirectoryEntryKey>,
        upserts: Vec<FsEntry>,
    },
    Closed {
        reason: DirectorySubscriptionCloseReason,
    },
}

impl DirectoryTableEvent {
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Self::Snapshot { rows } => {
                union_value(1, BTreeMap::from([(1, fs_entries_value(rows))])).encode()
            }
            Self::Patch { removes, upserts } => union_value(
                2,
                BTreeMap::from([
                    (
                        1,
                        Value::Array(removes.iter().map(DirectoryEntryKey::to_value).collect()),
                    ),
                    (2, fs_entries_value(upserts)),
                ]),
            )
            .encode(),
            Self::Closed { reason } => {
                union_value(3, BTreeMap::from([(1, reason.to_value())])).encode()
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootEntryKey {
    pub path: String,
}

impl RootEntryKey {
    fn to_value(&self) -> Value {
        Value::Map(BTreeMap::from([(1, Value::Text(self.path.clone()))]))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryEntryKey {
    pub name: String,
}

impl DirectoryEntryKey {
    fn to_value(&self) -> Value {
        Value::Map(BTreeMap::from([(1, Value::Text(self.name.clone()))]))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootsSubscriptionCloseReason {
    Failed,
    PermissionLost,
    Unknown,
}

impl RootsSubscriptionCloseReason {
    fn to_value(&self) -> Value {
        match self {
            Self::Failed => union_value(0, BTreeMap::new()),
            Self::PermissionLost => union_value(1, BTreeMap::new()),
            Self::Unknown => union_value(2, BTreeMap::new()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectorySubscriptionCloseReason {
    Failed,
    Deleted,
    Moved { to: Option<String> },
    PermissionLost,
    ReplacedByNonDirectory,
    Unknown,
}

impl DirectorySubscriptionCloseReason {
    fn to_value(&self) -> Value {
        match self {
            Self::Failed => union_value(0, BTreeMap::new()),
            Self::Deleted => union_value(1, BTreeMap::new()),
            Self::Moved { to } => {
                let mut fields = BTreeMap::new();
                if let Some(to) = to {
                    fields.insert(1, Value::Text(to.clone()));
                }
                union_value(2, fields)
            }
            Self::PermissionLost => union_value(3, BTreeMap::new()),
            Self::ReplacedByNonDirectory => union_value(4, BTreeMap::new()),
            Self::Unknown => union_value(5, BTreeMap::new()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateNodesReq {
    pub nodes: Vec<CreateNodeOp>,
}

impl CreateNodesReq {
    pub fn decode(bytes: &[u8]) -> Result<Self, RpcCodecError> {
        let map = expect_map(Value::decode(bytes)?)?;
        Ok(Self {
            nodes: expect_array(&map, 1)?
                .iter()
                .map(CreateNodeOp::from_value)
                .collect::<Result<_, _>>()?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateNodeOp {
    pub path: String,
    pub spec: CreateNodeSpec,
}

impl CreateNodeOp {
    fn from_value(value: &Value) -> Result<Self, RpcCodecError> {
        let Value::Map(map) = value else {
            return Err(RpcCodecError::ExpectedMap);
        };
        Ok(Self {
            path: expect_text(map, 1)?,
            spec: CreateNodeSpec::from_value(map.get(&2).ok_or(RpcCodecError::MissingField(2))?)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateNodeSpec {
    File,
    Directory,
    Symlink { target: String },
    Hardlink { target: String },
}

impl CreateNodeSpec {
    fn from_value(value: &Value) -> Result<Self, RpcCodecError> {
        let (variant, fields) = expect_union(value)?;
        match variant {
            1 => Ok(Self::File),
            2 => Ok(Self::Directory),
            3 => Ok(Self::Symlink {
                target: expect_text(fields, 1)?,
            }),
            4 => Ok(Self::Hardlink {
                target: expect_text(fields, 1)?,
            }),
            _ => Err(RpcCodecError::WrongFieldType(2)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenamePathsReq {
    pub ops: Vec<RenamePathOp>,
}

impl RenamePathsReq {
    pub fn decode(bytes: &[u8]) -> Result<Self, RpcCodecError> {
        let map = expect_map(Value::decode(bytes)?)?;
        Ok(Self {
            ops: expect_array(&map, 1)?
                .iter()
                .map(RenamePathOp::from_value)
                .collect::<Result<_, _>>()?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenamePathOp {
    pub from: String,
    pub to: String,
}

impl RenamePathOp {
    fn from_value(value: &Value) -> Result<Self, RpcCodecError> {
        let Value::Map(map) = value else {
            return Err(RpcCodecError::ExpectedMap);
        };
        Ok(Self {
            from: expect_text(map, 1)?,
            to: expect_text(map, 2)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletePathsReq {
    pub paths: Vec<String>,
    pub mode: DeleteMode,
}

impl DeletePathsReq {
    pub fn decode(bytes: &[u8]) -> Result<Self, RpcCodecError> {
        let map = expect_map(Value::decode(bytes)?)?;
        Ok(Self {
            paths: expect_array(&map, 1)?
                .iter()
                .map(expect_text_value)
                .collect::<Result<_, _>>()?,
            mode: DeleteMode::from_u64(expect_u64(&map, 2)?)
                .ok_or(RpcCodecError::WrongFieldType(2))?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum DeleteMode {
    Trash = 1,
    Permanent = 2,
}

impl DeleteMode {
    pub fn from_u64(value: u64) -> Option<Self> {
        match value {
            1 => Some(Self::Trash),
            2 => Some(Self::Permanent),
            _ => None,
        }
    }

    pub fn as_u64(self) -> u64 {
        self as u64
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BulkMutationRes {
    pub results: Vec<BulkMutationItemResult>,
}

impl BulkMutationRes {
    pub fn encode(&self) -> Vec<u8> {
        Value::Map(BTreeMap::from([(
            1,
            Value::Array(
                self.results
                    .iter()
                    .map(BulkMutationItemResult::to_value)
                    .collect(),
            ),
        )]))
        .encode()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BulkMutationItemResult {
    Failed {
        index: u64,
        error: FsMutationItemError,
    },
    Ok {
        index: u64,
    },
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

    fn to_value(&self) -> Value {
        match self {
            Self::Failed { index, error } => union_value(
                0,
                BTreeMap::from([(1, Value::U64(*index)), (2, error.to_value())]),
            ),
            Self::Ok { index } => union_value(1, BTreeMap::from([(1, Value::U64(*index))])),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsMutationItemError {
    Failed { message: String },
    PermissionDenied { message: String },
    NotFound { message: String },
    AlreadyExists { message: String },
    NotDirectory { message: String },
    NotFile { message: String },
    InvalidPath { message: String },
    Unsupported { message: String },
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

    fn to_value(&self) -> Value {
        let (variant, message) = match self {
            Self::Failed { message } => (0, message),
            Self::PermissionDenied { message } => (1, message),
            Self::NotFound { message } => (2, message),
            Self::AlreadyExists { message } => (3, message),
            Self::NotDirectory { message } => (4, message),
            Self::NotFile { message } => (5, message),
            Self::InvalidPath { message } => (6, message),
            Self::Unsupported { message } => (7, message),
        };
        union_value(variant, BTreeMap::from([(1, Value::Text(message.clone()))]))
    }
}

fn fs_entries_value(rows: &[FsEntry]) -> Value {
    Value::Array(rows.iter().map(FsEntry::to_value).collect())
}

fn union_value(variant: u64, fields: BTreeMap<u64, Value>) -> Value {
    Value::Array(vec![Value::U64(variant), Value::Map(fields)])
}

fn expect_union(value: &Value) -> Result<(u64, &BTreeMap<u64, Value>), RpcCodecError> {
    let Value::Array(items) = value else {
        return Err(RpcCodecError::ExpectedArray);
    };
    if items.len() != 2 {
        return Err(RpcCodecError::WrongFieldType(0));
    }
    let Value::U64(variant) = items[0] else {
        return Err(RpcCodecError::WrongFieldType(0));
    };
    let Value::Map(fields) = &items[1] else {
        return Err(RpcCodecError::ExpectedMap);
    };
    Ok((variant, fields))
}

fn expect_map(value: Value) -> Result<BTreeMap<u64, Value>, RpcCodecError> {
    match value {
        Value::Map(map) => Ok(map),
        _ => Err(RpcCodecError::ExpectedMap),
    }
}

fn expect_u64(map: &BTreeMap<u64, Value>, field: u64) -> Result<u64, RpcCodecError> {
    match map.get(&field).ok_or(RpcCodecError::MissingField(field))? {
        Value::U64(value) => Ok(*value),
        _ => Err(RpcCodecError::WrongFieldType(field)),
    }
}

fn expect_u53(map: &BTreeMap<u64, Value>, field: u64) -> Result<u64, RpcCodecError> {
    let value = expect_u64(map, field)?;
    if value <= MAX_U53 {
        Ok(value)
    } else {
        Err(RpcCodecError::IntegerOutOfRange(field))
    }
}

fn expect_i53(map: &BTreeMap<u64, Value>, field: u64) -> Result<i64, RpcCodecError> {
    match map.get(&field).ok_or(RpcCodecError::MissingField(field))? {
        Value::I64(value) if value.unsigned_abs() <= MAX_U53 => Ok(*value),
        Value::U64(value) if *value <= MAX_U53 => Ok(*value as i64),
        Value::I64(_) | Value::U64(_) => Err(RpcCodecError::IntegerOutOfRange(field)),
        _ => Err(RpcCodecError::WrongFieldType(field)),
    }
}

fn optional_u64(map: &BTreeMap<u64, Value>, field: u64) -> Result<Option<u64>, RpcCodecError> {
    match map.get(&field) {
        None => Ok(None),
        Some(Value::U64(value)) => Ok(Some(*value)),
        Some(_) => Err(RpcCodecError::WrongFieldType(field)),
    }
}

fn optional_u53(map: &BTreeMap<u64, Value>, field: u64) -> Result<Option<u64>, RpcCodecError> {
    optional_u64(map, field)?
        .map(|value| {
            if value <= MAX_U53 {
                Ok(value)
            } else {
                Err(RpcCodecError::IntegerOutOfRange(field))
            }
        })
        .transpose()
}

fn expect_text(map: &BTreeMap<u64, Value>, field: u64) -> Result<String, RpcCodecError> {
    match map.get(&field).ok_or(RpcCodecError::MissingField(field))? {
        Value::Text(value) => Ok(value.clone()),
        _ => Err(RpcCodecError::WrongFieldType(field)),
    }
}

fn optional_text(map: &BTreeMap<u64, Value>, field: u64) -> Result<Option<String>, RpcCodecError> {
    match map.get(&field) {
        None => Ok(None),
        Some(Value::Text(value)) => Ok(Some(value.clone())),
        Some(_) => Err(RpcCodecError::WrongFieldType(field)),
    }
}

fn expect_text_value(value: &Value) -> Result<String, RpcCodecError> {
    match value {
        Value::Text(value) => Ok(value.clone()),
        _ => Err(RpcCodecError::WrongFieldType(0)),
    }
}

fn expect_u53_value(value: &Value) -> Result<u64, RpcCodecError> {
    let Value::U64(value) = value else {
        return Err(RpcCodecError::WrongFieldType(0));
    };
    if *value <= MAX_U53 {
        Ok(*value)
    } else {
        Err(RpcCodecError::IntegerOutOfRange(0))
    }
}

fn optional_bool(map: &BTreeMap<u64, Value>, field: u64) -> Result<Option<bool>, RpcCodecError> {
    match map.get(&field) {
        None => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(RpcCodecError::WrongFieldType(field)),
    }
}

fn expect_bytes(map: &BTreeMap<u64, Value>, field: u64) -> Result<Vec<u8>, RpcCodecError> {
    match map.get(&field).ok_or(RpcCodecError::MissingField(field))? {
        Value::Bytes(value) => Ok(value.clone()),
        _ => Err(RpcCodecError::WrongFieldType(field)),
    }
}

fn expect_array<'a>(
    map: &'a BTreeMap<u64, Value>,
    field: u64,
) -> Result<&'a [Value], RpcCodecError> {
    match map.get(&field).ok_or(RpcCodecError::MissingField(field))? {
        Value::Array(value) => Ok(value),
        _ => Err(RpcCodecError::WrongFieldType(field)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_info_roundtrip() {
        let daemon_info = DaemonInfo::current();
        assert_eq!(
            DaemonInfo::decode(&daemon_info.encode()).unwrap(),
            daemon_info
        );
        assert_eq!(
            daemon_info.supported_proc_ids,
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]
        );
        assert_eq!(daemon_info.version, env!("CARGO_PKG_VERSION"));
        assert!(!daemon_info.os.is_empty());
        assert!(daemon_info.os.contains(&machine_bitness()));
    }

    #[test]
    fn macos_license_product_name_parses_rtf_heading() {
        let text = r"{\rtf1 SOFTWARE LICENSE AGREEMENT FOR macOS Tahoe 26\par}";
        assert_eq!(
            macos_license_product_name_from_text(text).as_deref(),
            Some("macOS Tahoe 26")
        );
    }

    #[test]
    fn macos_product_name_replaces_major_with_full_version() {
        assert_eq!(
            macos_product_name_with_version("macOS Tahoe 26", "26.5.1"),
            "macOS Tahoe 26.5.1"
        );
        assert_eq!(
            macos_product_name_with_version("macOS", "26.5.1"),
            "macOS 26.5.1"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_product_name_uses_windows_11_for_new_builds() {
        assert_eq!(
            normalize_windows_product_name("Windows 10 Pro".to_string(), Some("26200")),
            "Windows 11 Pro"
        );
        assert_eq!(
            normalize_windows_product_name("Windows 10 Pro".to_string(), Some("19045")),
            "Windows 10 Pro"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_build_label_includes_ubr_when_available() {
        assert_eq!(
            windows_build_label(Some("26200"), Some(8655)).as_deref(),
            Some("26200.8655")
        );
        assert_eq!(
            windows_build_label(Some("26200"), None).as_deref(),
            Some("26200")
        );
    }

    #[cfg(windows)]
    #[test]
    fn service_pack_label_does_not_duplicate_named_service_pack() {
        assert_eq!(service_pack_label("Service Pack 1"), "Service Pack 1");
        assert_eq!(
            service_pack_label("1000.26100.315.0"),
            "service pack 1000.26100.315.0"
        );
    }

    #[test]
    fn complete_pairing_roundtrip() {
        let request = StartPairingRequest {
            confirmation_code: "42".to_string(),
            client_label: "browser".to_string(),
            client_id: Some("client".to_string()),
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
            client_id: "client".to_string(),
            client_secret: "secret".to_string(),
            client_credential_expires_at_unix: 400,
        };
        assert_eq!(
            CompletePairingResponse::decode(&response.encode()).unwrap(),
            response
        );

        let response = RenewClientCredentialResponse {
            client_credential_expires_at_unix: 500,
        };
        assert_eq!(
            RenewClientCredentialResponse::decode(&response.encode()).unwrap(),
            response
        );
    }

    #[test]
    fn read_file_chunk_roundtrip() {
        let response = ReadFileChunk {
            offset: 4,
            bytes: b"hello".to_vec(),
        };
        assert_eq!(ReadFileChunk::decode(&response.encode()).unwrap(), response);
    }

    #[test]
    fn write_file_request_roundtrip() {
        let start = WriteFileReq::Start(WriteFileStart {
            path: "C:\\tmp\\foo.txt".to_string(),
            mode: WriteFileMode::Replace,
            expected_result_size: Some(5),
            modified_at_ms: None,
        });
        assert_eq!(WriteFileReq::decode(&start.encode()).unwrap(), start);

        let chunk = WriteFileReq::Chunk(WriteFileChunk {
            offset: Some(1),
            bytes: b"hello".to_vec(),
        });
        assert_eq!(WriteFileReq::decode(&chunk.encode()).unwrap(), chunk);
    }

    #[test]
    fn filesystem_snapshot_encodes_rows() {
        let rows = vec![
            FsEntry {
                name: "foo.txt".to_string(),
                path: "C:\\tmp\\foo.txt".to_string(),
                kind: FsEntryKind::File,
                size: Some(1234),
                modified_at_ms: Some(1_710_000_000_000),
                readonly: false,
            },
            FsEntry {
                name: "docs".to_string(),
                path: "C:\\tmp\\docs".to_string(),
                kind: FsEntryKind::Directory,
                size: None,
                modified_at_ms: None,
                readonly: true,
            },
        ];
        let encoded = DirectoryTableEvent::Snapshot { rows: rows.clone() }.encode();
        let Value::Array(items) = Value::decode(&encoded).unwrap() else {
            panic!("expected event union");
        };
        assert_eq!(items.first(), Some(&Value::U64(1)));
        let Value::Map(fields) = &items[1] else {
            panic!("expected event fields");
        };
        let Value::Array(encoded_rows) = fields.get(&1).unwrap() else {
            panic!("expected rows");
        };
        assert_eq!(
            encoded_rows
                .iter()
                .map(FsEntry::from_value)
                .collect::<Result<Vec<_>, _>>()
                .unwrap(),
            rows
        );
    }
}
