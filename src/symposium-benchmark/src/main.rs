//! Benchmark harness for testing rust-crate-sources-proxy research quality.
//!
//! Runs a research prompt through the proxy + Claude Code, then validates
//! the response against expected results using another Claude Code instance.

use anyhow::Result;
use clap::Parser;
use sacp::{ByteStreams, Component, DynComponent};
use sacp_conductor::conductor::Conductor;
use sacp_tokio::AcpAgent;
use std::path::PathBuf;
use std::str::FromStr;
use tokio::io::duplex;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

#[derive(Parser, Debug)]
#[command(name = "symposium-benchmark")]
#[command(about = "Benchmark harness for rust-crate-sources-proxy")]
struct Args {
    /// Benchmark to run (serde_from_value, etc.)
    #[arg(short, long)]
    benchmark: Option<String>,

    /// Directory to save raw output files
    #[arg(short, long, default_value = "benchmark-output")]
    output_dir: PathBuf,

    /// List available benchmarks
    #[arg(short, long)]
    list: bool,
}

struct Benchmark {
    name: &'static str,
    prompt: &'static str,
    expected: &'static str,
}

const BENCHMARKS: &[Benchmark] = &[Benchmark {
    name: "serde_from_value",
    prompt: "Please use the rust_crate_query tool to research the signature of the \
                 serde_json::from_value API and describe what inputs it accepts",
    expected: "The response should describe that serde_json::from_value takes a \
                   serde_json::Value and deserializes it into a type T. It should mention \
                   that it returns a Result<T, Error>.",
}];

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    // List benchmarks if requested
    if args.list {
        println!("Available benchmarks:");
        for benchmark in BENCHMARKS {
            println!("  - {}", benchmark.name);
        }
        return Ok(());
    }

    // Determine which benchmarks to run
    let benchmarks_to_run: Vec<&Benchmark> = if let Some(name) = &args.benchmark {
        BENCHMARKS.iter().filter(|b| b.name == name).collect()
    } else {
        BENCHMARKS.iter().collect()
    };

    if benchmarks_to_run.is_empty() {
        anyhow::bail!(
            "Benchmark '{}' not found. Use --list to see available benchmarks.",
            args.benchmark.unwrap()
        );
    }

    // Create output directory
    std::fs::create_dir_all(&args.output_dir)?;

    // Run benchmarks
    for benchmark in benchmarks_to_run {
        tracing::info!("Running benchmark: {}", benchmark.name);
        run_benchmark(benchmark, &args.output_dir).await?;
    }

    Ok(())
}

async fn run_benchmark(benchmark: &Benchmark, output_dir: &PathBuf) -> Result<()> {
    let research_prompt = benchmark.prompt;
    let expected_result = benchmark.expected;

    // Create components: rust-crate-sources-proxy + Claude Code
    let proxy = symposium_crate_sources_proxy::CrateSourcesProxy;
    let claude_agent = AcpAgent::from_str("npx -y '@zed-industries/claude-code-acp'")?;

    // Create duplex streams for editor <-> conductor communication
    let (editor_write, conductor_read) = duplex(8192);
    let (conductor_write, editor_read) = duplex(8192);

    // Spawn conductor with proxy + agent chain
    let conductor_handle = tokio::spawn(async move {
        Conductor::new(
            "benchmark-conductor".to_string(),
            vec![DynComponent::new(proxy), DynComponent::new(claude_agent)],
            None,
        )
        .run(ByteStreams::new(
            conductor_write.compat_write(),
            conductor_read.compat(),
        ))
        .await
    });

    // Send prompt using yopo
    let response = yopo::prompt(
        ByteStreams::new(editor_write.compat_write(), editor_read.compat()),
        research_prompt,
    )
    .await?;

    tracing::info!("Research response received: {} chars", response.len());

    // Validate response using another Claude Code instance
    tracing::info!("Validating response");

    let validator_agent = AcpAgent::from_str("npx -y '@zed-industries/claude-code-acp'")?;
    let (validator_write, validator_read) = duplex(8192);
    let (validator_out_write, validator_out_read) = duplex(8192);

    let validator_handle = tokio::spawn(async move {
        validator_agent
            .serve(ByteStreams::new(
                validator_out_write.compat_write(),
                validator_read.compat(),
            ))
            .await
    });

    let validation_prompt = format!(
        "Compare this response to the expected result and respond with PASS or FAIL. \
         If FAIL, explain what's missing.\n\n\
         Expected: {}\n\n\
         Actual response:\n{}",
        expected_result, response
    );

    let validation_result = yopo::prompt(
        ByteStreams::new(validator_write.compat_write(), validator_out_read.compat()),
        &validation_prompt,
    )
    .await?;

    // Save outputs to files
    let prompt_file = output_dir.join(format!("{}_prompt.txt", benchmark.name));
    let response_file = output_dir.join(format!("{}_response.txt", benchmark.name));
    let validation_file = output_dir.join(format!("{}_validation.txt", benchmark.name));
    let expected_file = output_dir.join(format!("{}_expected.txt", benchmark.name));

    std::fs::write(&prompt_file, research_prompt)?;
    std::fs::write(&response_file, &response)?;
    std::fs::write(&validation_file, &validation_result)?;
    std::fs::write(&expected_file, expected_result)?;

    tracing::info!("Output saved to:");
    tracing::info!("  Prompt: {}", prompt_file.display());
    tracing::info!("  Response: {}", response_file.display());
    tracing::info!("  Expected: {}", expected_file.display());
    tracing::info!("  Validation: {}", validation_file.display());

    println!("\n=== BENCHMARK: {} ===", benchmark.name);
    println!("VALIDATION RESULT:\n{}", validation_result);
    println!("========================\n");

    // Clean up
    validator_handle.await??;
    conductor_handle.await??;

    Ok(())
}
