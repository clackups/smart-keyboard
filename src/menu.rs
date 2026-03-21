// src/menu.rs
//
// TOML configuration save/restart helpers.
//
// The FLTK-based modal configuration editor has been replaced by an inline
// view rendered in main.rs.  This module retains the TOML text-rewriting
// logic (`build_toml_text`) and the `restart_application` helper.

#![allow(dead_code)]

// =============================================================================
// Integer helper
// =============================================================================

fn parse_int_relaxed(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() { return None; }
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).ok()
    } else {
        s.parse::<u64>().ok()
    }
}

// =============================================================================
// TOML text building
// =============================================================================

/// Build TOML text from the key-value pairs collected by the editor widgets
/// and write it to `config.toml`.
pub fn build_toml_and_save(pairs: &[(&str, String)]) -> Result<(), String> {
    let dir = std::env::var("SMART_KBD_CONFIG_PATH").unwrap_or_else(|_| ".".into());
    let path = std::path::Path::new(&dir).join("config.toml");

    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let out = build_toml_text(&existing, pairs);
    std::fs::write(&path, &out).map_err(|e| format!("{}", e))
}

/// Rewrite TOML text: for every key in `pairs`, replace its value in the
/// existing text (if it appears as an uncommented `key = value` line) or
/// append it at the end of the correct section.  Sections that do not exist
/// in the original text are appended at the end.
fn build_toml_text(existing: &str, pairs: &[(&str, String)]) -> String {
    let mut updates: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    for (k, v) in pairs {
        updates.insert(k, v.as_str());
    }

    let mut out = String::with_capacity(existing.len() + 512);
    let mut current_section = String::new();
    let mut written_keys: std::collections::HashSet<&str> = std::collections::HashSet::new();

    macro_rules! flush_section {
        ($section:expr) => {
            for (key, val) in pairs {
                if written_keys.contains(key) { continue; }
                let sec = match key.rfind('.') {
                    Some(i) => &key[..i],
                    None => "",
                };
                if sec == $section {
                    let field = match key.rfind('.') {
                        Some(i) => &key[i+1..],
                        None => *key,
                    };
                    let formatted = format_toml_value(key, val);
                    out.push_str(&format!("{} = {}\n", field, formatted));
                    written_keys.insert(*key);
                }
            }
        };
    }

    for line in existing.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with('[') && !trimmed.starts_with("[[") {
            flush_section!(&current_section);

            let section = trimmed.trim_start_matches('[')
                .split(']').next().unwrap_or("").trim().to_string();
            current_section = section;
            out.push_str(line);
            out.push('\n');
            continue;
        }

        if let Some(eq_pos) = trimmed.find('=') {
            let raw_key = trimmed[..eq_pos].trim();
            if !raw_key.starts_with('#') {
                let dotted = if current_section.is_empty() {
                    raw_key.to_string()
                } else {
                    format!("{}.{}", current_section, raw_key)
                };

                if let Some(new_val) = updates.get(dotted.as_str()) {
                    written_keys.insert(updates.keys().find(|k| **k == dotted.as_str()).copied().unwrap_or(""));
                    let formatted = format_toml_value(&dotted, new_val);
                    out.push_str(&format!("{} = {}\n", raw_key, formatted));
                    continue;
                }
            }
        }

        out.push_str(line);
        out.push('\n');
    }

    flush_section!(&current_section);

    let mut pending_sections: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for (key, val) in pairs {
        if written_keys.contains(key) { continue; }
        let (section, field) = match key.rfind('.') {
            Some(i) => (&key[..i], &key[i+1..]),
            None => ("", *key),
        };
        let formatted = format_toml_value(key, val);
        let line = format!("{} = {}", field, formatted);
        pending_sections.entry(section.to_string())
            .or_default()
            .push(line);
    }
    for (section, lines) in &pending_sections {
        if !section.is_empty() {
            out.push_str(&format!("\n[{}]\n", section));
        }
        for line in lines {
            out.push_str(line);
            out.push('\n');
        }
    }

    out
}

/// Format a value for TOML output based on the key name.
fn format_toml_value(key: &str, val: &str) -> String {
    if val == "true" || val == "false" {
        return val.to_string();
    }

    if key.starts_with("input.keyboard.") && val.starts_with("0x") {
        return val.to_string();
    }

    if (key == "output.ble.vid" || key == "output.ble.pid") && val.starts_with("0x") {
        return val.to_string();
    }

    if let Some(_n) = parse_int_relaxed(val) {
        return val.to_string();
    }

    if key == "ui.active_keymaps" {
        let items: Vec<String> = val.split(',')
            .map(|s| format!("\"{}\"", s.trim()))
            .collect();
        return format!("[{}]", items.join(", "));
    }

    if val.is_empty() {
        return "\"\"".to_string();
    }

    format!("\"{}\"", val)
}

// =============================================================================
// Application restart
// =============================================================================

/// Restart the application by re-exec'ing the current binary.
pub fn restart_application() {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => {
            eprintln!("[menu] cannot determine executable path; exiting instead");
            std::process::exit(1);
        }
    };
    let args: Vec<String> = std::env::args().collect();
    eprintln!("[menu] restarting: {:?} {:?}", exe, &args[1..]);
    use std::os::unix::process::CommandExt;
    let err = std::process::Command::new(&exe)
        .args(&args[1..])
        .exec();
    eprintln!("[menu] exec failed: {}", err);
    std::process::exit(1);
}

// =============================================================================
// Config pair loading for the configuration editor
// =============================================================================

/// Read config.toml and return a flat list of `(dotted.key, value)` pairs.
///
/// Both active (uncommented) and commented-out `# key = value` lines are
/// included, so the configuration editor shows every available option.
/// The order follows the file's section and line order.
pub fn load_config_pairs() -> Vec<(String, String)> {
    let dir = std::env::var("SMART_KBD_CONFIG_PATH").unwrap_or_else(|_| ".".into());
    let path = std::path::Path::new(&dir).join("config.toml");

    let content = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let mut pairs = Vec::new();
    let mut current_section = String::new();
    let mut seen = std::collections::HashSet::new();

    for line in content.lines() {
        let trimmed = line.trim();

        // Section header: [section.name]
        if trimmed.starts_with('[') && !trimmed.starts_with("[[") {
            current_section = trimmed
                .trim_start_matches('[')
                .split(']')
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            continue;
        }

        // Try to parse as `key = value` (active line).
        if let Some((key, val)) = parse_kv_line(trimmed) {
            let dotted = if current_section.is_empty() {
                key.to_string()
            } else {
                format!("{}.{}", current_section, key)
            };
            if seen.insert(dotted.clone()) {
                pairs.push((dotted, val));
            }
            continue;
        }

        // Try to parse as `# key = value` (commented-out line).
        let stripped = trimmed.trim_start_matches('#').trim();
        if let Some((key, val)) = parse_kv_line(stripped) {
            let dotted = if current_section.is_empty() {
                key.to_string()
            } else {
                format!("{}.{}", current_section, key)
            };
            // Only add if not already set by an active line.
            if seen.insert(dotted.clone()) {
                pairs.push((dotted, val));
            }
        }
    }

    pairs
}

/// Parse a `key = value` line, returning `(key, display_value)`.
fn parse_kv_line(s: &str) -> Option<(&str, String)> {
    let eq_pos = s.find('=')?;
    let raw_key = s[..eq_pos].trim();
    // Reject keys that look like comments or section headers.
    if raw_key.is_empty()
        || raw_key.starts_with('#')
        || raw_key.starts_with('[')
        || raw_key.contains(' ')
    {
        return None;
    }
    let raw_val = s[eq_pos + 1..].trim();
    // Strip inline comments: `value  # comment`
    let val_str = strip_inline_comment(raw_val);
    let display = toml_display_value(val_str);
    Some((raw_key, display))
}

/// Strip an inline `# comment` from a TOML value string, respecting quoted
/// strings.
fn strip_inline_comment(s: &str) -> &str {
    let mut in_str = false;
    let mut in_arr = 0u32;
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i];
        if ch == b'"' && (i == 0 || bytes[i - 1] != b'\\') {
            in_str = !in_str;
        }
        if !in_str {
            if ch == b'[' { in_arr += 1; }
            if ch == b']' { in_arr = in_arr.saturating_sub(1); }
            if ch == b'#' && in_arr == 0 {
                return s[..i].trim_end();
            }
        }
        i += 1;
    }
    s.trim_end()
}

/// Convert a raw TOML value string to a user-friendly display string.
///
/// Quoted strings have their quotes removed; arrays are flattened to a
/// comma-separated list of unquoted items; everything else is kept as-is.
fn toml_display_value(s: &str) -> String {
    // Quoted string
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        return s[1..s.len() - 1].to_string();
    }
    // Array: [item, item, ...]
    if s.starts_with('[') && s.ends_with(']') {
        let inner = s[1..s.len() - 1].trim();
        let items: Vec<&str> = inner.split(',').map(|i| {
            let i = i.trim();
            if i.starts_with('"') && i.ends_with('"') && i.len() >= 2 {
                &i[1..i.len() - 1]
            } else {
                i
            }
        }).collect();
        return items.join(", ");
    }
    // "null" sentinel from commented lines
    if s == "null" {
        return String::new();
    }
    s.to_string()
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Saving settings for a section that has commented-out keys must NOT
    /// produce a duplicate section header.
    #[test]
    fn no_duplicate_sections() {
        let existing = "\
[input.keyboard]
navigate_up = 0xff52

[input.gamepad]
enabled = true
device = \"auto\"
# axis_threshold = 16384
# rumble = false

[output]
mode = \"print\"
";
        let pairs: Vec<(&str, String)> = vec![
            ("input.keyboard.navigate_up",       "0xff52".into()),
            ("input.gamepad.enabled",            "true".into()),
            ("input.gamepad.device",             "auto".into()),
            ("input.gamepad.axis_threshold",     "16384".into()),
            ("input.gamepad.rumble",             "false".into()),
            ("output.mode",                      "print".into()),
        ];
        let result = build_toml_text(existing, &pairs);

        let count = result.lines()
            .filter(|l| l.trim() == "[input.gamepad]")
            .count();
        assert_eq!(count, 1, "Expected exactly one [input.gamepad] section, got {}.\nOutput:\n{}", count, result);

        assert!(result.contains("axis_threshold = 16384"), "axis_threshold missing:\n{}", result);
        assert!(result.contains("rumble = false"), "rumble missing:\n{}", result);
    }

    /// Keys for a brand-new section (not in existing file) are appended.
    #[test]
    fn new_section_appended() {
        let existing = "\
[output]
mode = \"print\"
";
        let pairs: Vec<(&str, String)> = vec![
            ("output.mode",           "ble".into()),
            ("navigate.rollover",     "true".into()),
        ];
        let result = build_toml_text(existing, &pairs);

        assert!(result.contains("[navigate]"), "Missing [navigate] section:\n{}", result);
        assert!(result.contains("rollover = true"), "Missing rollover:\n{}", result);

        assert!(result.contains("mode = \"ble\""), "mode not updated:\n{}", result);
    }

    /// Existing uncommented keys are updated in place.
    #[test]
    fn existing_keys_updated() {
        let existing = "\
[input.gamepad]
enabled = true
device = \"auto\"
";
        let pairs: Vec<(&str, String)> = vec![
            ("input.gamepad.enabled", "false".into()),
            ("input.gamepad.device",  "xbox".into()),
        ];
        let result = build_toml_text(existing, &pairs);

        assert!(result.contains("enabled = false"), "enabled not updated:\n{}", result);
        assert!(result.contains("device = \"xbox\""), "device not updated:\n{}", result);

        let count = result.lines()
            .filter(|l| l.trim() == "[input.gamepad]")
            .count();
        assert_eq!(count, 1, "Duplicate section headers:\n{}", result);
    }
}
