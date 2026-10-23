// Адаптировано из Zed: crates/theme/src/icon_theme.rs
//! Lucide icons of project entries, grouped like Zed's default icon theme.

use proteus_extension::file_icon;

/// Whole file names with their own icon.
const STEMS: &[(&str, &str)] = &[
    ("container", "Containerfile Dockerfile .dockerignore"),
    (
        "file-terminal",
        "Makefile GNUmakefile justfile Justfile Procfile .bashrc .zshrc .profile .bash_profile .zprofile .zshenv",
    ),
    (
        "file-lock",
        "Cargo.lock package-lock.json yarn.lock poetry.lock uv.lock pnpm-lock.yaml Gemfile.lock bun.lockb",
    ),
    (
        "file-cog",
        ".gitignore .gitattributes .gitmodules .gitkeep .editorconfig .prettierrc .eslintrc",
    ),
    (
        "file-text",
        "LICENSE LICENCE COPYING README AUTHORS CHANGELOG",
    ),
];

const SUFFIXES: &[(&str, &str)] = &[
    (
        "file-code",
        "rs py pyi js mjs cjs jsx ts mts cts tsx c h cc cpp cxx c++ h++ hpp hh hxx inl ixx cppm go java kt kts \
         swift rb php lua luau zig css pcss scss sass less html htm vue svelte astro cs fs dart ex exs eex heex \
         elm erl hrl hs jl ml mli nim nix odin r scala sc sol tcl v wgsl metal gleam cr coffee graphql gql \
         prisma rkt scm clj cljs groovy pl pm vy ipynb",
    ),
    ("file-braces", "json jsonc json5 jsonl"),
    (
        "file-cog",
        "toml yaml yml ini cfg conf properties env editorconfig tf tfvars hcl",
    ),
    (
        "file-text",
        "md markdown mdx txt rst adoc doc docx odt rtf pdf",
    ),
    ("file-terminal", "sh bash zsh fish nu ps1 bat cmd"),
    (
        "file-image",
        "png jpg jpeg gif svg webp ico bmp avif heic heif tiff psd jxl qoi",
    ),
    ("file-music", "aac flac m4a mka mp3 ogg opus wav wma wv"),
    ("file-video-camera", "avi m4v mkv mov mp4 webm wmv"),
    ("file-type", "otf ttf woff woff2"),
    ("file-archive", "zip tar gz tgz xz bz2 7z zst rar jar"),
    ("file-spreadsheet", "csv tsv xls xlsx ods psv ssv"),
    ("database", "sql sqlite sqlite3 db mdb accdb"),
    ("file-diff", "diff patch"),
    ("file-lock", "lock lockb"),
    ("file-key", "pem key crt cer p12 pfx gpg asc"),
    ("file-clock", "log bak"),
    ("file-code-corner", "xml plist hbs"),
];

/// Icon of a file or a symlink by its name and extension.
pub fn file(path: &str, symlink: bool) -> &'static str {
    if symlink {
        return "file-symlink";
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    if let Some((icon, _)) = STEMS
        .iter()
        .find(|(_, names)| names.split(' ').any(|stem| stem == name))
    {
        return icon;
    }
    if name.starts_with("docker-compose.") || name.starts_with("compose.y") {
        return "container";
    }
    let suffix = match name.rfind('.') {
        Some(dot) if dot + 1 < name.len() => name[dot + 1..].to_lowercase(),
        _ => return file_icon(path),
    };
    SUFFIXES
        .iter()
        .find(|(_, suffixes)| suffixes.split_whitespace().any(|known| known == suffix))
        .map(|(icon, _)| *icon)
        .unwrap_or_else(|| file_icon(path))
}

pub fn folder(expanded: bool) -> &'static str {
    if expanded { "folder-open" } else { "folder" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_follow_zed_groups() {
        for (path, icon) in [
            ("src/main.rs", "file-code"),
            ("a/b.py", "file-code"),
            ("package.json", "file-braces"),
            ("Cargo.toml", "file-cog"),
            ("ci.yml", "file-cog"),
            ("README.md", "file-text"),
            ("Cargo.lock", "file-lock"),
            ("logo.PNG", "file-image"),
            ("run.sh", "file-terminal"),
            ("Dockerfile", "container"),
            (".gitignore", "file-cog"),
            ("LICENSE", "file-text"),
            ("data.sqlite", "database"),
            ("noext", "file"),
        ] {
            assert_eq!(file(path, false), icon, "{path}");
        }
        assert_eq!(file("link", true), "file-symlink");
        assert_eq!((folder(true), folder(false)), ("folder-open", "folder"));
    }
}
