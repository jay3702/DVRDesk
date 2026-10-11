//! Renders a recording file's DVR record (`GET /dvr/files/{id}`) as a
//! readable page for the Details dialog — the native counterpart of the
//! Channels DVR admin page's "View Details", which shows the same record as
//! a raw JSON tree.
//!
//! Read straight from `serde_json::Value` rather than a typed struct: the
//! record's shape varies (an empty or missing file has no `FileSize` or
//! `SignalStats`), and it's display-only, so a missing field is just
//! skipped. The whole record stays available under "Raw JSON".

use chrono::{Local, TimeZone};
use serde_json::Value;

/// What the Details / Debug Log dialog shows once loaded.
pub enum FileInfoBody {
    /// The parsed record, plus the same pretty-printed for "Raw JSON" and
    /// Copy — formatted once on arrival, not every frame.
    Details {
        record: Value,
        raw: String,
    },
    Log(String),
}

/// Signal and buffer stats, in display order, with how each is formatted.
const STATS: [(&str, &str, StatUnit); 12] = [
    ("SignalStats", "SS", StatUnit::Percent),
    ("SignalStats", "SNQ", StatUnit::Percent),
    ("SignalStats", "SEQ", StatUnit::Percent),
    ("SignalStats", "BPS", StatUnit::Bitrate),
    ("SignalStats", "PPS", StatUnit::Count),
    ("SignalStats", "TSERR", StatUnit::Count),
    ("SignalStats", "NETERR", StatUnit::Count),
    ("SignalStats", "DVRBUF", StatUnit::Count),
    ("SignalStats", "DVRDROP", StatUnit::Count),
    ("SignalStats", "STREAMTIMEOUT", StatUnit::Count),
    ("BufferStats", "BufferPct", StatUnit::Percent),
    ("BufferStats", "BufferDrop", StatUnit::Count),
];

#[derive(Clone, Copy)]
enum StatUnit {
    Percent,
    Bitrate,
    Count,
}

pub fn show(ui: &mut egui::Ui, record: &Value, raw: &str) {
    let airing = &record["Airing"];

    section(ui, "Recording", true, |ui| {
        grid(ui, "details_recording", |ui| {
            row(ui, "Status", status(record));
            row(ui, "Path", text(&record["Path"]));
            row(ui, "Channel", text(&record["ChannelNumber"]));
            row(
                ui,
                "File size",
                record["FileSize"]
                    .as_f64()
                    .map(fmt_bytes)
                    .or_else(|| Some("none (no file data)".to_string())),
            );
            row(ui, "Length", record["Duration"].as_f64().map(fmt_hms));
            row(ui, "Scheduled", scheduled(airing));
            row(
                ui,
                "Recorded",
                record["JobTime"].as_f64().map(|t| {
                    let mut s = fmt_time(t);
                    if let Some(d) = record["JobDuration"].as_f64() {
                        s.push_str(&format!(" for {}", fmt_hms(d)));
                    }
                    s
                }),
            );
            row(ui, "Created", record["CreatedAt"].as_f64().map(fmt_time));
            row(ui, "Updated", record["UpdatedAt"].as_f64().map(fmt_time));
            row(
                ui,
                "Playback position",
                record["PlaybackTime"].as_f64().map(fmt_hms),
            );
            row(ui, "Last played", record["PlayedAt"].as_f64().map(fmt_time));
            row(
                ui,
                "Commercial detection",
                text(&record["CommercialDetectSource"]),
            );
            row(ui, "File ID", text(&record["ID"]));
            row(ui, "Job ID", text(&record["JobID"]));
            row(ui, "Rule ID", text(&record["RuleID"]));
            row(ui, "Group ID", text(&record["GroupID"]));
            row(ui, "Device ID", text(&record["DeviceID"]));
            row(ui, "Version", text(&record["Version"]));
            row(ui, "Checksum", text(&record["Checksum"]));
        });
    });

    if airing.is_object() {
        section(ui, "Program", true, |ui| {
            grid(ui, "details_program", |ui| {
                row(ui, "Title", text(&airing["Title"]));
                let episode = match (
                    airing["SeasonNumber"].as_u64(),
                    airing["EpisodeNumber"].as_u64(),
                ) {
                    (Some(s), Some(e)) => Some(format!("S{s}E{e}")),
                    _ => None,
                };
                let episode = match (episode, text(&airing["EpisodeTitle"])) {
                    (Some(n), Some(t)) => Some(format!("{n} {t}")),
                    (n, t) => n.or(t),
                };
                row(ui, "Episode", episode);
                row(ui, "Original air date", text(&airing["OriginalDate"]));
                row(ui, "Release year", text(&airing["ReleaseYear"]));
                row(ui, "Rating", text(&airing["ContentRating"]));
                row(ui, "Categories", list(&airing["Categories"]));
                row(ui, "Genres", list(&airing["Genres"]));
                row(ui, "Tags", list(&airing["Tags"]));
                row(ui, "Cast", list(&airing["Cast"]));
                row(ui, "Guide source", text(&airing["Source"]));
                row(ui, "Series ID", text(&airing["SeriesID"]));
                row(ui, "Program ID", text(&airing["ProgramID"]));
            });
            if let Some(summary) = text(&airing["FullSummary"]).or(text(&airing["Summary"])) {
                ui.add_space(6.0);
                ui.add(egui::Label::new(summary).wrap());
            }
        });
    }

    let has_stats = STATS
        .iter()
        .any(|(group, key, _)| record[group][key].is_object());
    if has_stats {
        section(ui, "Signal and buffer quality", true, |ui| {
            egui::Grid::new("details_stats")
                .striped(true)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    for heading in [
                        "", "Initial", "Last", "Min", "Max", "Average", "Good", "Bad",
                    ] {
                        ui.strong(heading);
                    }
                    ui.end_row();
                    for (group, key, unit) in STATS {
                        let stat = &record[group][key];
                        if !stat.is_object() {
                            continue;
                        }
                        ui.label(key);
                        for field in ["Initial", "Last", "Min", "Max"] {
                            ui.label(fmt_stat(stat[field].as_f64(), unit));
                        }
                        // Sum over the samples taken, so Sum / GoodCount.
                        let average = match (stat["Sum"].as_f64(), stat["GoodCount"].as_f64()) {
                            (Some(sum), Some(n)) if n > 0.0 => Some(sum / n),
                            _ => None,
                        };
                        ui.label(fmt_stat(average, unit));
                        ui.label(fmt_stat(stat["GoodCount"].as_f64(), StatUnit::Count));
                        bad_count(ui, stat["BadCount"].as_f64());
                        ui.end_row();
                    }
                    for key in ["Grade", "NetworkGrade", "SignalGrade", "StreamGrade"] {
                        let grade = &record["SignalStats"][key];
                        if !grade.is_object() {
                            continue;
                        }
                        ui.label(key);
                        for _ in 0..5 {
                            ui.label("");
                        }
                        ui.label(fmt_stat(grade["GoodCount"].as_f64(), StatUnit::Count));
                        bad_count(ui, grade["BadCount"].as_f64());
                        ui.end_row();
                    }
                });
        });
    }

    if let Some(regions) = record["MediaRegions"].as_array().filter(|r| !r.is_empty()) {
        section(
            ui,
            &format!("Commercials ({})", regions.len()),
            false,
            |ui| {
                egui::Grid::new("details_regions")
                    .striped(true)
                    .spacing([16.0, 4.0])
                    .show(ui, |ui| {
                        for heading in ["Start", "End", "Length", "Type", "Detected by"] {
                            ui.strong(heading);
                        }
                        ui.end_row();
                        for region in regions {
                            let start = region["Start"].as_f64();
                            let end = region["End"].as_f64();
                            ui.label(start.map(fmt_hms).unwrap_or_default());
                            ui.label(end.map(fmt_hms).unwrap_or_default());
                            ui.label(match (start, end) {
                                (Some(s), Some(e)) => fmt_hms(e - s),
                                _ => String::new(),
                            });
                            ui.label(text(&region["Type"]).unwrap_or_default());
                            ui.label(list(&region["Sources"]).unwrap_or_default());
                            ui.end_row();
                        }
                    });
            },
        );
    }

    section(ui, "Raw JSON", false, |ui| {
        // Read-only (`&str`) but still selectable.
        let mut raw = raw;
        ui.add(
            egui::TextEdit::multiline(&mut raw)
                .font(egui::TextStyle::Monospace)
                .desired_width(f32::INFINITY),
        );
    });
}

fn section(ui: &mut egui::Ui, title: &str, open: bool, body: impl FnOnce(&mut egui::Ui)) {
    egui::CollapsingHeader::new(egui::RichText::new(title).strong())
        .id_salt(("details_section", title.split(' ').next()))
        .default_open(open)
        .show(ui, body);
}

fn grid(ui: &mut egui::Ui, id: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::Grid::new(id)
        .num_columns(2)
        .striped(true)
        .spacing([16.0, 4.0])
        .show(ui, body);
}

/// A label/value row, skipped entirely when the record has no value.
fn row(ui: &mut egui::Ui, label: &str, value: Option<String>) {
    let Some(value) = value else {
        return;
    };
    ui.label(egui::RichText::new(label).weak());
    ui.add(egui::Label::new(value).selectable(true));
    ui.end_row();
}

fn bad_count(ui: &mut egui::Ui, count: Option<f64>) {
    let label = fmt_stat(count, StatUnit::Count);
    if count.unwrap_or(0.0) > 0.0 {
        ui.colored_label(ui.visuals().error_fg_color, label);
    } else {
        ui.label(label);
    }
}

fn status(record: &Value) -> Option<String> {
    let flag = |key: &str| record[key].as_bool().unwrap_or(false);
    let mut parts = Vec::new();
    parts.push(if flag("Completed") {
        "Completed"
    } else {
        "Recording"
    });
    if flag("Processed") {
        parts.push("Processed");
    }
    // The DVR's `Corrupted` is what its own admin page calls "Interrupted".
    if flag("Corrupted") {
        parts.push("Interrupted");
    }
    if flag("Delayed") {
        parts.push("Delayed");
    }
    if flag("Cancelled") {
        parts.push("Cancelled");
    }
    if flag("Deleted") {
        parts.push("Deleted");
    }
    let length = record["Duration"].as_f64().unwrap_or(0.0);
    let mut status = parts.join(", ");
    if flag("Completed") && length < crate::api::types::MIN_RECORDING_SECS {
        status.push_str(&if length > 0.0 {
            format!(", Corrupt (only {length:.1} s saved)")
        } else {
            ", Corrupt (0 length)".to_string()
        });
    }
    Some(status)
}

fn scheduled(airing: &Value) -> Option<String> {
    let start = airing["Time"].as_f64()?;
    let mut s = fmt_time(start);
    if let Some(d) = airing["Duration"].as_f64() {
        s.push_str(&format!(" for {}", fmt_hms(d)));
    }
    if let Some(channel) = text(&airing["Channel"]) {
        s.push_str(&format!(" on channel {channel}"));
    }
    Some(s)
}

/// A scalar as display text; `None` for missing, null or empty.
fn text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(if *b { "Yes" } else { "No" }.to_string()),
        _ => None,
    }
}

fn list(value: &Value) -> Option<String> {
    let items: Vec<String> = value.as_array()?.iter().filter_map(text).collect();
    (!items.is_empty()).then(|| items.join(", "))
}

/// The record mixes Unix seconds (`CreatedAt`, `JobTime`, `Airing.Time`)
/// with milliseconds (`UpdatedAt`, `PlayedAt`); anything too large to be
/// seconds in this era is treated as milliseconds.
fn fmt_time(ts: f64) -> String {
    let millis = if ts > 1e11 { ts } else { ts * 1000.0 };
    match Local.timestamp_millis_opt(millis as i64).single() {
        Some(dt) => dt.format("%a, %b %-d, %Y %-I:%M:%S %p").to_string(),
        None => ts.to_string(),
    }
}

fn fmt_hms(secs: f64) -> String {
    let total = secs.max(0.0).round() as u64;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

fn fmt_bytes(bytes: f64) -> String {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    if bytes >= GB {
        format!("{:.2} GB", bytes / GB)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes / MB)
    } else {
        format!("{bytes} bytes")
    }
}

fn fmt_stat(value: Option<f64>, unit: StatUnit) -> String {
    let Some(v) = value else {
        return String::new();
    };
    match unit {
        StatUnit::Percent => format!("{v:.0}%"),
        StatUnit::Bitrate => format!("{:.2} Mbps", v / 1_000_000.0),
        StatUnit::Count if v.fract() == 0.0 => format!("{v:.0}"),
        StatUnit::Count => format!("{v:.1}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(record: &Value) {
        let raw = serde_json::to_string_pretty(record).unwrap();
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| show(ui, record, &raw));
        });
    }

    #[test]
    fn renders_full_and_sparse_records() {
        render(&serde_json::json!({
            "ID": "46250", "Completed": true, "Processed": true,
            "FileSize": 1869076188u64, "Duration": 3659.789277,
            "CreatedAt": 1791532800, "UpdatedAt": 1791668041203u64,
            "Airing": {"Title": "Dateline", "SeasonNumber": 8, "EpisodeNumber": 7,
                       "EpisodeTitle": "The Ultimate Betrayal", "Time": 1791532800,
                       "Duration": 3600, "Genres": ["Newsmagazine"]},
            "SignalStats": {"BPS": {"Initial": 503840, "Last": 6143840, "Min": 503840,
                                    "Max": 6182944, "Sum": 14864771968u64,
                                    "GoodCount": 3652, "BadCount": 0},
                            "Grade": {"GoodCount": 1826, "BadCount": 0}},
            "MediaRegions": [{"Start": 0, "End": 5.808, "Type": "ad",
                              "Sources": ["comskip:0"]}]
        }));
        // An empty recording: no FileSize, SignalStats or MediaRegions.
        render(&serde_json::json!({
            "ID": "46269", "Completed": true, "Corrupted": true, "Duration": 0,
            "BufferStats": {"BufferPct": {"GoodCount": 0, "BadCount": 0}}
        }));
        render(&serde_json::json!({}));
    }

    #[test]
    fn status_flags_zero_length_as_corrupt() {
        let status = status(&serde_json::json!({
            "Completed": true, "Corrupted": true, "Duration": 0
        }))
        .unwrap();
        assert_eq!(status, "Completed, Interrupted, Corrupt (0 length)");
    }

    #[test]
    fn status_flags_a_one_second_recording_as_corrupt() {
        let status = status(&serde_json::json!({"Completed": true, "Duration": 1.295333})).unwrap();
        assert_eq!(status, "Completed, Corrupt (only 1.3 s saved)");
        let status = status_of_hour();
        assert_eq!(status, "Completed");
    }

    fn status_of_hour() -> String {
        status(&serde_json::json!({"Completed": true, "Duration": 3659.8})).unwrap()
    }

    #[test]
    fn times_accept_seconds_and_milliseconds() {
        assert_eq!(fmt_time(1791532800.0), fmt_time(1791532800000.0));
        assert_eq!(fmt_hms(3659.789277), "1:01:00");
        assert_eq!(fmt_hms(5.808), "0:06");
        assert_eq!(fmt_stat(Some(6143840.0), StatUnit::Bitrate), "6.14 Mbps");
    }
}
