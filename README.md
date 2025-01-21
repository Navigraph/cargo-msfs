
<div align="center" >
  <a href="https://navigraph.com">
    <img src="https://navigraph.com/assets/images/navigraph_logo_only.svg" alt="Logo" width="80" height="80">
  </a>

  <div align="center">
    <h1>Cargo MSFS</h1>
  </div>
  <p>A command-line utility to assist in compiling Rust code to WASM modules compatible with MSFS 2020 and 2024</p>
</div>

## Installation

You can install the utility by running `cargo install --git https://github.com/navigraph/cargo-msfs`

## Usage

### Commands

The tool supports the following commands:

- **install** – Installs the SDK for a specified MSFS version.
- **remove** – Removes the SDK for a specified MSFS version.
- **update** – Updates the SDK for a specified MSFS version.
- **build** – Builds a crate for a specified MSFS version. (**note**: this runs `wasm-opt` automatically!)
- **info** – Gets information on installed SDKs.

### Supported MSFS Versions

The tool currently supports the following MSFS versions:

- `msfs2020` – Microsoft Flight Simulator 2020
- `msfs2024` – Microsoft Flight Simulator 2024

## Command Structure

```shell
cargo-msfs <COMMAND> [OPTIONS]
```

### Arguments

- `command` *(required)* – The command to run. Acceptable values: `install`, `remove`, `update`, `build`, `info`.
- `msfs_version` *(optional)* – Specifies the MSFS version for commands that require it (`install`, `remove`, `update`, `build`).
- `-i, --in-folder` *(optional)* – The path to the crate to build. Required only for the `build` command.
- `-o, --out-wasm` *(optional)* – The full path (including filename) to output the compiled WASM file. Required only for the `build` command.

## Examples

### Installing the SDK for MSFS 2020

```shell
cargo-msfs install --msfs-version msfs2020
```

### Removing the SDK for MSFS 2024

```shell
cargo-msfs remove --msfs-version msfs2024
```

### Updating the SDK for MSFS 2020

```shell
cargo-msfs update --msfs-version msfs2020
```

### Building a crate for MSFS 2024

```shell
cargo-msfs build --msfs-version msfs2024 -i /path/to/crate -o /path/to/output.wasm
```

### Getting information on installed SDKs

```shell
cargo-msfs info
```

## License

This project is licensed under the MIT License.
