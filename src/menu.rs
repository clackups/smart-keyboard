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
                    written_keys.insert(*key);
                    if val.is_empty() {
                        // Empty -> comment out so serde uses the default.
                        out.push_str(&format!("# {} = null\n", field));
                    } else {
                        let formatted = format_toml_value(key, val);
                        out.push_str(&format!("{} = {}\n", field, formatted));
                    }
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
                    if new_val.is_empty() {
                        // Empty -> comment out so serde uses the default.
                        out.push_str(&format!("# {} = null\n", raw_key));
                    } else {
                        let formatted = format_toml_value(&dotted, new_val);
                        out.push_str(&format!("{} = {}\n", raw_key, formatted));
                    }
                    continue;
                }
            }
        }

        // Commented-out `# key = value` line that has an update: replace
        // in place so the active line appears here (not duplicated later
        // by flush_section!).
        if trimmed.starts_with('#') {
            let stripped = trimmed.trim_start_matches('#').trim();
            if let Some((commented_key, _old)) = parse_kv_line(stripped) {
                let dotted = if current_section.is_empty() {
                    commented_key.to_string()
                } else {
                    format!("{}.{}", current_section, commented_key)
                };
                if let Some(new_val) = updates.get(dotted.as_str()) {
                    written_keys.insert(
                        updates.keys()
                            .find(|k| **k == dotted.as_str())
                            .copied()
                            .unwrap_or("")
                    );
                    if new_val.is_empty() {
                        // Still empty/null -> keep original commented line.
                        out.push_str(line);
                        out.push('\n');
                    } else {
                        let formatted = format_toml_value(&dotted, new_val);
                        out.push_str(&format!("{} = {}\n", commented_key, formatted));
                    }
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
        if val.is_empty() {
            let line = format!("# {} = null", field);
            pending_sections.entry(section.to_string())
                .or_default()
                .push(line);
        } else {
            let formatted = format_toml_value(key, val);
            let line = format!("{} = {}", field, formatted);
            pending_sections.entry(section.to_string())
                .or_default()
                .push(line);
        }
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

/// Format a non-empty value for TOML output based on the key name.
///
/// Empty values are handled by `build_toml_text` (commented out as null)
/// and should not reach this function.
fn format_toml_value(key: &str, val: &str) -> String {
    debug_assert!(!val.is_empty(), "format_toml_value called with empty value for {}", key);

    if val == "true" || val == "false" {
        return val.to_string();
    }

    // Keyboard scancodes: always format as hex (0xNN).
    if key.starts_with("input.keyboard.") {
        if let Some(n) = parse_int_relaxed(val) {
            return format!("0x{:02x}", n);
        }
    }

    if (key == "output.ble.vid" || key == "output.ble.pid") && val.starts_with("0x") {
        return val.to_string();
    }

    if let Some(_n) = parse_int_relaxed(val) {
        return val.to_string();
    }

    // ui.active_keymaps -> ["item1", "item2", ...]
    if key == "ui.active_keymaps" {
        let items: Vec<String> = val.split(',')
            .map(|s| format!("\"{}\"", s.trim()))
            .collect();
        return format!("[{}]", items.join(", "));
    }

    // keymap.*.switch_scancode -> [0x03, 0x1e, ...]
    if key.ends_with(".switch_scancode") {
        return format_int_array(val);
    }

    // axis_navigate_horizontal / axis_navigate_vertical -> [int, "string"] or int
    if key.ends_with(".axis_navigate_horizontal") || key.ends_with(".axis_navigate_vertical") {
        return format_axis_config(val);
    }

    format!("\"{}\"", val)
}

/// Format a comma-separated list of integers as a TOML array.
///
/// Each element is written in its original notation (hex if prefixed with
/// `0x`, decimal otherwise).  Example: `"0x03, 0x1e"` -> `[0x03, 0x1e]`.
fn format_int_array(val: &str) -> String {
    let items: Vec<&str> = val.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    if items.is_empty() {
        return "[]".to_string();
    }
    format!("[{}]", items.join(", "))
}

/// Format an axis configuration value.
///
/// Accepts:
///   - `"0, normal"` -> `[0, "normal"]`
///   - `"1, inverted"` -> `[1, "inverted"]`
///   - `"0"` (plain integer) -> `0`
fn format_axis_config(val: &str) -> String {
    let parts: Vec<&str> = val.split(',').map(|s| s.trim()).collect();
    if parts.len() >= 2 {
        let axis = parts[0];
        let dir = parts[1];
        return format!("[{}, \"{}\"]", axis, dir);
    }
    // Plain integer
    if parse_int_relaxed(val).is_some() {
        return val.to_string();
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

    parse_config_pairs(&content)
}

/// Parse TOML content into a flat list of `(dotted.key, value)` pairs.
///
/// Both active (uncommented) and commented-out `# key = value` lines are
/// included.  Active lines always override a previously-seen commented
/// default for the same key.
fn parse_config_pairs(content: &str) -> Vec<(String, String)> {
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
            } else {
                // Key was already seen from a commented line earlier;
                // the active (uncommented) value takes precedence.
                if let Some(existing) = pairs.iter_mut().find(|(k, _)| *k == dotted) {
                    existing.1 = val;
                }
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
// Field metadata for the configuration editor
// =============================================================================

/// Return the list of allowed values for a config key, if the field is
/// an enumerated type.  Returns `None` for free-form fields.
pub fn field_options(key: &str) -> Option<&'static [&'static str]> {
    // Strip section prefix for matching.
    let field = match key.rfind('.') {
        Some(pos) => &key[pos + 1..],
        None => key,
    };

    // Boolean fields.
    match field {
        "enabled" | "rollover" | "center_after_activate"
        | "show_text_display" | "absolute_axes" | "rumble" => {
            return Some(&["true", "false"]);
        }
        _ => {}
    }

    // Enum fields that depend on the full dotted key.
    match key {
        "output.mode" => Some(&["print", "ble"] as &[&str]),
        "output.audio" => Some(&["none", "narrate", "tone", "tone_hint"] as &[&str]),
        _ => None,
    }
    .or_else(|| {
        // GPIO-specific enums (match by field suffix under input.gpio).
        if key.starts_with("input.gpio.") {
            match field {
                "gpio_signal" => Some(&["high", "low"] as &[&str]),
                "gpio_pull" => Some(&["up", "down", "null"] as &[&str]),
                _ => None,
            }
        } else {
            None
        }
    })
}

/// Return `true` when `key` is a colour field that should show a swatch /
/// colour-picker in the config editor.
pub fn is_color_field(key: &str) -> bool {
    key.starts_with("ui.colors.")
}

/// Parse a `#RRGGBB` hex colour string into (r, g, b) floats in 0.0..1.0.
/// Returns `None` when the string is not a valid 6-digit hex colour.
pub fn parse_hex_color(s: &str) -> Option<(f32, f32, f32)> {
    let s = s.trim();
    let hex = s.strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()? as f32 / 255.0;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()? as f32 / 255.0;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()? as f32 / 255.0;
    Some((r, g, b))
}

/// Validate a config value for a given key.  Returns `None` when the value
/// is acceptable, or `Some(error_message)` when it is not.
///
/// Empty values are always accepted (they become `# key = null` on save).
pub fn validate_field(key: &str, val: &str) -> Option<String> {
    let val = val.trim();
    if val.is_empty() {
        return None; // empty -> commented out / default
    }

    // Fields with a fixed list of options.
    if let Some(opts) = field_options(key) {
        if !opts.contains(&val) {
            return Some(format!(
                "expected one of: {}",
                opts.join(", "),
            ));
        }
        return None;
    }

    // Colour fields: must be #RRGGBB.
    if is_color_field(key) {
        if parse_hex_color(val).is_none() {
            return Some("expected #RRGGBB hex colour".into());
        }
        return None;
    }

    let field = match key.rfind('.') {
        Some(pos) => &key[pos + 1..],
        None => key,
    };

    // Integer scancode / numeric fields under input.keyboard.
    if key.starts_with("input.keyboard.") && field != "device" {
        if parse_int_relaxed(val).is_none() {
            return Some("expected an integer (decimal or 0x hex)".into());
        }
        return None;
    }

    // Numeric fields (integer only).
    let int_fields = [
        "axis_threshold", "rumble_duration_ms", "rumble_magnitude",
        "repeat_delay_ms", "repeat_interval_ms",
        "move_max_size", "repeat_interval", "move_max_time",
        "key_release_delay", "lang_switch_release_delay",
    ];
    if int_fields.contains(&field) {
        if parse_int_relaxed(val).is_none() {
            return Some("expected an integer".into());
        }
        return None;
    }

    // VID / PID: hex integer.
    if field == "vid" || field == "pid" {
        if parse_int_relaxed(val).is_none() {
            return Some("expected a hex integer (e.g. 0x1209)".into());
        }
        return None;
    }

    // Array fields: switch_scancode.
    if field == "switch_scancode" {
        // Comma-separated integers.
        for item in val.split(',') {
            let item = item.trim();
            if !item.is_empty() && parse_int_relaxed(item).is_none() {
                return Some(format!("'{}' is not a valid integer", item));
            }
        }
        return None;
    }

    // active_keymaps: comma-separated identifiers.
    if field == "active_keymaps" {
        for item in val.split(',') {
            let item = item.trim();
            if !item.is_empty() && !item.chars().all(|c| c.is_alphanumeric() || c == '_') {
                return Some(format!("'{}' is not a valid keymap name", item));
            }
        }
        return None;
    }

    // axis_navigate_horizontal / axis_navigate_vertical: "index, dir" or integer.
    if field == "axis_navigate_horizontal" || field == "axis_navigate_vertical" {
        let parts: Vec<&str> = val.split(',').map(|s| s.trim()).collect();
        if parts.len() == 1 {
            if parse_int_relaxed(parts[0]).is_none() {
                return Some("expected an integer or 'index, normal|inverted'".into());
            }
        } else if parts.len() == 2 {
            if parse_int_relaxed(parts[0]).is_none() {
                return Some("first element must be an integer axis index".into());
            }
            if parts[1] != "normal" && parts[1] != "inverted" {
                return Some("second element must be 'normal' or 'inverted'".into());
            }
        } else {
            return Some("expected 'index' or 'index, normal|inverted'".into());
        }
        return None;
    }

    // Gamepad button-or-axis fields: integer or "a:N".
    if key.starts_with("input.gamepad.") {
        let nav_act_fields = [
            "navigate_up", "navigate_down", "navigate_left", "navigate_right",
            "activate", "menu", "activate_shift", "activate_ctrl",
            "activate_alt", "activate_altgr", "activate_enter", "activate_space",
            "activate_arrow_left", "activate_arrow_right",
            "activate_arrow_up", "activate_arrow_down",
            "activate_bksp", "navigate_center", "mouse_toggle",
        ];
        if nav_act_fields.contains(&field) {
            // Accept plain integer or "a:N" axis specifier.
            if parse_int_relaxed(val).is_some() {
                return None;
            }
            if let Some(rest) = val.strip_prefix("a:") {
                if parse_int_relaxed(rest).is_some() {
                    return None;
                }
            }
            return Some("expected integer button ID or \"a:N\" axis".into());
        }
    }

    // GPIO line numbers.
    if key.starts_with("input.gpio.") {
        let gpio_nav = [
            "navigate_up", "navigate_down", "navigate_left", "navigate_right",
            "activate", "menu", "activate_shift", "activate_ctrl",
            "activate_alt", "activate_altgr", "activate_enter", "activate_space",
            "activate_arrow_left", "activate_arrow_right",
            "activate_arrow_up", "activate_arrow_down",
            "activate_bksp", "navigate_center", "mouse_toggle",
        ];
        if gpio_nav.contains(&field) {
            if parse_int_relaxed(val).is_none() {
                return Some("expected an integer GPIO line number".into());
            }
            return None;
        }
    }

    None // no validation rule -> accept anything
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

    /// Empty values must be written as `# key = null` (commented out) so
    /// that the TOML parser treats them as absent keys and serde applies
    /// defaults.  Writing them as `key = ""` would fail for non-String types.
    #[test]
    fn empty_values_become_null_comments() {
        let existing = "\
[input.keyboard]
navigate_up = 0x67
activate_shift = 0x2a
";
        let pairs: Vec<(&str, String)> = vec![
            ("input.keyboard.navigate_up",     "0x67".into()),
            ("input.keyboard.activate_shift",  "".into()),    // user cleared it
            ("input.keyboard.mouse_toggle",    "".into()),    // new empty key
        ];
        let result = build_toml_text(existing, &pairs);

        assert!(result.contains("navigate_up = 0x67"),
            "navigate_up missing:\n{}", result);
        assert!(result.contains("# activate_shift = null"),
            "activate_shift should be commented out:\n{}", result);
        assert!(!result.contains("activate_shift = \"\""),
            "activate_shift should NOT be an empty string:\n{}", result);
        assert!(result.contains("# mouse_toggle = null"),
            "mouse_toggle should be commented out:\n{}", result);
    }

    /// Array-typed values must round-trip correctly:
    /// switch_scancode, active_keymaps, axis configs.
    #[test]
    fn array_values_round_trip() {
        let existing = "\
[keymap.us]
switch_scancode = [0x03, 0x1e]

[ui]
active_keymaps = [\"us\", \"ua\"]

[input.gamepad]
enabled = true
";
        let pairs: Vec<(&str, String)> = vec![
            ("keymap.us.switch_scancode",  "0x03, 0x1e".into()),
            ("ui.active_keymaps",          "us, ua, de".into()),
            ("input.gamepad.enabled",      "true".into()),
            ("input.gamepad.axis_navigate_horizontal", "0, normal".into()),
        ];
        let result = build_toml_text(existing, &pairs);

        assert!(result.contains("switch_scancode = [0x03, 0x1e]"),
            "switch_scancode format wrong:\n{}", result);
        assert!(result.contains("active_keymaps = [\"us\", \"ua\", \"de\"]"),
            "active_keymaps format wrong:\n{}", result);
        assert!(result.contains("axis_navigate_horizontal = [0, \"normal\"]"),
            "axis config format wrong:\n{}", result);
    }

    /// The saved TOML text must be parseable by `config::Config`.
    /// This simulates the full round-trip: load pairs -> edit -> save -> parse.
    #[test]
    fn saved_config_is_parseable() {
        let existing = std::fs::read_to_string("config.toml")
            .expect("config.toml should exist in the repo root");

        // Simulate what the config editor does: load all pairs, then save.
        let pairs = load_config_pairs();
        let save_pairs: Vec<(&str, String)> = pairs.iter()
            .map(|(k, v)| (k.as_str(), v.clone()))
            .collect();
        let result = build_toml_text(&existing, &save_pairs);

        // The saved text must be valid TOML parseable by our Config struct.
        let processed = crate::config::strip_null_values(&result);
        let parsed: Result<crate::config::Config, _> = toml::from_str(processed.as_ref());
        match parsed {
            Ok(_) => {},
            Err(e) => panic!(
                "Saved config failed to parse: {}\n\nSaved text:\n{}",
                e, result),
        }
    }

    /// When a commented-out key gets a non-empty value, `build_toml_text`
    /// must uncomment it in place so the active line does not appear after
    /// the old comment -- which would cause `parse_config_pairs` to see the
    /// stale commented value first.
    #[test]
    fn commented_key_uncommented_in_place() {
        let existing = "\
[input.keyboard]
navigate_up = 0x67
# activate_shift = null  # equivalent to activate when Shift is held
# mouse_toggle = null   # toggle mouse-pointer mode
";
        let pairs: Vec<(&str, String)> = vec![
            ("input.keyboard.navigate_up",     "0x67".into()),
            ("input.keyboard.activate_shift",  "0x2a".into()),   // was null, now set
            ("input.keyboard.mouse_toggle",    "".into()),     // still empty
        ];
        let result = build_toml_text(existing, &pairs);

        // activate_shift must appear as an active (uncommented) line.
        assert!(result.contains("activate_shift = 0x2a"),
            "activate_shift should be uncommented with value 0x2a:\n{}", result);
        // There must be NO commented activate_shift = null left.
        assert!(!result.contains("# activate_shift"),
            "old commented activate_shift should be gone:\n{}", result);

        // mouse_toggle is still empty -> stays commented.
        assert!(result.contains("# mouse_toggle = null"),
            "mouse_toggle should stay commented:\n{}", result);

        // No duplicate keys: activate_shift should appear exactly once.
        let count = result.lines()
            .filter(|l| {
                let t = l.trim();
                t.starts_with("activate_shift")
            })
            .count();
        assert_eq!(count, 1,
            "activate_shift should appear exactly once:\n{}", result);
    }

    /// parse_config_pairs must let active (uncommented) lines override values
    /// that were already seen from a commented line earlier in the file.
    #[test]
    fn active_line_overrides_earlier_comment() {
        // Simulate the state after a previous save where a commented line
        // precedes its active replacement (the bug scenario).
        let content = "\
[input.keyboard]
# activate_shift = null  # description
activate_shift = 0x2a
navigate_up = 0x67
";
        let pairs = parse_config_pairs(content);

        let val = pairs.iter()
            .find(|(k, _)| k == "input.keyboard.activate_shift")
            .map(|(_, v)| v.as_str());
        assert_eq!(val, Some("0x2a"),
            "Active line should override commented value; pairs: {:?}", pairs);
    }

    /// Full round-trip: load -> change a commented value -> save -> reload.
    /// The reloaded value must reflect the change, not the old comment.
    #[test]
    fn round_trip_comment_to_active() {
        let original = "\
[input.keyboard]
navigate_up = 0x67
# activate_shift = null  # equivalent to activate when Shift is held
";
        // Step 1: parse pairs from the original text.
        let mut pairs = parse_config_pairs(original);

        // Step 2: user changes activate_shift from empty to "42" (decimal).
        if let Some(p) = pairs.iter_mut().find(|(k, _)| k == "input.keyboard.activate_shift") {
            p.1 = "42".to_string();
        }

        // Step 3: save.
        let save_pairs: Vec<(&str, String)> = pairs.iter()
            .map(|(k, v)| (k.as_str(), v.clone()))
            .collect();
        let saved = build_toml_text(original, &save_pairs);

        // Step 4: reload and check.  format_toml_value converts keyboard
        // scancodes to hex, so "42" becomes "0x2a".
        let reloaded = parse_config_pairs(&saved);

        let val = reloaded.iter()
            .find(|(k, _)| k == "input.keyboard.activate_shift")
            .map(|(_, v)| v.as_str());
        assert_eq!(val, Some("0x2a"),
            "After round-trip, activate_shift should be 0x2a.\n\
             Saved text:\n{}\nReloaded pairs: {:?}", saved, reloaded);
    }

    /// Keyboard scancode values entered as decimal are saved as hex.
    #[test]
    fn keyboard_scancodes_saved_as_hex() {
        let existing = "\
[input.keyboard]
navigate_up = 103
menu = 50
";
        let pairs: Vec<(&str, String)> = vec![
            ("input.keyboard.navigate_up",  "103".into()),
            ("input.keyboard.menu",         "50".into()),
        ];
        let result = build_toml_text(existing, &pairs);

        assert!(result.contains("navigate_up = 0x67"),
            "navigate_up should be 0x67 (hex for 103):\n{}", result);
        assert!(result.contains("menu = 0x32"),
            "menu should be 0x32 (hex for 50):\n{}", result);
    }

    // -----------------------------------------------------------------
    // Validation / field-metadata tests
    // -----------------------------------------------------------------

    #[test]
    fn field_options_returns_correct_lists() {
        assert_eq!(field_options("output.mode"), Some(&["print", "ble"] as &[&str]));
        assert_eq!(field_options("output.audio"),
            Some(&["none", "narrate", "tone", "tone_hint"] as &[&str]));
        assert_eq!(field_options("input.gpio.gpio_signal"),
            Some(&["high", "low"] as &[&str]));
        assert_eq!(field_options("input.gpio.gpio_pull"),
            Some(&["up", "down", "null"] as &[&str]));
        // Boolean fields
        assert_eq!(field_options("input.gamepad.enabled"),
            Some(&["true", "false"] as &[&str]));
        assert_eq!(field_options("navigate.rollover"),
            Some(&["true", "false"] as &[&str]));
        // Non-enum field
        assert_eq!(field_options("input.keyboard.navigate_up"), None);
    }

    #[test]
    fn is_color_field_recognises_colours() {
        assert!(is_color_field("ui.colors.key_normal"));
        assert!(is_color_field("ui.colors.nav_sel"));
        assert!(!is_color_field("ui.show_text_display"));
        assert!(!is_color_field("output.mode"));
    }

    #[test]
    fn parse_hex_color_valid() {
        assert_eq!(parse_hex_color("#ff0000"), Some((1.0, 0.0, 0.0)));
        assert_eq!(parse_hex_color("#00ff00"), Some((0.0, 1.0, 0.0)));
        assert_eq!(parse_hex_color("#000000"), Some((0.0, 0.0, 0.0)));
        assert!(parse_hex_color("#fff").is_none());
        assert!(parse_hex_color("red").is_none());
        assert!(parse_hex_color("").is_none());
    }

    #[test]
    fn validate_field_enum_values() {
        assert!(validate_field("output.mode", "print").is_none());
        assert!(validate_field("output.mode", "ble").is_none());
        assert!(validate_field("output.mode", "usb").is_some());
        assert!(validate_field("output.mode", "").is_none()); // empty is always ok
    }

    #[test]
    fn validate_field_colour() {
        assert!(validate_field("ui.colors.nav_sel", "#ffc800").is_none());
        assert!(validate_field("ui.colors.nav_sel", "red").is_some());
        assert!(validate_field("ui.colors.nav_sel", "#fff").is_some());
        assert!(validate_field("ui.colors.nav_sel", "").is_none());
    }

    #[test]
    fn validate_field_integers() {
        assert!(validate_field("input.keyboard.navigate_up", "0x67").is_none());
        assert!(validate_field("input.keyboard.navigate_up", "103").is_none());
        assert!(validate_field("input.keyboard.navigate_up", "abc").is_some());
        assert!(validate_field("input.gamepad.axis_threshold", "16384").is_none());
        assert!(validate_field("input.gamepad.axis_threshold", "xyz").is_some());
    }

    #[test]
    fn validate_field_gamepad_button_or_axis() {
        assert!(validate_field("input.gamepad.activate", "5").is_none());
        assert!(validate_field("input.gamepad.activate", "a:2").is_none());
        assert!(validate_field("input.gamepad.activate", "foo").is_some());
    }
}
