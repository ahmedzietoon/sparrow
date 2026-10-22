use crate::EPOCH;
use crate::config::{ShrinkDecayStrategy, SparrowConfig};
use crate::consts::DEFAULT_EXPLORE_TIME_RATIO;
use crate::optimizer::StopReason;
use anyhow::{Context, Result};
use clap::Parser;
use jagua_rs::probs::spp::io::ext_repr::{ExtSPInstance, ExtSPSolution};
use log::{log, warn, Level, LevelFilter};
use serde::{Deserialize, Serialize};
use std::fs;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::num::NonZeroUsize;
use std::path::Path;
use svg::Document;

fn parse_decay(s: &str) -> Result<f32, String> {
    let r: f32 = s.parse().map_err(|e| format!("not a number: {e}"))?;
    if r > 0.0 && r < 1.0 {
        Ok(r)
    } else {
        Err(format!("must be strictly between 0 and 1, got {r}"))
    }
}

#[derive(Parser)]
pub struct MainCli {
    /// Path to input file (mandatory)
    #[arg(short = 'i', long, help = "Path to the input JSON file, or a solution JSON file for warm starting")]
    pub input: String,

    /// Global time limit in seconds (mutually exclusive with -e and -c)
    #[arg(short = 't', long, conflicts_with_all = &["exploration", "compression"], help = "Set a global time limit (in seconds)")]
    pub global_time: Option<u64>,

    /// Exploration time limit in seconds (requires compression time)
    #[arg(short = 'e', long, requires = "compression", help = "Set the exploration phase time limit (in seconds)")]
    pub exploration: Option<u64>,

    /// Compression time limit in seconds (requires exploration time)
    #[arg(short = 'c', long, requires = "exploration", help = "Set the compression phase time limit (in seconds)")]
    pub compression: Option<u64>,

    /// Enable early and automatic termination
    #[arg(short = 'x', long, help = "Enable early termination of the optimization process")]
    pub early_termination: bool,

    #[arg(short = 's', long, help = "Fixed seed for the random number generator")]
    pub rng_seed: Option<u64>,

    /// Number of worker threads used by the separator
    #[arg(long)]
    pub workers: Option<NonZeroUsize>,

    /// Deterministic work budget: total separator calls, split 80/20 between exploration and compression like -t
    #[arg(long, value_name = "N", help = "Stop after N separator calls (80% exploration, 20% compression); -t then acts only as a safety cap")]
    pub max_separations: Option<NonZeroUsize>,

    /// Consecutive failed exploration attempts that end exploration (default 10 with -x)
    #[arg(long, value_name = "N", help = "Stop exploration after N consecutive failed attempts (default 10 with -x)")]
    pub max_fails: Option<NonZeroUsize>,

    /// Compression shrink decay per failed attempt, strictly between 0 and 1 (default 0.9 with -x)
    #[arg(long, value_name = "R", value_parser = parse_decay, help = "Compression shrink decay per failed attempt, 0 < R < 1 (default 0.9 with -x)")]
    pub cmpr_decay: Option<f32>,

    /// Write the best feasible solution so far to this file on every improvement
    #[arg(long, value_name = "PATH", help = "Write the best feasible solution so far (same JSON as the final output) on every improvement")]
    pub snapshot: Option<String>,

    /// Do not write intermediate solution SVGs (the final SVG is still written)
    #[arg(long, help = "Do not write intermediate solution SVGs")]
    pub no_intermediate_svg: bool,
}

impl MainCli {
    /// Applies optional CLI values that override the default optimizer configuration.
    pub fn apply_config_overrides(&self, config: &mut SparrowConfig) {
        if let Some(rng_seed) = self.rng_seed {
            warn!("[MAIN] overriding RNG seed: {rng_seed}");
            config.rng_seed = Some(rng_seed as usize);
        }
        if let Some(workers) = self.workers {
            warn!("[MAIN] overriding separator worker count: {workers}");
            config.expl_cfg.separator_config.n_workers = workers.get();
            config.cmpr_cfg.separator_config.n_workers = workers.get();
        }
        if let Some(max_fails) = self.max_fails {
            config.expl_cfg.max_conseq_failed_attempts = Some(max_fails.get());
        }
        if let Some(cmpr_decay) = self.cmpr_decay {
            config.cmpr_cfg.shrink_decay = ShrinkDecayStrategy::FailureBased(cmpr_decay);
        }
        if let Some(max_separations) = self.max_separations {
            // Same 80/20 split as -t: exploration takes the first 80% of the calls, compression the rest
            let expl = (max_separations.get() as f32 * DEFAULT_EXPLORE_TIME_RATIO) as usize;
            let cmpr = max_separations.get() - expl;
            warn!("[MAIN] separator call budget: {max_separations} (exploration {expl}, compression {cmpr})");
            config.expl_cfg.max_separations = Some(expl);
            config.cmpr_cfg.max_separations = Some(cmpr);
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ExtSPOutput {
    #[serde(flatten)]
    pub instance: ExtSPInstance,
    pub solution: ExtSPSolution,
    /// Why the search stopped; `in_progress` in snapshots. Absent in files written before this field existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<StopReason>,
}

pub fn init_logger(level_filter: LevelFilter, log_file_path: &Path) -> Result<()> {
    //remove old log file
    let _ = fs::remove_file(log_file_path);
    fern::Dispatch::new()
        // Perform allocation-free log formatting
        .format(|out, message, record| {
            let handle = std::thread::current();
            let thread_name = handle.name().unwrap_or("-");

            let duration = EPOCH.elapsed();
            let sec = duration.as_secs() % 60;
            let min = (duration.as_secs() / 60) % 60;
            let hours = (duration.as_secs() / 60) / 60;

            let prefix = format!(
                "[{}] [{:0>2}:{:0>2}:{:0>2}] <{}>",
                record.level(),
                hours,
                min,
                sec,
                thread_name,
            );

            out.finish(format_args!("{:<25}{}", prefix, message))
        })
        // Add blanket level filter -
        .level(level_filter)
        .chain(std::io::stdout())
        .chain(fern::log_file(log_file_path)?)
        .apply()?;
    log!(
        Level::Info,
        "[EPOCH]: {}",
        jiff::Timestamp::now()
    );
    Ok(())
}


pub fn write_svg(document: &Document, path: &Path, log_lvl: Level) -> Result<()> {
    //make sure the parent directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("could not create parent directory for svg file")?;
    }
    svg::save(path, document)?;
    log!(log_lvl,
        "[IO] svg exported to file://{}",
        fs::canonicalize(path)
            .expect("could not canonicalize path")
            .to_str()
            .unwrap()
    );
    Ok(())
}

pub fn write_json(json: &impl Serialize, path: &Path, log_lvl: Level) -> Result<()> {
    let file = File::create(path)?;
    serde_json::to_writer_pretty(file, json)?;
    log!(log_lvl,
        "[IO] json exported to file://{}",
        fs::canonicalize(path)
            .expect("could not canonicalize path")
            .to_str()
            .unwrap()
    );
    Ok(())
}

/// Writes JSON to `<path>.tmp`, flushes it to disk, then renames it over `path`.
/// A reader or a killed process sees either the old file or the new one, never a partial one.
pub fn write_json_atomic(json: &impl Serialize, path: &Path) -> Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let mut writer = BufWriter::new(File::create(&tmp)?);
    serde_json::to_writer_pretty(&mut writer, json)?;
    writer.into_inner().map_err(|e| e.into_error())?.sync_all()?;
    fs::rename(&tmp, path)?;
    Ok(())
}

pub fn read_spp_input(path: &Path) -> Result<(ExtSPInstance, Option<ExtSPSolution>)> {
    let input_str = fs::read_to_string(path).context("could not read input file")?;
    //try parsing a full output (instance + solution)
    match serde_json::from_str::<ExtSPOutput>(&input_str) {
        Ok(ext_output) => {
            Ok((ext_output.instance, Some(ext_output.solution)))
        }
        Err(_) => {
            //try parsing just the instance
            let ext_instance = serde_json::from_str::<ExtSPInstance>(&input_str)
                .context("could not parse instance from input file")?;
            Ok((ext_instance, None))
        }
    }
}
