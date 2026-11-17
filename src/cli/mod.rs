use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "iec61850",
    version,
    about = "SCL engineering and optional MMS/GOOSE simulation"
)]
pub struct Cli {
    /// Emit one versioned JSON result on stdout.
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List supported operations and validation limits.
    Capabilities,
    /// Check or run a virtual IED using the optional native runtime.
    Sim {
        #[command(subcommand)]
        command: SimCommand,
    },
    /// Browse, read, or write an MMS server.
    Mms {
        #[command(subcommand)]
        command: MmsCommand,
    },
    /// Observe GOOSE on an explicitly selected Ethernet interface.
    Goose {
        #[command(subcommand)]
        command: GooseCommand,
    },
    /// Inspect the native backend and available Ethernet interfaces.
    Runtime {
        #[command(flatten)]
        runtime: RuntimeOptions,
    },
    /// Inspect, validate, compare, or edit SCL documents.
    Scl {
        #[command(subcommand)]
        command: SclCommand,
    },
    /// Create an ICD from an explicit device specification.
    Icd {
        #[command(subcommand)]
        command: IcdCommand,
    },
}

#[derive(Debug, Args)]
pub struct RuntimeOptions {
    /// Native backend executable (built separately).
    #[arg(long, default_value = "iec61850-runtime")]
    pub runtime: PathBuf,
    /// Startup/request deadline, in seconds.
    #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u64).range(1..))]
    pub timeout: u64,
}

#[derive(Debug, Args)]
pub struct SimulationOptions {
    #[arg(long)]
    pub spec: PathBuf,
    /// Official libiec61850 genconfig.jar matching the runtime version.
    #[arg(long)]
    pub genconfig: PathBuf,
    #[arg(long, default_value = "java")]
    pub java: PathBuf,
    #[command(flatten)]
    pub backend: RuntimeOptions,
}

#[derive(Debug, Subcommand)]
pub enum SimCommand {
    /// Convert and check a model without opening network interfaces.
    Check {
        #[command(flatten)]
        options: SimulationOptions,
    },
    /// Run a bounded simulation; duration is expressed in seconds.
    Run {
        #[command(flatten)]
        options: SimulationOptions,
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
        duration: u32,
        /// Stream versioned JSONL events to a new file.
        #[arg(long)]
        events: Option<PathBuf>,
    },
}

#[derive(Debug, Args)]
pub struct MmsOptions {
    #[arg(long)]
    pub host: String,
    #[arg(long, default_value_t = 102, value_parser = clap::value_parser!(u16).range(1..))]
    pub port: u16,
    #[command(flatten)]
    pub backend: RuntimeOptions,
}

#[derive(Debug, Subcommand)]
pub enum MmsCommand {
    /// List logical devices and their logical nodes.
    Browse {
        #[command(flatten)]
        options: MmsOptions,
    },
    Read {
        #[command(flatten)]
        options: MmsOptions,
        #[arg(long)]
        reference: String,
        #[arg(long)]
        fc: String,
    },
    /// Write one scalar attribute using its remotely queried MMS type. No retries.
    Write {
        #[command(flatten)]
        options: MmsOptions,
        #[arg(long)]
        reference: String,
        #[arg(long)]
        fc: String,
        /// A JSON scalar, such as true or 25.5; use a file for strings/INT64.
        #[arg(
            long,
            required_unless_present = "value_file",
            conflicts_with = "value_file",
            allow_hyphen_values = true
        )]
        value: Option<String>,
        /// UTF-8 file containing one JSON scalar.
        #[arg(long, required_unless_present = "value")]
        value_file: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
pub enum GooseCommand {
    /// Receive the control block selected by a subscription JSON specification.
    Subscribe {
        #[arg(long)]
        spec: PathBuf,
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
        duration: u32,
        #[arg(long)]
        events: Option<PathBuf>,
        #[command(flatten)]
        backend: RuntimeOptions,
    },
}

#[derive(Debug, Subcommand)]
pub enum SclCommand {
    Inspect {
        input: PathBuf,
        #[arg(long)]
        ied: Option<String>,
    },
    Validate {
        input: PathBuf,
        #[command(flatten)]
        validation: ValidationOptions,
    },
    Diff {
        before: PathBuf,
        after: PathBuf,
    },
    Edit {
        input: PathBuf,
        #[arg(long)]
        patch: PathBuf,
        #[command(flatten)]
        output: OutputOptions,
        #[command(flatten)]
        validation: ValidationOptions,
    },
}

#[derive(Debug, Subcommand)]
pub enum IcdCommand {
    Create {
        #[arg(long)]
        spec: PathBuf,
        #[command(flatten)]
        output: OutputOptions,
        #[command(flatten)]
        validation: ValidationOptions,
    },
}

#[derive(Debug, Args)]
pub struct OutputOptions {
    /// Destination must not already exist; originals are never overwritten.
    #[arg(short, long, required_unless_present = "dry_run")]
    pub output: Option<PathBuf>,
    /// Prepare and validate without writing a file.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Args)]
pub struct ValidationOptions {
    /// Explicitly request XML and supported reference checks, excluding XSD.
    #[arg(long, conflicts_with = "xsd")]
    pub references_only: bool,
    /// Local root SCL schema; include its matching schema files alongside it.
    #[arg(long)]
    pub xsd: Option<PathBuf>,
    /// Path to the xmllint executable (only used with --xsd).
    #[arg(long, default_value = "xmllint")]
    pub xmllint: PathBuf,
    /// Maximum external XSD validation duration, in seconds.
    #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u64).range(1..=60))]
    pub xsd_timeout: u64,
}
