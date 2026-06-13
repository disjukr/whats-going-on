use std::ffi::OsString;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use wgo_daemon_core::config::windows_program_data_config_path;
use wgo_daemon_core::DEFAULT_LISTEN_ADDR;
use windows_service::define_windows_service;
use windows_service::service::{
    ServiceAccess, ServiceAction, ServiceActionType, ServiceControl, ServiceControlAccept,
    ServiceErrorControl, ServiceExitCode, ServiceFailureActions, ServiceFailureResetPeriod,
    ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_dispatcher;
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

use crate::fs::WindowsFileService;
use crate::ipc::UserTrayPairingNotifier;
use std::sync::Arc;
use wgo_daemon_host::server::run_system_server;

pub const SERVICE_NAME: &str = "wgo-windows-system";
pub const SERVICE_DISPLAY_NAME: &str = "Whats Going On System Daemon";
const SERVICE_DESCRIPTION: &str = "Runs the whats-going-on Windows system daemon.";
const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;

static SERVICE_OPTIONS: OnceLock<ServiceRunOptions> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct ServiceRunOptions {
    pub listen: SocketAddr,
    pub config_path: PathBuf,
}

impl ServiceRunOptions {
    pub fn new(listen: SocketAddr, config_path: PathBuf) -> Self {
        Self {
            listen,
            config_path,
        }
    }
}

pub fn run_dispatcher(options: ServiceRunOptions) -> Result<()> {
    SERVICE_OPTIONS
        .set(options)
        .map_err(|_| anyhow!("service options were already initialized"))?;
    service_dispatcher::start(SERVICE_NAME, ffi_service_main)?;
    Ok(())
}

pub fn install_service(
    service_binary_path: PathBuf,
    listen: SocketAddr,
    config_path: PathBuf,
) -> Result<()> {
    let manager_access = ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE;
    let service_manager = ServiceManager::local_computer(None::<&str>, manager_access)?;
    let service_info = ServiceInfo {
        name: OsString::from(SERVICE_NAME),
        display_name: OsString::from(SERVICE_DISPLAY_NAME),
        service_type: SERVICE_TYPE,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: service_binary_path,
        launch_arguments: vec![
            OsString::from("service"),
            OsString::from("run"),
            OsString::from("--listen"),
            OsString::from(listen.to_string()),
            OsString::from("--config"),
            config_path.into_os_string(),
        ],
        dependencies: vec![],
        account_name: None,
        account_password: None,
    };
    let service_access = ServiceAccess::QUERY_STATUS
        | ServiceAccess::CHANGE_CONFIG
        | ServiceAccess::START
        | ServiceAccess::STOP;
    let service = service_manager
        .create_service(&service_info, service_access)
        .or_else(|_| service_manager.open_service(SERVICE_NAME, service_access))?;
    service.change_config(&service_info)?;
    service.set_description(SERVICE_DESCRIPTION)?;
    service.set_delayed_auto_start(true)?;
    service.update_failure_actions(ServiceFailureActions {
        reset_period: ServiceFailureResetPeriod::After(Duration::from_secs(24 * 60 * 60)),
        reboot_msg: None,
        command: None,
        actions: Some(vec![
            ServiceAction {
                action_type: ServiceActionType::Restart,
                delay: Duration::from_secs(5),
            },
            ServiceAction {
                action_type: ServiceActionType::Restart,
                delay: Duration::from_secs(30),
            },
            ServiceAction {
                action_type: ServiceActionType::None,
                delay: Duration::default(),
            },
        ]),
    })?;
    service.set_failure_actions_on_non_crash_failures(true)?;
    Ok(())
}

pub fn uninstall_service() -> Result<()> {
    let service_manager =
        ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
    let service = service_manager.open_service(
        SERVICE_NAME,
        ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE,
    )?;
    if service.query_status()?.current_state != ServiceState::Stopped {
        let _ = service.stop();
        wait_for_service_state(&service, ServiceState::Stopped, Duration::from_secs(15))?;
    }
    service.delete()?;
    Ok(())
}

pub fn start_service() -> Result<()> {
    let service_manager =
        ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
    let service = service_manager.open_service(
        SERVICE_NAME,
        ServiceAccess::QUERY_STATUS | ServiceAccess::START,
    )?;
    if service.query_status()?.current_state == ServiceState::Running {
        return Ok(());
    }
    service.start::<&str>(&[])?;
    wait_for_service_state(&service, ServiceState::Running, Duration::from_secs(30))
}

pub fn stop_service() -> Result<()> {
    let service_manager =
        ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
    let service = service_manager.open_service(
        SERVICE_NAME,
        ServiceAccess::QUERY_STATUS | ServiceAccess::STOP,
    )?;
    if service.query_status()?.current_state == ServiceState::Stopped {
        return Ok(());
    }
    let _ = service.stop()?;
    wait_for_service_state(&service, ServiceState::Stopped, Duration::from_secs(30))
}

fn wait_for_service_state(
    service: &windows_service::service::Service,
    target_state: ServiceState,
    timeout: Duration,
) -> Result<()> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if service.query_status()?.current_state == target_state {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Err(anyhow!(
        "service did not reach {target_state:?} within {timeout:?}"
    ))
}

define_windows_service!(ffi_service_main, service_main);

fn service_main(_arguments: Vec<OsString>) {
    if let Err(err) = run_service() {
        tracing::error!(?err, "Windows service stopped with an error");
    }
}

fn run_service() -> windows_service::Result<()> {
    let (shutdown_tx, shutdown_rx) = mpsc::channel();
    let event_handler = move |control_event| -> ServiceControlHandlerResult {
        match control_event {
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            ServiceControl::Stop | ServiceControl::Shutdown => {
                let _ = shutdown_tx.send(());
                ServiceControlHandlerResult::NoError
            }
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };
    let status_handle = service_control_handler::register(SERVICE_NAME, event_handler)?;
    status_handle.set_service_status(service_status(
        ServiceState::StartPending,
        ServiceControlAccept::empty(),
        1,
        Duration::from_secs(20),
    ))?;
    let options = SERVICE_OPTIONS
        .get()
        .cloned()
        .unwrap_or_else(default_service_options);
    status_handle.set_service_status(service_status(
        ServiceState::Running,
        ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
        0,
        Duration::default(),
    ))?;
    let result = run_server_until_shutdown(options, shutdown_rx);
    status_handle.set_service_status(service_status(
        ServiceState::StopPending,
        ServiceControlAccept::empty(),
        1,
        Duration::from_secs(10),
    ))?;
    let exit_code: u32 = if result.is_ok() { 0 } else { 1 };
    let mut stopped_status = service_status(
        ServiceState::Stopped,
        ServiceControlAccept::empty(),
        0,
        Duration::default(),
    );
    stopped_status.exit_code = ServiceExitCode::Win32(exit_code);
    status_handle.set_service_status(stopped_status)?;
    if let Err(err) = result {
        tracing::error!(?err, "system daemon failed while running as a service");
    }
    if exit_code == 0 {
        Ok(())
    } else {
        Err(windows_service::Error::Winapi(
            std::io::Error::from_raw_os_error(exit_code as i32),
        ))
    }
}

fn run_server_until_shutdown(
    options: ServiceRunOptions,
    shutdown_rx: mpsc::Receiver<()>,
) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let shutdown = tokio::task::spawn_blocking(move || shutdown_rx.recv());
        tokio::select! {
            result = run_system_server(
                options.listen,
                options.config_path,
                Arc::new(WindowsFileService),
                Some(Arc::new(UserTrayPairingNotifier)),
                "Windows system service",
            ) => result,
            _ = shutdown => Ok(()),
        }
    })
}

fn service_status(
    current_state: ServiceState,
    controls_accepted: ServiceControlAccept,
    checkpoint: u32,
    wait_hint: Duration,
) -> ServiceStatus {
    ServiceStatus {
        service_type: SERVICE_TYPE,
        current_state,
        controls_accepted,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint,
        wait_hint,
        process_id: None,
    }
}

fn default_service_options() -> ServiceRunOptions {
    ServiceRunOptions {
        listen: DEFAULT_LISTEN_ADDR
            .parse()
            .expect("default listen address is valid"),
        config_path: windows_program_data_config_path(),
    }
}
