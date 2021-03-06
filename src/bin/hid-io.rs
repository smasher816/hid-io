#[macro_use]
extern crate log;

#[cfg(windows)]
#[macro_use]
extern crate windows_service;

use clap::App;
use hid_io::{api, built_info, device, module};

use crate::device::{HIDIOMailbox, HIDIOMailer, HIDIOMessage};
use std::sync::mpsc::channel;

use hid_io::RUNNING;
use std::sync::atomic::Ordering;

#[cfg(windows)]
fn main() -> Result<(), std::io::Error> {
    let args: Vec<_> = std::env::args().collect();
    if args[1] == "-d" {
        service::run();
    } else {
        flexi_logger::Logger::with_env()
            .start()
            .unwrap_or_else(|e| panic!("Logger initialization failed {}", e));
        info!("Running in interactive mode");

        start();
    }

    Ok(())
}

#[cfg(not(windows))]
fn main() -> Result<(), std::io::Error> {
    flexi_logger::Logger::with_env()
        .start()
        .unwrap_or_else(|e| panic!("Logger initialization failed {}", e));

    start();
    Ok(())
}

/// Main entry point
fn start() {
    info!("Starting!");

    // Setup signal handler
    let r = RUNNING.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })
    .expect("Error setting Ctrl-C handler");
    println!("Press Ctrl-C to exit...");

    // Process command-line arguments
    // Most of the information is generated from Cargo.toml using built crate (build.rs)
    App::new(built_info::PKG_NAME.to_string())
        .version(
            format!(
                "{}{} - {}",
                built_info::PKG_VERSION,
                built_info::GIT_VERSION.map_or_else(|| "".to_owned(), |v| format!(" (git {})", v)),
                built_info::PROFILE,
            )
            .as_str(),
        )
        .author(built_info::PKG_AUTHORS)
        .about(format!("\n{}", built_info::PKG_DESCRIPTION).as_str())
        .after_help(
            format!(
                "{} ({}) -> {} ({})",
                built_info::RUSTC_VERSION,
                built_info::HOST,
                built_info::TARGET,
                built_info::BUILT_TIME_UTC,
            )
            .as_str(),
        )
        .get_matches();

    // Start initialization
    info!("Initializing HID-IO daemon...");

    let (mailer_writer, mailer_reader) = channel::<HIDIOMessage>();
    let mut mailer = HIDIOMailer::new(mailer_reader);

    let nodes1 = mailer.devices();
    let (sink1, mailbox1) = HIDIOMailbox::from_sender(mailer_writer.clone(), nodes1);
    mailer.register_listener(sink1);

    let nodes2 = mailer.devices();
    let (sink2, mailbox2) = HIDIOMailbox::from_sender(mailer_writer.clone(), nodes2);
    mailer.register_listener(sink2);

    // Initialize Modules
    let thread = module::initialize(mailbox2);

    // Initialize Devices
    device::initialize(mailer);

    // Initialize Cap'n'Proto API Server
    api::initialize(mailbox1);

    // Cleanup
    while RUNNING.load(Ordering::SeqCst) {}
    println!("Waiting for threads to finish...");
    thread.join().unwrap();
    println!("Exiting.");
}

#[cfg(windows)]
fn stop() {
    info!("Stopping!");
    let r = RUNNING.clone();
    r.store(false, Ordering::SeqCst);
}

#[cfg(windows)]
mod service {
    use flexi_logger::{opt_format, Logger};
    use hid_io::built_info;
    use windows_service::service_dispatcher;

    const SERVICE_NAME: &str = built_info::PKG_NAME;

    // Generate the windows service boilerplate.
    // The boilerplate contains the low-level service entry function (ffi_service_main) that parses
    // incoming service arguments into Vec<OsString> and passes them to user defined service
    // entry (my_service_main).
    define_windows_service!(ffi_service_main, my_service_main);

    use std::ffi::OsString;
    use std::time::Duration;
    use windows_service::service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    };
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};

    pub fn run() -> windows_service::Result<()> {
        // Register generated `ffi_service_main` with the system and start the service, blocking
        // this thread until the service is stopped.
        service_dispatcher::start(SERVICE_NAME, ffi_service_main)
    }

    fn my_service_main(arguments: Vec<OsString>) {
        Logger::with_env()
            .log_to_file()
            .directory("log_files")
            .format(opt_format)
            .start()
            .unwrap_or_else(|e| panic!("Logger initialization failed {}", e));
        info!("Running as service!");

        if let Err(_e) = run_service(arguments) {
            // Handle error in some way.
        }
    }

    fn run_service(arguments: Vec<OsString>) -> windows_service::Result<()> {
        let event_handler = move |control_event| -> ServiceControlHandlerResult {
            info!("EVENT: {:?}", control_event);
            match control_event {
                ServiceControl::Stop => {
                    crate::stop();
                    ServiceControlHandlerResult::NoError
                }
                ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
                _ => ServiceControlHandlerResult::NotImplemented,
            }
        };

        // Register system service event handler
        let status_handle = service_control_handler::register(SERVICE_NAME, event_handler)?;

        let next_status = ServiceStatus {
            // Should match the one from system service registry
            service_type: ServiceType::OwnProcess,
            // The new state
            current_state: ServiceState::Running,
            // Accept stop events when running
            controls_accepted: ServiceControlAccept::STOP,
            // Used to report an error when starting or stopping only, otherwise must be zero
            exit_code: ServiceExitCode::Win32(0),
            // Only used for pending states, otherwise must be zero
            checkpoint: 0,
            // Only used for pending states, otherwise must be zero
            wait_hint: Duration::default(),
        };

        // Tell the system that the service is running now
        status_handle.set_service_status(next_status)?;

        crate::start();

        // Tell the system that service has stopped.
        status_handle.set_service_status(ServiceStatus {
            service_type: ServiceType::OwnProcess,
            current_state: ServiceState::Stopped,
            controls_accepted: ServiceControlAccept::empty(),
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::default(),
        })?;

        Ok(())
    }
}
