//! Terminal formatting helpers.

use ct_domain::{Confidence, TokenCount};

/// Thousands-separated integer, e.g. `146820` -> `146,820`.
pub fn thousands(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Human byte size.
pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Percentage with one decimal, e.g. `0.417` -> `41.7%`.
pub fn percent(share: f32) -> String {
    format!("{:.1}%", share * 100.0)
}

/// Truncate to `max` characters, marking that it was shortened.
pub fn ellipsize(s: &str, max: usize) -> String {
    let clean: String = s
        .chars()
        // Control characters in tool output would otherwise corrupt the table.
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let trimmed = clean.trim();
    if trimmed.chars().count() <= max {
        return trimmed.to_string();
    }
    let kept: String = trimmed.chars().take(max.saturating_sub(1)).collect();
    format!("{kept}\u{2026}")
}

/// Shorten by removing the middle, keeping both ends.
///
/// For anything path-shaped, cutting the tail throws away the only part that
/// identifies it: `C:\Users\anesk\source\repos\VoxMux\BACKLO…` names a machine
/// and a repository but not a file. Commands are the opposite -- their start
/// carries the meaning -- so keeping both ends is the one rule that serves both.
pub fn ellipsize_middle(s: &str, max: usize) -> String {
    let clean: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let trimmed = clean.trim();
    let len = trimmed.chars().count();
    if len <= max {
        return trimmed.to_string();
    }
    // Below this there is no middle worth preserving; fall back to a plain cut.
    if max < 8 {
        return ellipsize(trimmed, max);
    }
    let budget = max - 1;
    let head = budget.div_ceil(2);
    let tail = budget - head;
    let start: String = trimmed.chars().take(head).collect();
    let end: String = trimmed.chars().skip(len - tail).collect();
    format!("{start}\u{2026}{end}")
}

/// Pad to `width` display columns.
pub fn pad(s: &str, width: usize) -> String {
    let len = s.chars().count();
    if len >= width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(width - len))
    }
}

/// Right-align within `width`.
pub fn rpad(s: &str, width: usize) -> String {
    let len = s.chars().count();
    if len >= width {
        s.to_string()
    } else {
        format!("{}{s}", " ".repeat(width - len))
    }
}

/// A short tag naming how much a figure can be trusted.
///
/// Printed next to every token count. The whole point of ContextTrace's type
/// system is that a reader can always tell a measurement from a guess, and that
/// guarantee is worthless if the terminal output drops the distinction.
pub fn confidence_tag(c: Confidence) -> &'static str {
    match c {
        Confidence::Observed => "[observed]",
        Confidence::Derived => "[exact]",
        Confidence::Estimated => "[estimated]",
    }
}

/// Describe a token count including how it was arrived at.
pub fn token_count(tc: TokenCount) -> String {
    match tc {
        TokenCount::Observed { tokens } => format!("{} [observed]", thousands(tokens)),
        TokenCount::Exact { tokens } => format!("{} [exact]", thousands(tokens)),
        TokenCount::Calibrated {
            tokens,
            raw_estimate,
        } => format!(
            "{} [calibrated from {}]",
            thousands(tokens),
            thousands(raw_estimate)
        ),
        TokenCount::Estimated { tokens } => format!("{} [estimated]", thousands(tokens)),
    }
}

/// A simple proportional bar for terminal histograms.
pub fn bar(share: f32, width: usize) -> String {
    let filled = ((share.clamp(0.0, 1.0)) * width as f32).round() as usize;
    format!(
        "{}{}",
        "\u{2588}".repeat(filled),
        "\u{00b7}".repeat(width.saturating_sub(filled))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_separates_correctly() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(146_820), "146,820");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn bytes_scales_units() {
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(2048), "2.0 KB");
        assert!(bytes(55_469_083).ends_with("MB"));
    }

    #[test]
    fn ellipsize_strips_control_characters() {
        assert_eq!(ellipsize("a\nb\tc", 10), "a b c");
        assert_eq!(ellipsize("abcdefghij", 5), "abcd\u{2026}");
        assert_eq!(ellipsize("abc", 5), "abc");
    }

    #[test]
    fn ellipsize_handles_multibyte_without_panicking() {
        assert_eq!(ellipsize("ααααα", 3), "αα\u{2026}");
    }

    #[test]
    fn a_shortened_path_still_shows_which_file_it_is() {
        // The regression this guards: cutting the tail off an absolute path
        // leaves a row that names a machine and a repository but not a file,
        // which is the one thing the reader is looking for.
        let path = "Tool output: Read C:\\Users\\anesk\\source\\repos\\VoxMux\\docs\\HANDBOOK.md";
        let short = ellipsize_middle(path, 48);
        assert!(short.starts_with("Tool output: Read"), "got {short}");
        assert!(short.ends_with("HANDBOOK.md"), "the filename must survive: {short}");
        assert_eq!(short.chars().count(), 48);
    }

    #[test]
    fn middle_ellipsis_leaves_short_text_alone_and_never_panics() {
        assert_eq!(ellipsize_middle("short", 40), "short");
        assert_eq!(ellipsize_middle("ααααααααα", 6).chars().count(), 6);
        // Too narrow for two ends: degrade to a plain cut rather than nonsense.
        assert_eq!(ellipsize_middle("abcdefghij", 4), "abc\u{2026}");
    }

    #[test]
    fn calibrated_counts_disclose_their_original_estimate() {
        let text = token_count(TokenCount::calibrated(61_240, 67_800));
        assert!(text.contains("61,240"));
        assert!(text.contains("67,800"), "the pre-scaling figure must stay visible");
    }

    #[test]
    fn observed_counts_are_marked_as_such() {
        assert_eq!(token_count(TokenCount::observed(1000)), "1,000 [observed]");
    }

    #[test]
    fn bar_is_proportional_and_clamped() {
        assert_eq!(bar(0.0, 4).chars().filter(|c| *c == '\u{2588}').count(), 0);
        assert_eq!(bar(1.0, 4).chars().filter(|c| *c == '\u{2588}').count(), 4);
        assert_eq!(bar(5.0, 4).chars().filter(|c| *c == '\u{2588}').count(), 4);
        assert_eq!(bar(0.5, 4).chars().count(), 4);
    }
}
