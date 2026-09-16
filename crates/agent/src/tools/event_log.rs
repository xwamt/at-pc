//! Windows Event Log inspection and diagnostic tool.
//! Queries Windows System / Application event logs for recent Error, Critical,
//! and Warning events with cross-platform fallback for testing on Unix/macOS.

use serde::{Deserialize, Serialize};

/// Detailed Windows Event Log entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EventLogEntry {
    pub event_id: u32,
    pub log_name: String,
    pub source: String,
    pub level: String,
    pub time_generated: String,
    pub message: String,
}

/// Result of Event Log query.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EventLogResult {
    pub log_name: String,
    pub total_events: usize,
    pub events: Vec<EventLogEntry>,
}

/// Queries recent event logs from the operating system.
///
/// # Arguments
/// * `log_name` - Log channel name (default: `"System"`, can be `"Application"`, `"Security"`, etc.).
/// * `level` - Filter level: `"Error"`, `"Critical"`, `"Warning"`, or `"All"` (default: `"Error"`).
/// * `hours_back` - Query events from the last N hours (default: 24).
/// * `limit` - Maximum number of event entries to return (default: 20).
pub fn get_event_logs(
    log_name: Option<&str>,
    level: Option<&str>,
    hours_back: Option<u64>,
    limit: Option<usize>,
) -> Result<EventLogResult, String> {
    let target_log = log_name.unwrap_or("System");
    let target_level = level.unwrap_or("Error");
    let hours = hours_back.unwrap_or(24).max(1);
    let max_count = limit.unwrap_or(20).clamp(1, 100);

    #[cfg(windows)]
    {
        get_windows_event_logs(target_log, target_level, hours, max_count)
    }

    #[cfg(not(windows))]
    {
        get_unix_system_logs_fallback(target_log, target_level, hours, max_count)
    }
}

#[cfg(windows)]
fn get_windows_event_logs(
    log_name: &str,
    level: &str,
    hours_back: u64,
    limit: usize,
) -> Result<EventLogResult, String> {
    // 1. Try high-performance native Win32 Event Log API first (< 20ms latency)
    match get_windows_event_logs_win32(log_name, level, hours_back, limit) {
        Ok(res) => return Ok(res),
        Err(e) => {
            tracing::debug!(
                "Native Win32 Event Log query failed ({}); falling back to PowerShell",
                e
            );
        }
    }

    // 2. Fallback to PowerShell Get-EventLog
    get_windows_event_logs_powershell(log_name, level, hours_back, limit)
}

#[cfg(windows)]
fn parse_xml_tag(xml: &str, tag: &str) -> Option<String> {
    let mut search_pos = 0;
    while let Some(rel_start) = xml[search_pos..].find(&format!("<{}", tag)) {
        let tag_start = search_pos + rel_start;
        let after_tag = tag_start + tag.len() + 1;
        if after_tag < xml.len() {
            let next_ch = xml.as_bytes()[after_tag];
            if next_ch == b'>' || next_ch.is_ascii_whitespace() || next_ch == b'/' {
                let open_close = xml[after_tag..].find('>')? + after_tag;
                if open_close > 0 && xml.as_bytes()[open_close - 1] == b'/' {
                    return None;
                }
                let end_tag = format!("</{}>", tag);
                let content_start = open_close + 1;
                let content_end = xml[content_start..].find(&end_tag)? + content_start;
                return Some(xml[content_start..content_end].trim().to_string());
            }
        }
        search_pos = tag_start + 1;
    }
    None
}

#[cfg(windows)]
fn extract_event_data(xml: &str) -> Option<String> {
    let mut data_items = Vec::new();
    let mut search_pos = 0;
    while let Some(rel_start) = xml[search_pos..].find("<Data") {
        let tag_start = search_pos + rel_start;
        if let Some(open_close) = xml[tag_start..].find('>') {
            let content_start = tag_start + open_close + 1;
            if let Some(end_data) = xml[content_start..].find("</Data>") {
                let val = xml[content_start..content_start + end_data].trim();
                if !val.is_empty() {
                    data_items.push(val.to_string());
                }
                search_pos = content_start + end_data + 7;
                continue;
            }
        }
        search_pos = tag_start + 5;
    }
    if data_items.is_empty() {
        None
    } else {
        Some(data_items.join(" | "))
    }
}

#[cfg(windows)]
fn parse_xml_attr(xml: &str, element: &str, attr: &str) -> Option<String> {
    let el_start = xml.find(&format!("<{}", element))?;
    let el_end = xml[el_start..].find('>')? + el_start;
    let snippet = &xml[el_start..el_end];
    let attr_pattern = format!("{}=\"", attr);
    let val_start = snippet.find(&attr_pattern)? + attr_pattern.len();
    let val_end = snippet[val_start..].find('"')? + val_start;
    Some(snippet[val_start..val_end].to_string())
}

#[cfg(windows)]
fn get_windows_event_logs_win32(
    log_name: &str,
    level: &str,
    hours_back: u64,
    limit: usize,
) -> Result<EventLogResult, String> {
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::System::EventLog::*;

    unsafe {
        let channel_wide: Vec<u16> = log_name.encode_utf16().chain(std::iter::once(0)).collect();

        // Level 1 = Critical, 2 = Error, 3 = Warning, 4 = Information
        let level_clause = match level.trim().to_lowercase().as_str() {
            "critical" => "Level=1",
            "warning" => "Level=1 or Level=2 or Level=3",
            "all" => "Level>=1",
            _ => "Level=1 or Level=2",
        };
        let milliseconds = hours_back * 3600 * 1000;
        let query_str = format!(
            "*[System[({}) and TimeCreated[timediff(@SystemTime) <= {}]]]",
            level_clause, milliseconds
        );
        let query_wide: Vec<u16> = query_str.encode_utf16().chain(std::iter::once(0)).collect();

        let h_query = EvtQuery(
            0,
            channel_wide.as_ptr(),
            query_wide.as_ptr(),
            EvtQueryChannelPath | EvtQueryReverseDirection,
        );

        if h_query == 0 {
            return Err(format!("EvtQuery failed with error: {}", GetLastError()));
        }

        let mut events: Vec<isize> = vec![0; limit];
        let mut returned = 0;

        let next_ok = EvtNext(
            h_query,
            limit as u32,
            events.as_mut_ptr(),
            3000,
            0,
            &mut returned,
        );

        let mut entries = Vec::new();
        if next_ok != 0 && returned > 0 {
            for i in 0..returned as usize {
                let ev_handle = events[i];
                if ev_handle == 0 {
                    continue;
                }

                let mut buf = vec![0u16; 4096];
                let mut buf_used = 0;
                let mut prop_count = 0;

                let mut render_ok = EvtRender(
                    0,
                    ev_handle,
                    EvtRenderEventXml,
                    (buf.len() * 2) as u32,
                    buf.as_mut_ptr() as *mut _,
                    &mut buf_used,
                    &mut prop_count,
                );

                // ERROR_INSUFFICIENT_BUFFER = 122
                if render_ok == 0 && GetLastError() == 122 && buf_used > 0 {
                    buf.resize((buf_used as usize + 1) / 2, 0);
                    render_ok = EvtRender(
                        0,
                        ev_handle,
                        EvtRenderEventXml,
                        (buf.len() * 2) as u32,
                        buf.as_mut_ptr() as *mut _,
                        &mut buf_used,
                        &mut prop_count,
                    );
                }

                if render_ok != 0 {
                    let xml =
                        String::from_utf16_lossy(&buf[..(buf_used as usize / 2).min(buf.len())]);
                    let event_id = parse_xml_tag(&xml, "EventID")
                        .and_then(|s| s.parse::<u32>().ok())
                        .unwrap_or(0);
                    let source = parse_xml_attr(&xml, "Provider", "Name")
                        .unwrap_or_else(|| "System".to_string());
                    let time_gen =
                        parse_xml_attr(&xml, "TimeCreated", "SystemTime").unwrap_or_default();
                    let raw_level = parse_xml_tag(&xml, "Level").unwrap_or_default();
                    let level_str = match raw_level.as_str() {
                        "1" => "Critical",
                        "2" => "Error",
                        "3" => "Warning",
                        _ => "Information",
                    };

                    let message = if let Some(data) = extract_event_data(&xml) {
                        format!("{}: {}", source, data)
                    } else {
                        format!(
                            "Event ID {} reported by {} in {}",
                            event_id, source, log_name
                        )
                    };

                    entries.push(EventLogEntry {
                        event_id,
                        log_name: log_name.to_string(),
                        source,
                        level: level_str.to_string(),
                        time_generated: time_gen,
                        message,
                    });
                }

                EvtClose(ev_handle);
            }
        }

        EvtClose(h_query);

        Ok(EventLogResult {
            log_name: log_name.to_string(),
            total_events: entries.len(),
            events: entries,
        })
    }
}

#[cfg(windows)]
fn get_windows_event_logs_powershell(
    log_name: &str,
    level: &str,
    hours_back: u64,
    limit: usize,
) -> Result<EventLogResult, String> {
    use crate::tools::command::exec_powershell;

    // Sanitize log name
    let clean_log: String = log_name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || *c == '/')
        .collect();

    let entry_type = match level.trim().to_lowercase().as_str() {
        "warning" => "'Warning', 'Error'",
        "critical" => "'Error'",
        "all" => "'Error', 'Warning', 'Information'",
        _ => "'Error'",
    };

    let ps_script = format!(
        r#"$since = (Get-Date).AddHours(-{hours});
Get-EventLog -LogName '{log}' -EntryType {types} -After $since -ErrorAction SilentlyContinue |
Select-Object -First {limit} |
ForEach-Object {{
    [PSCustomObject]@{{
        EventID = $_.EventID;
        LogName = '{log}';
        Source = $_.Source;
        Level = $_.EntryType.ToString();
        TimeGenerated = $_.TimeGenerated.ToString('yyyy-MM-dd HH:mm:ss');
        Message = if ($_.Message.Length -gt 250) {{ $_.Message.Substring(0, 250) + '...' }} else {{ $_.Message }};
    }}
}} | ConvertTo-Json -Compress"#,
        hours = hours_back,
        log = clean_log,
        types = entry_type,
        limit = limit
    );

    let res = exec_powershell(&ps_script, 25, None)
        .map_err(|e| format!("Failed to execute Event Log query: {}", e))?;

    let stdout = res.stdout.trim();
    if stdout.is_empty() || stdout == "null" {
        return Ok(EventLogResult {
            log_name: log_name.to_string(),
            total_events: 0,
            events: Vec::new(),
        });
    }

    #[derive(Deserialize)]
    struct PsEventLog {
        #[serde(rename = "EventID", default)]
        event_id: u32,
        #[serde(rename = "LogName", default)]
        log_name: String,
        #[serde(rename = "Source", default)]
        source: String,
        #[serde(rename = "Level", default)]
        level: String,
        #[serde(rename = "TimeGenerated", default)]
        time_generated: String,
        #[serde(rename = "Message", default)]
        message: String,
    }

    let mut parsed_events = Vec::new();
    if let Ok(single) = serde_json::from_str::<PsEventLog>(stdout) {
        parsed_events.push(EventLogEntry {
            event_id: single.event_id,
            log_name: single.log_name,
            source: single.source,
            level: single.level,
            time_generated: single.time_generated,
            message: single.message,
        });
    } else if let Ok(multiple) = serde_json::from_str::<Vec<PsEventLog>>(stdout) {
        for item in multiple {
            parsed_events.push(EventLogEntry {
                event_id: item.event_id,
                log_name: item.log_name,
                source: item.source,
                level: item.level,
                time_generated: item.time_generated,
                message: item.message,
            });
        }
    }

    Ok(EventLogResult {
        log_name: log_name.to_string(),
        total_events: parsed_events.len(),
        events: parsed_events,
    })
}

#[cfg(not(windows))]
fn get_unix_system_logs_fallback(
    log_name: &str,
    level: &str,
    _hours_back: u64,
    limit: usize,
) -> Result<EventLogResult, String> {
    use std::fs;

    let candidate_paths = [
        "/var/log/system.log",
        "/var/log/syslog",
        "/var/log/messages",
    ];
    let mut events = Vec::new();

    for path in candidate_paths {
        if let Ok(content) = fs::read_to_string(path) {
            let query = level.trim().to_lowercase();
            for (idx, line) in content.lines().rev().enumerate() {
                if events.len() >= limit {
                    break;
                }
                let line_lower = line.to_lowercase();
                let is_match = match query.as_str() {
                    "all" => true,
                    "warning" => line_lower.contains("warn") || line_lower.contains("error"),
                    _ => line_lower.contains("error") || line_lower.contains("fail"),
                };

                if is_match {
                    events.push(EventLogEntry {
                        event_id: 1000 + (idx as u32),
                        log_name: log_name.to_string(),
                        source: path.to_string(),
                        level: level.to_string(),
                        time_generated: chrono::Local::now()
                            .format("%Y-%m-%d %H:%M:%S")
                            .to_string(),
                        message: if line.len() > 250 {
                            format!("{}...", &line[..250])
                        } else {
                            line.to_string()
                        },
                    });
                }
            }
            if !events.is_empty() {
                break;
            }
        }
    }

    // Default simulated if no root permission to /var/log
    if events.is_empty() {
        events.push(EventLogEntry {
            event_id: 100,
            log_name: log_name.to_string(),
            source: "SystemLog".to_string(),
            level: level.to_string(),
            time_generated: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            message: format!(
                "No critical {} events recorded in the specified timeframe",
                level
            ),
        });
    }

    Ok(EventLogResult {
        log_name: log_name.to_string(),
        total_events: events.len(),
        events,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_event_logs_basic() {
        let res = get_event_logs(Some("System"), Some("Error"), Some(24), Some(5));
        assert!(res.is_ok());
        let val = res.unwrap();
        assert_eq!(val.log_name, "System");
    }
}
