use std::{
    io::Cursor,
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
};

use anyhow::{anyhow, Context, Result};
use cargo_metadata::Message;
use clap::{Parser, ValueEnum};
use console::style;
use directories::ProjectDirs;
use indicatif::{ProgressBar, ProgressStyle};
use sdk::{
    get_installed_sdk_version, get_latest_sdk_version, get_sdk_path, get_wasi_sysroot_path,
    install_latest_sdk, remove_sdk_version,
};
use wasm_opt::{Feature, OptimizationOptions, Pass};

/// SDK info and download utility
mod sdk;

#[cfg(target_os = "windows")]
const BUILT_INS_PATH: &str = ".\\lib\\wasm32-wasi\\libclang_rt.builtins-wasm32.a";
#[cfg(not(target_os = "windows"))]
const BUILT_INS_PATH: &str = "./lib/wasm32-wasi/libclang_rt.builtins-wasm32.a";

#[cfg(target_os = "windows")]
const WASI_PATH: &str = ".\\lib\\wasm32-wasi";
#[cfg(not(target_os = "windows"))]
const WASI_PATH: &str = "./lib/wasm32-wasi";

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
    /// The path to the crate to build. This is only required for the build command type
    #[arg(short, required_if_eq_any([
        ("command", "build"),
    ]))]
    in_folder: Option<String>,
    /// The full path (including filename) to output the compiled WASM file. This is only required for the build command type
    #[arg(short, required_if_eq_any([
        ("command", "build"),
    ]))]
    out_wasm: Option<String>,
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

fn main() -> Result<()> {
    let args = Args::parse();

    match args.command {
        CommandType::Install => {
            let sim_version = args.msfs_version.unwrap();
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
            let sim_version = args.msfs_version.unwrap();
            if get_sdk_path(sim_version)?.exists() {
                remove_sdk_version(sim_version)?;
                print_success("SDK deleted");
            } else {
                print_info("SDK is not installed, nothing to remove");
            }
        }
        CommandType::Update => {
            let sim_version = args.msfs_version.unwrap();
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
            let sim_version = args.msfs_version.unwrap();

            // Assure we downloaded the SDK
            if get_installed_sdk_version(sim_version)?.is_none() {
                return Err(anyhow!("SDK not installed"));
            }

            // Locate SDK wasi-sysroot
            let sdk_path = get_sdk_path(sim_version)?;
            let wasi_sysroot_path = get_wasi_sysroot_path(sim_version)?;
            // Construct the build flags
            let flags = [
                "-Cstrip=symbols",
                "-Clto",
                "-Ctarget-feature=-crt-static,+bulk-memory",
                "-Clink-self-contained=no",
                "-Clink-arg=-l",
                "-Clink-arg=c",
                &format!(
                    "-Clink-arg={}",
                    wasi_sysroot_path.join(BUILT_INS_PATH).to_string_lossy()
                ),
                "-Clink-arg=-L",
                &format!(
                    "-Clink-arg={}",
                    wasi_sysroot_path.join(WASI_PATH).to_string_lossy()
                ),
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
            // Run build, capture output
            let command = Command::new("cargo")
                .args([
                    "build",
                    "--release",
                    "--target",
                    "wasm32-wasip1",
                    "--message-format=json",
                    "--verbose",
                ])
                .env("WASI_SYSROOT", wasi_sysroot_path.as_os_str())
                .env("MSFS_SDK", sdk_path)
                .env("RUSTFLAGS", flags.join(" "))
                .env(
                    "CFLAGS",
                    format!("--sysroot={}", wasi_sysroot_path.to_string_lossy()),
                )
                .current_dir(args.in_folder.unwrap())
                .stdout(Stdio::piped())
                .spawn()?
                .wait_with_output()?;

            // Map the JSON stdout to structures
            let messages = Message::parse_stream(Cursor::new(command.stdout))
                .map(|x| x.unwrap())
                .collect::<Vec<_>>();

            // Ensure build finished and did so successfully
            let Some(Message::BuildFinished(data)) = messages.last() else {
                return Err(anyhow!("build didn't finish"));
            };
            if !data.success {
                // Print out the compiler messages to guide user on what went wrong
                let compiler_messages = messages.iter().filter_map(|m| {
                    if let Message::CompilerMessage(compiler_message) = m {
                        Some(compiler_message)
                    } else {
                        None
                    }
                });

                for compiler_message in compiler_messages {
                    if let Some(message) = &compiler_message.message.rendered {
                        println!("{message}");
                    }
                }
                return Err(anyhow!("build did not finish successfully"));
            }

            // Find the output artifacts
            let out_artifact = messages
                .iter()
                .filter_map(|x| {
                    if let Message::CompilerArtifact(data) = x {
                        Some(data)
                    } else {
                        None
                    }
                })
                .last()
                .ok_or(anyhow!("couldn't get out artifact"))?;

            if out_artifact.filenames.len() > 1 {
                return Err(anyhow!(
                    "more than one file outputted for artifact, unsure how to proceed"
                ));
            }

            // Run wasm-opt
            let path = out_artifact
                .filenames
                .get(0)
                .ok_or(anyhow!("no filenames"))?;

            OptimizationOptions::new_opt_level_1()
                .add_pass(Pass::SignextLowering)
                .enable_feature(Feature::BulkMemory)
                .run(path, args.out_wasm.unwrap())?;
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

    Ok(())
}
