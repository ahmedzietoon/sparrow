use crate::optimizer::StopReason;
use crate::util::io::{self, ExtSPOutput};
use crate::util::listener::{ReportType, SolutionListener};
use crate::EPOCH;
use jagua_rs::probs::spp::entities::SPSolution;
use jagua_rs::probs::spp::io::ext_repr::ExtSPInstance;
use log::warn;
use std::path::Path;

/// Writes the best feasible solution so far to `path` on every improvement (`--snapshot`).
/// Exploration reports only strictly narrower feasible strips and compression only successful shrinks,
/// so every `ExplFeas` and `CmprFeas` report is a new best.
pub struct SnapshotExporter {
    path: Option<String>,
    instance: ExtSPInstance,
}

impl SnapshotExporter {
    pub fn new(path: Option<String>, instance: ExtSPInstance) -> Self {
        Self { path, instance }
    }
}

impl SolutionListener for SnapshotExporter {
    fn report(&mut self, report_type: ReportType, solution: &SPSolution) {
        let Some(path) = &self.path else { return };
        if !matches!(report_type, ReportType::ExplFeas | ReportType::CmprFeas) {
            return;
        }
        let output = ExtSPOutput {
            instance: self.instance.clone(),
            solution: jagua_rs::probs::spp::io::export(solution, *EPOCH),
            stop_reason: Some(StopReason::InProgress),
        };
        // A failed write keeps the previous snapshot, which is still valid. The next improvement tries again.
        if let Err(e) = io::write_json_atomic(&output, Path::new(path)) {
            warn!("[SNAP] snapshot not written to {path}: {e:#}");
        }
    }
}
