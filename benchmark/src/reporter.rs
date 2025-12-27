// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Ankit Kumar Pandey

//! JSON report generation for benchmark results.
//!
//! Handles saving benchmark data to timestamped JSON files for later visualization.

use crate::metrics::BenchmarkReport;
use chrono::Utc;
use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Errors that can occur during report generation.
#[derive(Debug, Error)]
pub enum ReporterError {
    #[error("Failed to create output directory: {0}")]
    DirectoryCreation(#[from] std::io::Error),

    #[error("Failed to serialize report: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// JSON reporter for benchmark results.
pub struct JsonReporter {
    /// Output directory for benchmark data
    output_dir: PathBuf,
}

impl JsonReporter {
    /// Create a new JSON reporter with the specified output directory.
    pub fn new(output_dir: impl AsRef<Path>) -> Result<Self, ReporterError> {
        let output_dir = output_dir.as_ref().to_path_buf();
        fs::create_dir_all(&output_dir)?;
        Ok(Self { output_dir })
    }

    /// Create a reporter using the default data directory.
    pub fn default_location() -> Result<Self, ReporterError> {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let data_dir = Path::new(manifest_dir).join("data");
        Self::new(data_dir)
    }

    /// Save a benchmark report to a JSON file.
    ///
    /// Returns the path to the created file.
    pub fn save(&self, report: &BenchmarkReport) -> Result<PathBuf, ReporterError> {
        let timestamp = Utc::now().format("%Y-%m-%dT%H-%M-%SZ");
        let category = report
            .results
            .first()
            .map(|r| r.category.to_string())
            .unwrap_or_else(|| "mixed".to_string());

        let filename = format!("{}_{}.json", category, timestamp);
        let filepath = self.output_dir.join(&filename);

        let file = File::create(&filepath)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, report)?;

        Ok(filepath)
    }

    /// Save multiple reports, one per category.
    pub fn save_by_category(
        &self,
        report: &BenchmarkReport,
    ) -> Result<Vec<PathBuf>, ReporterError> {
        use std::collections::HashMap;

        // Group results by category
        let mut by_category: HashMap<_, Vec<_>> = HashMap::new();
        for result in &report.results {
            by_category
                .entry(result.category)
                .or_default()
                .push(result.clone());
        }

        let mut paths = Vec::new();
        for (category, results) in by_category {
            let mut category_report = BenchmarkReport::new();
            category_report.timestamp = report.timestamp;
            category_report.results = results;

            let timestamp = report.timestamp.format("%Y-%m-%dT%H-%M-%SZ");
            let filename = format!("{}_{}.json", category, timestamp);
            let filepath = self.output_dir.join(&filename);

            let file = File::create(&filepath)?;
            let writer = BufWriter::new(file);
            serde_json::to_writer_pretty(writer, &category_report)?;

            paths.push(filepath);
        }

        Ok(paths)
    }

    /// List all existing benchmark files in the output directory.
    pub fn list_reports(&self) -> Result<Vec<PathBuf>, ReporterError> {
        let mut reports = Vec::new();
        for entry in fs::read_dir(&self.output_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                reports.push(path);
            }
        }
        reports.sort();
        Ok(reports)
    }

    /// Load an existing benchmark report from a file.
    pub fn load(path: impl AsRef<Path>) -> Result<BenchmarkReport, ReporterError> {
        let file = File::open(path)?;
        let report = serde_json::from_reader(file)?;
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::{BenchmarkCategory, BenchmarkResult};
    use tempfile::TempDir;

    #[test]
    fn test_reporter_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let reporter = JsonReporter::new(temp_dir.path()).unwrap();

        let mut report = BenchmarkReport::new();
        report.add_result(BenchmarkResult::latency(
            "test",
            BenchmarkCategory::ColdStart,
            vec![100, 200, 300],
            false,
        ));

        let path = reporter.save(&report).unwrap();
        assert!(path.exists());

        let loaded = JsonReporter::load(&path).unwrap();
        assert_eq!(loaded.results.len(), 1);
        assert_eq!(loaded.results[0].name, "test");
    }

    #[test]
    fn test_list_reports() {
        let temp_dir = TempDir::new().unwrap();
        let reporter = JsonReporter::new(temp_dir.path()).unwrap();

        let mut report = BenchmarkReport::new();
        report.add_result(BenchmarkResult::latency(
            "test",
            BenchmarkCategory::ColdStart,
            vec![100],
            false,
        ));

        reporter.save(&report).unwrap();
        // Sleep to ensure different timestamp
        std::thread::sleep(std::time::Duration::from_millis(1100));
        reporter.save(&report).unwrap();

        let reports = reporter.list_reports().unwrap();
        // At least 1 report should exist (2 if timestamps differ)
        assert!(!reports.is_empty());
    }
}
