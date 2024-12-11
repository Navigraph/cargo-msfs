use std::{
    collections::HashMap,
    fs::{self, File},
    io::{self, Cursor, Read, Write},
    path::PathBuf,
};

use anyhow::{anyhow, Context, Result};
use cab::Cabinet;
use msi::{Expr, Package, Row, Select};
use serde::Deserialize;
use zip::ZipArchive;

use crate::{get_data_dir, SimulatorVersion};

// Root URLs for SDK files for each version of MSFS
const MSFS2020_SDK_URL: &str = "https://sdk.flightsimulator.com/files/";
const MSFS2024_SDK_URL: &str = "https://sdk.flightsimulator.com/msfs2024/files/";

// File name of the manifest file located under the root URL
const MANIFEST_FILE: &str = "sdk.json";

// The download option key for the core SDK installer
const CORE_INSTALLER_KEY: &str = "SDK Installer (Core)";

// The folder in the SDK MSI to extract from
const MSFS2020_SDK_EXTRACT_FROM: &str = ".\\MSFS SDK\\";
const MSFS2024_SDK_EXTRACT_FROM: &str = ".\\MSFS 2024 SDK\\";

// Local destination folder names for the downloaded SDK
const MSFS2020_FOLDER_NAME: &str = "msfs2020";
const MSFS2024_FOLDER_NAME: &str = "msfs2024";

// File name within the local destination folder where the SDK version is stored
const VERSION_FILE_NAME: &str = "version.txt";

// WASI sysroot location, relative to the SDK installation. Valid for both SDK editions
const WASI_SYSROOT_PATH: &str = "WASM\\wasi-sysroot";

// Configuration
const CHUNK_SIZE: u64 = 1024;

/// A downloads "menu option" containing an optional value
///
/// For our case, the downloads menu key we are using will always have a Some() value
#[derive(Debug, Deserialize, Clone)]
pub struct DownloadsMenuOption {
    pub value: Option<String>,
}
// A specific SDK (game) version
#[derive(Debug, Deserialize, Clone)]
pub struct GameVersion {
    /// A hashmap of menu titles to URLs (among other things, but we only care about URLs here)
    pub downloads_menu: HashMap<String, DownloadsMenuOption>,
    /// Vec of SDK versions. Latest is always the last entry
    pub release_notes: Vec<String>,
}
/// The manifest of available SDK versions
#[derive(Debug, Deserialize, Clone)]
pub struct SdkManifest {
    pub game_versions: Vec<GameVersion>,
}

/// Extracts the long file name from a string containing both short and long. This works for strings that are only the long file name as well
///
/// See https://learn.microsoft.com/en-us/windows/win32/msi/filename
///
/// * `string` - The string to parse from. Example value: `vacirzcc.h|WASM_Static_Library.h`
fn get_long_file_name(string: &str) -> Result<&str> {
    string
        .split("|")
        .last()
        .context("couldn't get long file name")
}

/// Recursively traverses the directories and gets the full path of a directory entry
///
/// Note: There's not really a type safe way to guarantee in the function signature the row is actually a full Directory row with proper values
///
/// See https://learn.microsoft.com/en-us/windows/win32/msi/directory-table
///
/// * `directory` - The directory key (DirectoryDir)
/// * `directories` - A vec of rows from the directory table
fn get_directory_parent<'a>(directory: &'a str, directories: &'a Vec<Row>) -> Result<PathBuf> {
    let row = directories
        .iter()
        .find(|d| d["Directory"].as_str() == Some(directory))
        .context("couldn't find source row")?;

    if let Some(parent) = row["Directory_Parent"].as_str() {
        let parent_path_buf = get_directory_parent(parent, directories)?;
        Ok(parent_path_buf.join(get_long_file_name(
            row["DefaultDir"]
                .as_str()
                .context("couldn't get directory name")?,
        )?))
    } else {
        // root!
        Ok(PathBuf::from(""))
    }
}

/// Gets the latest SDK version information for the given simulator
///
/// * `version` - The simulator version to get for
pub fn get_latest_sdk_release(version: SimulatorVersion) -> Result<GameVersion> {
    let response = reqwest::blocking::get(format!(
        "{}{}",
        if version == SimulatorVersion::Msfs2020 {
            MSFS2020_SDK_URL
        } else {
            MSFS2024_SDK_URL
        },
        MANIFEST_FILE
    ))?
    .text()?;

    let manifest = serde_json::from_str::<SdkManifest>(&response)?;

    let latest_sdk = manifest
        .game_versions
        .first()
        .context("can't find game version for SDK")?;

    Ok(latest_sdk.clone())
}

/// Gets the latest SDK version string for the given simulator
///
/// Note: This difers from get_latest_sdk_release, as that returns a struct with extra data
///
/// * `version` - The simulator version to get for
pub fn get_latest_sdk_version(version: SimulatorVersion) -> Result<String> {
    // 2020's release notes are ordered from oldest to most recent, while 2024 is most recent to oldest
    if version == SimulatorVersion::Msfs2020 {
        Ok(get_latest_sdk_release(version)?
            .release_notes
            .last()
            .context("no available sdk version")?
            .to_string())
    } else {
        Ok(get_latest_sdk_release(version)?
            .release_notes
            .first()
            .context("no available sdk version")?
            .to_string())
    }
}

/// Gets the desired path for the given simulator
///
/// * `version` The simulator version to get the path for
pub fn get_sdk_path(version: SimulatorVersion) -> Result<PathBuf> {
    Ok(
        get_data_dir()?.join(if version == SimulatorVersion::Msfs2020 {
            MSFS2020_FOLDER_NAME
        } else {
            MSFS2024_FOLDER_NAME
        }),
    )
}

/// Gets the WASI sysroot path for the given simulator
///
/// * `version` The simulator version to get the path for
pub fn get_wasi_sysroot_path(version: SimulatorVersion) -> Result<PathBuf> {
    Ok(get_sdk_path(version)?.join(WASI_SYSROOT_PATH))
}

/// Gets the installed SDK version for the given simulator
///
/// * `version` - The simulator version to get for
pub fn get_installed_sdk_version(version: SimulatorVersion) -> Result<Option<String>> {
    Ok(
        match File::open(get_sdk_path(version)?.join(VERSION_FILE_NAME)) {
            Ok(mut file) => {
                let mut version = String::new();
                file.read_to_string(&mut version)?;
                Some(version)
            }
            Err(_) => None,
        },
    )
}

/// Removes the installed SDK for the given simulator
///
/// * `version` The simulator version to delete the SDK for
pub fn remove_sdk_version(version: SimulatorVersion) -> Result<()> {
    let path = get_sdk_path(version)?;

    // Clear the out directory and recreate it
    if path.exists() {
        fs::remove_dir_all(&path)?;
    }

    Ok(())
}

/// Installs the latest SDK version for the given simulator
///
/// * `version` - The simulator version to download for
/// * `download_progress_callback` - An optional callback to report download statistics. Useful for logging. Parameters: `downloaded: u64, total: u64`
pub fn install_latest_sdk<F>(
    version: SimulatorVersion,
    mut download_progress_callback: Option<F>,
) -> Result<()>
where
    F: FnMut(u64, u64),
{
    // Clear and recreate the SDK path
    let out_directory = get_sdk_path(version)?;
    remove_sdk_version(version)?;
    fs::create_dir_all(&out_directory)?;

    // Get latest SDK data
    let latest_sdk = get_latest_sdk_release(version)?;
    let download_url = latest_sdk
        .downloads_menu
        .get(CORE_INSTALLER_KEY)
        .context("can't find core installer download option")?
        .value
        .as_ref()
        .context("can't find core installer download url")?;
    let release_number = latest_sdk
        .release_notes
        .last()
        .context("couldn't get latest release number")?;

    // Download the installer
    let mut response = reqwest::blocking::get(format!(
        "{}{}",
        if version == SimulatorVersion::Msfs2020 {
            MSFS2020_SDK_URL
        } else {
            MSFS2024_SDK_URL
        },
        download_url
    ))?;
    let content_length = response
        .content_length()
        .context("couldn't get content length of response")?;

    let mut file = Cursor::new(Vec::new());
    loop {
        let mut buf = [0u8; CHUNK_SIZE as usize];
        match response.read(&mut buf) {
            Ok(0) => break, // End of file
            Ok(data_size) => {
                file.write_all(&buf[0..data_size])?;
                if let Some(callback) = download_progress_callback.as_mut() {
                    callback(file.position() + data_size as u64, content_length);
                }
            }
            Err(e) => return Err(anyhow!(e)),
        }
    }

    // Parse the MSI. Some releases are zipped MSI files with external CAB files, so we need to handle that. Otherwise, everything is included in the MSI.
    let mut msi = if download_url.ends_with(".zip") {
        let mut zip_archive = ZipArchive::new(file)?;

        // Find the MSI file in the zip listing
        let msi_file_name = zip_archive
            .file_names()
            .find(|f| f.ends_with(".msi"))
            .context("couldn't find msi in zip")?
            .to_string();

        // Read the MSI archive to a buffer and then create the package
        let mut msi_buffer = Vec::new();
        zip_archive
            .by_name(&msi_file_name)?
            .read_to_end(&mut msi_buffer)?;

        let mut package = Package::open(Cursor::new(msi_buffer))?;

        // Since the CAB files are external, we need to manually add them to the package. TODO: This is *really* ineff
        for cab_file_path in zip_archive
            .file_names()
            .filter_map(|f| {
                if f.ends_with(".cab") {
                    Some(f.to_string())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
        {
            // Get the file name from the path
            let cab_file_name = PathBuf::from(&cab_file_path)
                .file_name()
                .context("couldn't get file name")?
                .to_str()
                .context("couldn't convert to str")?
                .to_string();

            // Read the cab archive to a buffer
            let mut cab_buffer = Vec::new();
            zip_archive
                .by_name(&cab_file_path)?
                .read_to_end(&mut cab_buffer)?;

            // Write the stream
            let mut stream = package.write_stream(&cab_file_name)?;
            stream.write_all(&cab_buffer)?;
        }

        package
    } else {
        Package::open(file)?
    };

    // Query the MSI tables for info on file and folder names (see https://learn.microsoft.com/en-us/windows/win32/msi/database-tables for info on the values)
    let query = Select::table("File")
        .inner_join(
            Select::table("Component"),
            Expr::col("Component.Component").eq(Expr::col("File.Component_")),
        )
        .columns(&["File.File", "File.FileName", "Component.Directory_"]);
    let files = msi.select_rows(query)?.collect::<Vec<_>>();
    let directories = msi
        .select_rows(Select::table("Directory").columns(&[
            "Directory",
            "Directory_Parent",
            "DefaultDir",
        ]))?
        .collect::<Vec<_>>();

    // Create a map of the file ID to the full output relative path (e.g. filFQCSYDXD6IK3UAB8101TGG3B0387F7ZD to ./Foo/Bar/Baz.qux)
    let mut file_map = HashMap::new();
    for file in files {
        let file_id = file["File.File"].as_str().context("couldn't get file id")?;
        let file_name = get_long_file_name(
            file["File.FileName"]
                .as_str()
                .context("couldn't get file name")?,
        )?;
        let directory = get_directory_parent(
            file["Component.Directory_"]
                .as_str()
                .context("couldn't get file name")?,
            &directories,
        )?;
        file_map.insert(file_id.to_string(), directory.join(file_name));
    }

    // Write version file
    let mut version_file = File::create(out_directory.join(VERSION_FILE_NAME))?;
    version_file.write_all(release_number.as_bytes())?;

    // Write SDK files
    let extract_from = if version == SimulatorVersion::Msfs2020 {
        MSFS2020_SDK_EXTRACT_FROM
    } else {
        MSFS2024_SDK_EXTRACT_FROM
    };
    // A more efficient way would be to find the stream associated with a file, but that is not possible. Given that, we must loop over all streams
    for stream_name in msi.streams().collect::<Vec<_>>() {
        let stream = msi.read_stream(&stream_name)?;
        let mut cabinet = match Cabinet::new(stream) {
            Ok(cabinet) => cabinet,
            Err(_) => continue, // Not a cabinet file
        };
        // Since there is a weird ownership model of the crate we use, we need to go ahead and extract all the file names
        let files = cabinet
            .folder_entries()
            .flat_map(|f| f.file_entries())
            .map(|f| f.name().to_string())
            .collect::<Vec<_>>();
        for cab_file_name in files {
            // cab_file_name will be the file identifier, which we will query from the file path hashmap
            let entry = file_map
                .get(&cab_file_name)
                .context("couldn't find mapped file name")?;

            // Only extract the SDK files we care about
            if entry
                .as_os_str()
                .to_str()
                .context("couldn't convert to str")?
                .starts_with(extract_from)
            {
                // Calculate the path relative to the folder we are extracting
                let out_file_path = out_directory.join(entry.strip_prefix(extract_from)?);
                // Ensure directories exist
                let parent = out_file_path.parent().context("could not get parent")?;
                if !parent.exists() {
                    fs::create_dir_all(parent)?;
                }
                // Write
                let mut file = File::create(out_file_path)?;
                let mut data = cabinet.read_file(&cab_file_name)?;
                io::copy(&mut data, &mut file)?;
            }
        }
    }

    Ok(())
}
