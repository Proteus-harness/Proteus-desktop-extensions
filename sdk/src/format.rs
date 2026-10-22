/// Compact token count: `950`, `14k`, `1.2M`; `—` when unknown.
pub fn tokens(value: Option<f64>) -> String {
    match value {
        None => "—".to_owned(),
        Some(value) if value < 1000. => format!("{value:.0}"),
        Some(value) if value < 1_000_000. => format!("{:.0}k", value / 1000.),
        Some(value) => format!("{:.1}M", value / 1_000_000.),
    }
}

/// Exact number with thin-space thousands: `14 021`; `—` when unknown.
pub fn exact(value: Option<i64>) -> String {
    let Some(value) = value else {
        return "—".to_owned();
    };
    let digits = value.unsigned_abs().to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push('\u{202f}');
        }
        out.push(digit);
    }
    if value < 0 { format!("-{out}") } else { out }
}

const ICONS: &[(&str, &str)] = &[
    (
        "file-code",
        "rs py pyi js mjs cjs jsx ts mts cts tsx c h cc cpp cxx hpp hh hxx go java kt swift rb php lua zig css scss html htm vue svelte",
    ),
    ("file-braces", "json jsonc jsonl"),
    ("file-cog", "toml yaml yml ini cfg conf env"),
    ("file-text", "md markdown txt rst adoc"),
    ("file-terminal", "sh bash zsh fish ps1"),
    ("file-image", "png jpg jpeg gif svg webp ico bmp"),
    ("file-archive", "zip tar gz xz bz2 7z zst"),
    ("file-spreadsheet", "csv tsv xlsx ods"),
    ("file-diff", "diff patch"),
    ("file-lock", "lock"),
];

/// Lucide icon for a file by its name and extension.
pub fn file_icon(path: &str) -> &'static str {
    let name = path.rsplit('/').next().unwrap_or(path);
    if matches!(
        name,
        "Cargo.lock" | "package-lock.json" | "yarn.lock" | "poetry.lock" | "uv.lock"
    ) {
        return "file-lock";
    }
    if matches!(name, "Makefile" | "Dockerfile" | "justfile") {
        return "file-terminal";
    }
    if name.starts_with(".git") || name.starts_with(".env") {
        return "file-cog";
    }
    let suffix = match name.rfind('.') {
        Some(dot) if dot > 0 => name[dot + 1..].to_lowercase(),
        _ => return "file",
    };
    ICONS
        .iter()
        .find(|(_, extensions)| extensions.split(' ').any(|extension| extension == suffix))
        .map(|(icon, _)| *icon)
        .unwrap_or("file")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_numbers_and_icons() {
        assert_eq!(tokens(Some(14021.)), "14k");
        assert_eq!(tokens(Some(1_250_000.)), "1.2M");
        assert_eq!(tokens(None), "—");
        assert_eq!(exact(Some(1_234_567)), "1\u{202f}234\u{202f}567");
        assert_eq!(exact(Some(12)), "12");
        assert_eq!(file_icon("src/main.rs"), "file-code");
        assert_eq!(file_icon("Cargo.lock"), "file-lock");
        assert_eq!(file_icon(".gitignore"), "file-cog");
        assert_eq!(file_icon("LICENSE"), "file");
    }
}
