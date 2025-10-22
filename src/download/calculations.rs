//! Pure calculation functions for download operations.
//!
//! This module contains testable logic for calculating progress, determining resume
//! capability, and computing optimal chunk sizes.

/// Calculates the download progress percentage if total size is known.
///
/// Returns None if total size is unknown (total = None).
///
/// # Examples
///
/// ```
/// use gdl::download::calculate_progress_percentage;
///
/// assert_eq!(calculate_progress_percentage(50, Some(100)), Some(50.0));
/// assert_eq!(calculate_progress_percentage(0, Some(100)), Some(0.0));
/// assert_eq!(calculate_progress_percentage(100, Some(100)), Some(100.0));
/// assert_eq!(calculate_progress_percentage(50, None), None);
/// ```
pub fn calculate_progress_percentage(downloaded: u64, total: Option<u64>) -> Option<f64> {
    total.map(|t| {
        if t == 0 {
            100.0 // Avoid division by zero, treat as complete
        } else {
            (downloaded as f64 / t as f64) * 100.0
        }
    })
}

/// Determines if a download can be resumed from a partial state.
///
/// A download can be resumed if:
/// - The server supports range requests
/// - There's a partial file with non-zero size
/// - The partial size is less than the expected total size
///
/// # Examples
///
/// ```
/// use gdl::download::can_resume_download;
///
/// assert!(can_resume_download(1024, Some(2048), true));
/// assert!(!can_resume_download(1024, Some(2048), false)); // No range support
/// assert!(!can_resume_download(0, Some(2048), true));     // No partial data
/// assert!(!can_resume_download(2048, Some(2048), true));  // Already complete
/// assert!(!can_resume_download(3000, Some(2048), true));  // Partial larger than expected
/// ```
pub fn can_resume_download(
    partial_size: u64,
    expected_size: Option<u64>,
    supports_ranges: bool,
) -> bool {
    supports_ranges && partial_size > 0 && expected_size.map_or(false, |s| partial_size < s)
}

/// Determines if a partial download should be replaced (deleted and restarted).
///
/// A partial download should be replaced if:
/// - We know the expected size AND
/// - The partial file is >= expected size (complete or corrupted)
pub fn should_replace_partial(partial_size: u64, expected_size: Option<u64>) -> bool {
    expected_size.map_or(false, |expected| partial_size >= expected)
}

/// Calculates the optimal chunk size for downloading based on file size.
///
/// Chunk size strategy:
/// - Files < 1MB: 8KB chunks (fast for small files)
/// - Files < 10MB: 16KB chunks (balanced)
/// - Files >= 10MB or unknown size: 32KB chunks (efficient for large files)
///
/// # Examples
///
/// ```
/// use gdl::download::calculate_chunk_size;
///
/// assert_eq!(calculate_chunk_size(Some(512 * 1024)), 8192);     // 512KB -> 8KB chunks
/// assert_eq!(calculate_chunk_size(Some(5 * 1024 * 1024)), 16384); // 5MB -> 16KB chunks
/// assert_eq!(calculate_chunk_size(Some(50 * 1024 * 1024)), 32768); // 50MB -> 32KB chunks
/// assert_eq!(calculate_chunk_size(None), 32768);                 // Unknown -> 32KB chunks
/// ```
pub fn calculate_chunk_size(file_size: Option<u64>) -> usize {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;

    match file_size {
        Some(size) if size < MB => 8192,       // < 1MB: 8KB chunks
        Some(size) if size < 10 * MB => 16384, // < 10MB: 16KB chunks
        _ => 32768,                            // >= 10MB or unknown: 32KB chunks
    }
}

/// Calculates the estimated time remaining for a download.
///
/// Returns None if download rate is zero or time would overflow.
///
/// # Arguments
///
/// * `remaining_bytes` - Number of bytes left to download
/// * `bytes_per_second` - Current download speed in bytes per second
///
/// # Examples
///
/// ```
/// use gdl::download::calculate_time_remaining_secs;
///
/// assert_eq!(calculate_time_remaining_secs(1000, 100), Some(10));
/// assert_eq!(calculate_time_remaining_secs(1000, 0), None);
/// assert_eq!(calculate_time_remaining_secs(0, 100), Some(0));
/// ```
pub fn calculate_time_remaining_secs(remaining_bytes: u64, bytes_per_second: u64) -> Option<u64> {
    if bytes_per_second == 0 {
        return None;
    }
    Some(remaining_bytes / bytes_per_second)
}

/// Calculates the download speed in bytes per second.
///
/// Returns None if duration is zero (to avoid division by zero).
///
/// # Arguments
///
/// * `bytes_downloaded` - Number of bytes downloaded
/// * `duration_secs` - Time elapsed in seconds
pub fn calculate_download_speed(bytes_downloaded: u64, duration_secs: f64) -> Option<f64> {
    if duration_secs <= 0.0 {
        return None;
    }
    Some(bytes_downloaded as f64 / duration_secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_progress_percentage() {
        // Happy path
        assert_eq!(calculate_progress_percentage(50, Some(100)), Some(50.0));
        assert_eq!(calculate_progress_percentage(100, Some(100)), Some(100.0));

        // Edge cases
        assert_eq!(calculate_progress_percentage(50, None), None);
        assert_eq!(calculate_progress_percentage(0, Some(0)), Some(100.0));
    }

    #[test]
    fn test_can_resume_download() {
        // Can resume
        assert!(can_resume_download(1024, Some(2048), true));

        // Cannot resume - various reasons
        assert!(!can_resume_download(1024, Some(2048), false)); // No range support
        assert!(!can_resume_download(0, Some(2048), true)); // No partial data
        assert!(!can_resume_download(2048, Some(2048), true)); // Already complete
        assert!(!can_resume_download(1024, None, true)); // Unknown size
    }

    #[test]
    fn test_should_replace_partial() {
        assert!(should_replace_partial(2048, Some(2048))); // Complete
        assert!(should_replace_partial(3000, Some(2048))); // Too large
        assert!(!should_replace_partial(1024, Some(2048))); // Incomplete
        assert!(!should_replace_partial(1024, None)); // Unknown
    }

    #[test]
    fn test_calculate_chunk_size() {
        assert_eq!(calculate_chunk_size(Some(512 * 1024)), 8192); // < 1MB
        assert_eq!(calculate_chunk_size(Some(5 * 1024 * 1024)), 16384); // < 10MB
        assert_eq!(calculate_chunk_size(Some(50 * 1024 * 1024)), 32768); // >= 10MB
        assert_eq!(calculate_chunk_size(None), 32768); // Unknown

        // Boundaries
        assert_eq!(calculate_chunk_size(Some(1024 * 1024)), 16384);
        assert_eq!(calculate_chunk_size(Some(10 * 1024 * 1024)), 32768);
    }

    #[test]
    fn test_calculate_time_remaining_secs() {
        assert_eq!(calculate_time_remaining_secs(1000, 100), Some(10));
        assert_eq!(calculate_time_remaining_secs(1000, 0), None); // Zero speed
        assert_eq!(calculate_time_remaining_secs(0, 100), Some(0)); // Complete
    }

    #[test]
    fn test_calculate_download_speed() {
        assert_eq!(calculate_download_speed(1000, 10.0), Some(100.0));
        assert_eq!(calculate_download_speed(1000, 0.0), None); // Zero duration
        assert_eq!(calculate_download_speed(0, 10.0), Some(0.0)); // Zero bytes
    }
}
