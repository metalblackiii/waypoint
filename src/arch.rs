use std::path::Path;

use crate::map;
use crate::map::index::ArchSummary;

/// Mirrors the drift threshold used by session-start's `should_rescan` predicate.
const FILE_COUNT_DRIFT_THRESHOLD: f64 = 0.03;

/// Render architecture summary in the same 2-line style used by session-start:
/// language distribution first, hotspots second when present.
#[must_use]
pub fn format_summary(summary: &ArchSummary) -> String {
    let mut output = summary.lang_dist.clone();
    if !summary.hotspots.trim().is_empty() {
        output.push('\n');
        output.push_str(&summary.hotspots);
    }
    output
}

/// Whether map metadata indicates stale architecture data.
///
/// Mirrors `session_start::should_rescan`: prefers mtime-based staleness when
/// stored mtimes exist, falls back to age + file-count drift for legacy maps.
#[must_use]
pub fn summary_is_stale(waypoint_dir: &Path, project_root: &Path) -> bool {
    let Some(header) = map::parse_map_header(waypoint_dir) else {
        return true;
    };

    // Prefer mtime-based staleness (mirrors session_start::should_rescan)
    if let Ok(stored_mtimes) = map::index::get_stored_mtimes(waypoint_dir)
        && !stored_mtimes.is_empty()
    {
        return crate::hook::session_start::has_mtime_drift(project_root, &stored_mtimes);
    }

    // Legacy fallback: age + file-count drift (for maps without mtime data)
    let age = chrono::Utc::now() - header.generated_at;
    if age.num_days() >= map::MAP_STALE_DAYS {
        return true;
    }
    let actual_count = map::scan::count_scannable_files(project_root);
    #[allow(clippy::cast_precision_loss)]
    let drift =
        (actual_count as f64 - header.file_count as f64).abs() / header.file_count.max(1) as f64;
    drift > FILE_COUNT_DRIFT_THRESHOLD
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchSummaryState {
    Missing,
    Stale,
}

/// Actionable message shown when arch summary is missing or stale.
#[must_use]
pub fn summary_guidance(project_root: &Path, state: ArchSummaryState) -> String {
    let scan_cmd = format!("waypoint scan \"{}\"", project_root.display());
    if state == ArchSummaryState::Stale {
        format!("Architecture summary is stale. Run `{scan_cmd}` to refresh.")
    } else {
        format!("Architecture summary not found. Run `{scan_cmd}` to generate it.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_summary_emits_two_lines_with_hotspots() {
        let summary = ArchSummary {
            lang_dist: "[waypoint] arch: Rust 80%, Markdown 20%".to_string(),
            hotspots: "[waypoint] arch: hotspots: src/ (12 imports-in)".to_string(),
            file_count: 12,
        };

        let output = format_summary(&summary);
        let mut lines = output.lines();
        assert_eq!(lines.next(), Some(summary.lang_dist.as_str()));
        assert_eq!(lines.next(), Some(summary.hotspots.as_str()));
        assert_eq!(lines.next(), None);
    }

    #[test]
    fn summary_guidance_recommends_waypoint_scan() {
        let message = summary_guidance(Path::new("/tmp/project"), ArchSummaryState::Missing);
        assert!(message.contains("waypoint scan"));
        assert!(message.contains("/tmp/project"));
    }
}
