use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "iec61850",
    version,
    about = "Offline SCL inspection, generation, controlled editing, and validation"
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
