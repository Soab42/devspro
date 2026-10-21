//! `nvidia-smi` is the one GPU source that is genuinely the same across platforms —
//! same binary name, same query flags, same CSV out — so both the Linux and Windows
//! backends try it before falling back to their own OS-specific enumeration (which
//! can name the adapter but not measure it). Returns `None` when the tool isn't
//! installed or fails, so callers can tell "no NVIDIA tooling here" apart from
//! "the tool ran and reported no GPUs".

use std::process::Command;
use std::sync::OnceLock;

use super::super::{GpuInfo, TemperatureReading};

/// `nvidia-smi` is absent on most machines; remember that rather than failing to
/// spawn it on every poll.
static AVAILABLE: OnceLock<bool> = OnceLock::new();

pub fn query() -> Option<Vec<GpuInfo>> {
    let available = *AVAILABLE
        .get_or_init(|| Command::new("nvidia-smi").arg("-L").output().is_ok_and(|o| o.status.success()));
    if !available {
        return None;
    }
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,driver_version,utilization.gpu,memory.used,memory.total,temperature.gpu",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Some(parse(&text))
}

fn parse(text: &str) -> Vec<GpuInfo> {
    text.lines()
        .filter_map(|line| {
            let cols: Vec<&str> = line.split(',').map(str::trim).collect();
            if cols.len() < 6 {
                return None;
            }
            Some(GpuInfo {
                name: cols[0].to_string(),
                driver: Some(cols[1].to_string()),
                usage_percent: cols[2].parse().ok(),
                memory_used_mb: cols[3].parse().ok(),
                memory_total_mb: cols[4].parse().ok(),
                // nvidia-smi's CSV query has no column for the slowdown or
                // shutdown thresholds, so the frontend falls back to its own GPU
                // defaults for those.
                temperature: cols[5].parse().ok().map(|celsius| TemperatureReading {
                    label: "GPU".to_string(),
                    celsius,
                    high_c: None,
                    critical_c: None,
                }),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_csv_row_per_gpu() {
        let gpus = parse("NVIDIA GeForce RTX 4060, 560.94, 12, 1024, 8188, 47\n");
        assert_eq!(gpus.len(), 1);
        assert_eq!(gpus[0].name, "NVIDIA GeForce RTX 4060");
        assert_eq!(gpus[0].driver.as_deref(), Some("560.94"));
        assert_eq!(gpus[0].usage_percent, Some(12.0));
        assert_eq!(gpus[0].memory_used_mb, Some(1024.0));
        assert_eq!(gpus[0].memory_total_mb, Some(8188.0));
        assert_eq!(gpus[0].temperature.as_ref().map(|t| t.celsius), Some(47.0));
    }

    // `nounits` still prints "[N/A]" for fields a given card doesn't report, so
    // every measurement is parsed optionally rather than defaulted to zero — a
    // faked 0% would read as an idle GPU instead of an unmeasurable one.
    #[test]
    fn keeps_unreported_measurements_as_none() {
        let gpus = parse("Quadro K420, 470.256.02, [N/A], [N/A], 2048, [N/A]\n");
        assert_eq!(gpus.len(), 1);
        assert_eq!(gpus[0].usage_percent, None);
        assert_eq!(gpus[0].memory_used_mb, None);
        assert_eq!(gpus[0].memory_total_mb, Some(2048.0));
        assert!(gpus[0].temperature.is_none());
    }

    #[test]
    fn skips_short_rows_and_trailing_blank_lines() {
        assert!(parse("\n").is_empty());
        assert!(parse("only, three, columns\n").is_empty());
    }
}
