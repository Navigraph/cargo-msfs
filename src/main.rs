use std::{
    io::BufReader,
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
};

use anyhow::{Context, Result};
use cargo_metadata::Message;
use clap::{builder::ArgPredicate, Parser, ValueEnum};
use console::style;
use directories::ProjectDirs;
use indicatif::{ProgressBar, ProgressStyle};
use sdk::{
    get_installed_sdk_version, get_latest_sdk_release, get_latest_sdk_version, get_sdk_path,
    get_wasi_sysroot_path, install_latest_sdk, remove_sdk_version,
};

/// SDK info and download utility
mod sdk;

/// A specific version of MSFS
#[derive(Debug, PartialEq, Eq, Clone, Copy, ValueEnum)]
enum SimulatorVersion {
    Msfs2020,
    Msfs2024,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CommandType {
    /// Installs the SDK for a specified MSFS version
    Install,
    /// Removes the SDK for a specified MSFS version
    Remove,
    /// Updates the SDK for a specified MSFS version
    Update,
    /// Builds a crate for a specified MSFS version
    Build,
    /// Gets info on installed SDKs
    Info,
}

#[derive(Debug, Parser)]
struct Args {
    /// The command to run
    #[arg(value_enum)]
    command: CommandType,
    /// The version of MSFS to run for. This is optional if the command type is info
    #[arg(value_enum, required_if_eq_any([
        ("command", "install"),
        ("command", "remove"),
        ("command", "update"),
        ("command", "build"),
    ]))]
    msfs_version: Option<SimulatorVersion>,
}

/// Formats a string containing the installed SDK version of a given sim
///
/// Example: `MSFS2024 SDK version X.X.X is installed` or `MSFS 2024 SDK is not installed`
///
/// * `simulator_version` - The simulator version to format for
fn format_version_string(simulator_version: SimulatorVersion) -> Result<String> {
    let root_string = format!(
        "MSFS {} SDK",
        if simulator_version == SimulatorVersion::Msfs2020 {
            "2020"
        } else {
            "2024"
        }
    );

    if let Some(installed_version) = get_installed_sdk_version(simulator_version)? {
        Ok(format!(
            "{} version {} is installed, latest available version is {}",
            root_string,
            style(installed_version).bold(),
            style(get_latest_sdk_version(simulator_version)?).bold()
        ))
    } else {
        Ok(format!("{} is not installed", root_string))
    }
}

/// Gets the directory that can be used for data
fn get_data_dir() -> Result<PathBuf> {
    Ok(ProjectDirs::from("", "", "cargo-msfs")
        .context("could not get project dir")?
        .data_dir()
        .to_path_buf())
}

/// Logs info
fn print_info(message: &str) {
    println!("{} {}", style("[INFO]").cyan(), message);
}

/// Logs success
fn print_success(message: &str) {
    println!("{} {}", style("[SUCCESS]").green(), message);
}

/// Logs a step
fn print_step(step_number: u8, num_steps: u8, message: &str) {
    if step_number == num_steps {
        println!("{} {}", style("{step_number}/{num_steps}").green(), message);
    } else {
        println!(
            "{} {}",
            style("{step_number}/{num_steps}").yellow(),
            message
        );
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    let mut command = None;

    match args.command {
        CommandType::Install => {
            let sim_version = args.msfs_version.context("msfs version is not present")?;
            let installed_version = get_installed_sdk_version(sim_version)?;
            if installed_version.is_some() {
                print_info("SDK for simulator version is already installed. To update it, run with the update command");
                return Ok(());
            }

            print_info("Downloading and installing SDK...");
            // Create the progress bar. Since we won't know the full length until the callback, initialize with 0
            let progress_bar = ProgressBar::new(0);
            progress_bar.set_style(
                ProgressStyle::with_template(
                    "{spinner:.green} [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} (ETA {eta})",
                )
                .unwrap()
                .progress_chars("#>-"),
            );
            progress_bar.enable_steady_tick(Duration::from_millis(100));
            install_latest_sdk(
                sim_version,
                Some(|downloaded, total| {
                    if progress_bar.length() != Some(total) {
                        progress_bar.set_length(total);
                    }

                    progress_bar.set_position(downloaded);
                }),
            )?;
            print_success("SDK installed");
        }
        CommandType::Remove => {
            let sim_version = args.msfs_version.context("msfs version is not present")?;
            if get_sdk_path(sim_version)?.exists() {
                remove_sdk_version(sim_version)?;
                print_success("SDK deleted");
            } else {
                print_info("SDK is not installed, nothing to remove");
            }
        }
        CommandType::Update => {
            let sim_version = args.msfs_version.context("msfs version is not present")?;
            let latest_release = get_latest_sdk_version(sim_version)?;
            let installed_version = get_installed_sdk_version(sim_version)?;
            if installed_version == Some(latest_release) {
                print_info("Latest SDK is already installed");
                return Ok(());
            } else if installed_version.is_none() {
                print_info("SDK is not installed. To install it, run with the install command");
                return Ok(());
            }

            print_info("Downloading and installing SDK...");
            // Create the progress bar. Since we won't know the full length until the callback, initialize with 0
            let progress_bar = ProgressBar::new(0);
            progress_bar.set_style(
                ProgressStyle::with_template(
                    "{spinner:.green} [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} (ETA {eta})",
                )
                .unwrap()
                .progress_chars("#>-"),
            );
            progress_bar.enable_steady_tick(Duration::from_millis(100));
            install_latest_sdk(
                sim_version,
                Some(|downloaded, total| {
                    if progress_bar.length() != Some(total) {
                        progress_bar.set_length(total);
                    }

                    progress_bar.set_position(downloaded);
                }),
            )?;
            print_success("SDK updated");
        }
        CommandType::Build => {
            let sim_version = args.msfs_version.context("msfs version is not present")?;
            let sdk_path = get_sdk_path(sim_version)?;
            let wasi_sysroot_path = get_wasi_sysroot_path(sim_version)?
                .as_os_str()
                .to_str()
                .context("couldn't convert osstr to str")?
                .to_string();
            let flags = [
                "-Cstrip=symbols",
                "-Clto",
                "-Ctarget-feature=-crt-static,+bulk-memory",
                "-Clink-self-contained=no",
                "-Clink-arg=-l",
                "-Clink-arg=c",
                &format!(
                    "-Clink-arg={}\\lib\\wasm32-wasi\\libclang_rt.builtins-wasm32.a",
                    wasi_sysroot_path
                ),
                "-Clink-arg=-L",
                &format!("-Clink-arg={}\\lib\\wasm32-wasi", wasi_sysroot_path),
                "-Clink-arg=--export-table",
                "-Clink-arg=--allow-undefined",
                "-Clink-arg=--export-dynamic",
                "-Clink-arg=--export=__wasm_call_ctors",
                "-Clink-arg=--export=malloc",
                "-Clink-arg=--export=free",
                "-Clink-arg=--export=mark_decommit_pages",
                "-Clink-arg=--export=mallinfo",
                "-Clink-arg=--export=mchunkit_begin",
                "-Clink-arg=--export=mchunkit_next",
                "-Clink-arg=--export=get_pages_state",
            ];
            command = Some(
                Command::new("cargo")
                    .args([
                        "build",
                        "--release",
                        "--target",
                        "wasm32-wasip1",
                        // "--message-format=json",
                    ])
                    .env("WASI_SYSROOT", &wasi_sysroot_path)
                    .env("MSFS_SDK", sdk_path)
                    .env("RUSTFLAGS", flags.join(" "))
                    .env("CFLAGS", format!("--sysroot={}", wasi_sysroot_path))
                    .stdout(Stdio::piped())
                    .spawn()?,
            );

            // let reader = BufReader::new(command.stdout.take().context("couldn't take stdout")?);
            // for message in Message::parse_stream(reader) {
            //     dbg!(message?);
            // }
        }

        CommandType::Info => {
            if args.msfs_version == None || args.msfs_version == Some(SimulatorVersion::Msfs2020) {
                print_info(&format_version_string(SimulatorVersion::Msfs2020)?);
            }
            if args.msfs_version == None || args.msfs_version == Some(SimulatorVersion::Msfs2024) {
                print_info(&format_version_string(SimulatorVersion::Msfs2024)?);
            }
        }
    }

    if let Some(command) = command {
        let output = command.wait_with_output().unwrap();
        print!("{}", String::from_utf8_lossy(&output.stdout));
    }

    Ok(())
}
