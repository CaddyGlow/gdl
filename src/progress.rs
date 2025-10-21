use std::path::Path;

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use log::info;

use crate::paths::format_path_for_log;

#[derive(Debug)]
pub struct DownloadProgress {
    pub total_files: usize,
    pub downloaded_files: usize,
    pub total_bytes: u64,
    pub downloaded_bytes: u64,
    file_bar: Option<ProgressBar>,
    byte_bar: Option<ProgressBar>,
}

impl DownloadProgress {
    /// Create a new progress tracker with visual progress bars
    pub fn new(total_files: usize, total_bytes: u64) -> Self {
        Self::with_multi_progress(total_files, total_bytes, None)
    }

    /// Create a new progress tracker with optional MultiProgress for coordinated display
    pub fn with_multi_progress(
        total_files: usize,
        total_bytes: u64,
        multi: Option<&MultiProgress>,
    ) -> Self {
        let (file_bar, byte_bar) = if let Some(mp) = multi {
            // Create file progress bar
            let file_pb = mp.add(ProgressBar::new(total_files as u64));
            file_pb.set_style(
                ProgressStyle::default_bar()
                    .template(
                        "{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} files ({percent}%)",
                    )
                    .expect("invalid progress bar template")
                    .progress_chars("#>-"),
            );

            // Create byte progress bar
            let byte_pb = mp.add(ProgressBar::new(total_bytes));
            byte_pb.set_style(
                ProgressStyle::default_bar()
                    .template(
                        "{spinner:.green} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({percent}%)",
                    )
                    .expect("invalid progress bar template")
                    .progress_chars("#>-"),
            );

            (Some(file_pb), Some(byte_pb))
        } else {
            (None, None)
        };

        Self {
            total_files,
            downloaded_files: 0,
            total_bytes,
            downloaded_bytes: 0,
            file_bar,
            byte_bar,
        }
    }

    pub fn log_start(&self, item_path: &str, target_path: &Path, size: Option<u64>) {
        let current = self.downloaded_files + 1;
        let total = self.total_files.max(current);
        let size_info = size
            .map(format_bytes)
            .unwrap_or_else(|| "size unknown".to_string());

        if self.file_bar.is_none() {
            info!(
                "Starting ({}/{}) {} -> {} [{}]",
                current,
                total,
                item_path,
                format_path_for_log(target_path),
                size_info
            );
        }
    }

    pub fn record_download(&mut self, item_path: &str, target_path: &Path, size: Option<u64>) {
        self.downloaded_files += 1;
        if let Some(bytes) = size {
            self.downloaded_bytes = self.downloaded_bytes.saturating_add(bytes);
        }

        // Update progress bars if they exist
        if let Some(ref bar) = self.file_bar {
            bar.set_position(self.downloaded_files as u64);
        }
        if let Some(ref bar) = self.byte_bar {
            bar.set_position(self.downloaded_bytes);
        }

        // Only log to console if no progress bars are being used
        if self.file_bar.is_none() {
            let total = self.total_files.max(self.downloaded_files);
            let size_info = match (size, self.total_bytes) {
                (Some(bytes), total_bytes) if total_bytes > 0 => format!(
                    "{} ({} / {})",
                    format_bytes(bytes),
                    format_bytes(self.downloaded_bytes),
                    format_bytes(total_bytes)
                ),
                (Some(bytes), _) => format_bytes(bytes),
                (None, total_bytes) if total_bytes > 0 => format!(
                    "{} / {}",
                    format_bytes(self.downloaded_bytes),
                    format_bytes(total_bytes)
                ),
                _ => "size unknown".to_string(),
            };
            info!(
                "({}/{}) {} -> {} [{}]",
                self.downloaded_files,
                total,
                item_path,
                format_path_for_log(target_path),
                size_info
            );
        }
    }

    /// Finish and clean up progress bars
    pub fn finish(&self) {
        if let Some(ref bar) = self.file_bar {
            bar.finish_and_clear();
        }
        if let Some(ref bar) = self.byte_bar {
            bar.finish_and_clear();
        }
    }
}

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if bytes == 0 {
        return "0 B".to_string();
    }

    let exp = (bytes as f64).log(1024.0).floor() as usize;
    let index = exp.min(UNITS.len() - 1);
    let value = bytes as f64 / 1024_f64.powi(index as i32);
    if index == 0 {
        format!("{} {}", bytes, UNITS[index])
    } else {
        format!("{:.1} {}", value, UNITS[index])
    }
}
