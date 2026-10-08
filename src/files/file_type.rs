//! A file's type as its icon shows it (specs/files.md §3.1): read from its name,
//! drawn with a Nerd Font glyph (codepoints after nvim-web-devicons).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    Archive,
    Audio,
    C,
    Cargo,
    Config,
    Cpp,
    CSharp,
    Css,
    Csv,
    Dart,
    Docker,
    EditorConfig,
    Elixir,
    Env,
    Font,
    Git,
    Go,
    GraphQl,
    Haskell,
    Header,
    Html,
    Image,
    Java,
    JavaScript,
    Json,
    Jsx,
    Kotlin,
    License,
    Lock,
    Log,
    Lua,
    Makefile,
    Markdown,
    Nix,
    Notebook,
    Npm,
    Pdf,
    Php,
    Python,
    Readme,
    Ruby,
    Rust,
    Sass,
    Scala,
    Shell,
    Sql,
    Svelte,
    Svg,
    Swift,
    Terraform,
    Text,
    Toml,
    Tsx,
    TypeScript,
    TypeScriptDeclaration,
    Video,
    Vue,
    Wasm,
    Xml,
    Yaml,
    Zig,
}

/// Whole names, matched before any extension.
const BY_NAME: &[(&str, FileType)] = &[
    ("Cargo.toml", FileType::Cargo),
    ("Cargo.lock", FileType::Cargo),
    ("Dockerfile", FileType::Docker),
    ("Containerfile", FileType::Docker),
    (".dockerignore", FileType::Docker),
    ("docker-compose.yml", FileType::Docker),
    ("docker-compose.yaml", FileType::Docker),
    ("compose.yml", FileType::Docker),
    ("compose.yaml", FileType::Docker),
    ("Makefile", FileType::Makefile),
    ("GNUmakefile", FileType::Makefile),
    ("justfile", FileType::Makefile),
    (".gitignore", FileType::Git),
    (".gitattributes", FileType::Git),
    (".gitmodules", FileType::Git),
    (".gitkeep", FileType::Git),
    ("package.json", FileType::Npm),
    ("package-lock.json", FileType::Npm),
    (".npmrc", FileType::Npm),
    ("tsconfig.json", FileType::TypeScript),
    ("go.mod", FileType::Go),
    ("go.sum", FileType::Go),
    ("Gemfile", FileType::Ruby),
    ("Rakefile", FileType::Ruby),
    ("LICENSE", FileType::License),
    ("LICENSE.md", FileType::License),
    ("LICENSE.txt", FileType::License),
    ("LICENCE", FileType::License),
    ("COPYING", FileType::License),
    ("README", FileType::Readme),
    ("README.md", FileType::Readme),
    ("README.txt", FileType::Readme),
    (".env", FileType::Env),
    (".editorconfig", FileType::EditorConfig),
    ("yarn.lock", FileType::Lock),
    ("pnpm-lock.yaml", FileType::Lock),
];

/// A double extension comes before the single one it ends with (`d.ts` before `ts`).
const BY_EXTENSION: &[(&str, FileType)] = &[
    ("d.ts", FileType::TypeScriptDeclaration),
    ("rs", FileType::Rust),
    ("ts", FileType::TypeScript),
    ("mts", FileType::TypeScript),
    ("cts", FileType::TypeScript),
    ("tsx", FileType::Tsx),
    ("js", FileType::JavaScript),
    ("mjs", FileType::JavaScript),
    ("cjs", FileType::JavaScript),
    ("jsx", FileType::Jsx),
    ("json", FileType::Json),
    ("jsonc", FileType::Json),
    ("json5", FileType::Json),
    ("toml", FileType::Toml),
    ("yaml", FileType::Yaml),
    ("yml", FileType::Yaml),
    ("md", FileType::Markdown),
    ("markdown", FileType::Markdown),
    ("mdx", FileType::Markdown),
    ("html", FileType::Html),
    ("htm", FileType::Html),
    ("css", FileType::Css),
    ("scss", FileType::Sass),
    ("sass", FileType::Sass),
    ("less", FileType::Css),
    ("py", FileType::Python),
    ("pyi", FileType::Python),
    ("ipynb", FileType::Notebook),
    ("go", FileType::Go),
    ("java", FileType::Java),
    ("kt", FileType::Kotlin),
    ("kts", FileType::Kotlin),
    ("swift", FileType::Swift),
    ("c", FileType::C),
    ("cpp", FileType::Cpp),
    ("cc", FileType::Cpp),
    ("cxx", FileType::Cpp),
    ("h", FileType::Header),
    ("hpp", FileType::Header),
    ("hh", FileType::Header),
    ("cs", FileType::CSharp),
    ("sh", FileType::Shell),
    ("bash", FileType::Shell),
    ("zsh", FileType::Shell),
    ("fish", FileType::Shell),
    ("rb", FileType::Ruby),
    ("php", FileType::Php),
    ("sql", FileType::Sql),
    ("lua", FileType::Lua),
    ("ex", FileType::Elixir),
    ("exs", FileType::Elixir),
    ("dart", FileType::Dart),
    ("scala", FileType::Scala),
    ("zig", FileType::Zig),
    ("nix", FileType::Nix),
    ("hs", FileType::Haskell),
    ("tf", FileType::Terraform),
    ("graphql", FileType::GraphQl),
    ("gql", FileType::GraphQl),
    ("wasm", FileType::Wasm),
    ("vue", FileType::Vue),
    ("svelte", FileType::Svelte),
    ("xml", FileType::Xml),
    ("plist", FileType::Xml),
    ("csv", FileType::Csv),
    ("tsv", FileType::Csv),
    ("txt", FileType::Text),
    ("log", FileType::Log),
    ("ini", FileType::Config),
    ("conf", FileType::Config),
    ("cfg", FileType::Config),
    ("mk", FileType::Makefile),
    ("lock", FileType::Lock),
    ("env", FileType::Env),
    ("png", FileType::Image),
    ("jpg", FileType::Image),
    ("jpeg", FileType::Image),
    ("gif", FileType::Image),
    ("webp", FileType::Image),
    ("ico", FileType::Image),
    ("bmp", FileType::Image),
    ("icns", FileType::Image),
    ("svg", FileType::Svg),
    ("pdf", FileType::Pdf),
    ("zip", FileType::Archive),
    ("tar", FileType::Archive),
    ("gz", FileType::Archive),
    ("tgz", FileType::Archive),
    ("xz", FileType::Archive),
    ("bz2", FileType::Archive),
    ("7z", FileType::Archive),
    ("rar", FileType::Archive),
    ("ttf", FileType::Font),
    ("otf", FileType::Font),
    ("woff", FileType::Font),
    ("woff2", FileType::Font),
    ("mp4", FileType::Video),
    ("mov", FileType::Video),
    ("webm", FileType::Video),
    ("mp3", FileType::Audio),
    ("wav", FileType::Audio),
    ("flac", FileType::Audio),
    ("ogg", FileType::Audio),
];

/// The type of the file at `path` (`/`-separated, or a bare name), ignoring case:
/// its whole name first, then its extension; `None` when neither is known.
pub fn file_type(path: &str) -> Option<FileType> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let by_name = BY_NAME
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(name));
    let by_extension = || {
        BY_EXTENSION
            .iter()
            .find(|(extension, _)| has_extension(name, extension))
    };
    by_name.or_else(by_extension).map(|(_, kind)| *kind)
}

/// `name` ends with `.extension` after a non-empty stem — `.rs` alone has none.
fn has_extension(name: &str, extension: &str) -> bool {
    let Some(stem_len) = name.len().checked_sub(extension.len() + 1) else {
        return false;
    };
    let (stem, suffix) = name.as_bytes().split_at(stem_len);
    !stem.is_empty() && suffix[0] == b'.' && suffix[1..].eq_ignore_ascii_case(extension.as_bytes())
}

impl FileType {
    /// Its Nerd Font glyph — private-use codepoints, served by the mono family.
    pub const fn glyph(self) -> char {
        match self {
            Self::Archive => '\u{f410}',
            Self::Audio => '\u{f001}',
            Self::C => '\u{e61e}',
            Self::Cargo | Self::Rust => '\u{e68b}',
            Self::Config => '\u{e615}',
            Self::Cpp => '\u{e61d}',
            Self::CSharp => '\u{f031b}',
            Self::Css => '\u{e6b8}',
            Self::Csv => '\u{e64a}',
            Self::Dart => '\u{e798}',
            Self::Docker => '\u{f0868}',
            Self::EditorConfig => '\u{e652}',
            Self::Elixir => '\u{e62d}',
            Self::Env => '\u{f462}',
            Self::Font => '\u{f031}',
            Self::Git => '\u{e702}',
            Self::Go => '\u{e627}',
            Self::GraphQl => '\u{f20e}',
            Self::Haskell => '\u{e61f}',
            Self::Header => '\u{f0fd}',
            Self::Html => '\u{e736}',
            Self::Image => '\u{e60d}',
            Self::Java => '\u{e738}',
            Self::JavaScript => '\u{e60c}',
            Self::Json => '\u{e60b}',
            Self::Jsx | Self::Tsx => '\u{e7ba}',
            Self::Kotlin => '\u{e634}',
            Self::License => '\u{e60a}',
            Self::Lock => '\u{e672}',
            Self::Log => '\u{f0331}',
            Self::Lua => '\u{e620}',
            Self::Makefile => '\u{e779}',
            Self::Markdown => '\u{f48a}',
            Self::Nix => '\u{f313}',
            Self::Notebook => '\u{e80f}',
            Self::Npm => '\u{e71e}',
            Self::Pdf => '\u{eaeb}',
            Self::Php => '\u{e608}',
            Self::Python => '\u{e606}',
            Self::Readme => '\u{f00ba}',
            Self::Ruby => '\u{e791}',
            Self::Sass => '\u{e603}',
            Self::Scala => '\u{e737}',
            Self::Shell => '\u{e795}',
            Self::Sql => '\u{e706}',
            Self::Svelte => '\u{e697}',
            Self::Svg => '\u{f0721}',
            Self::Swift => '\u{e755}',
            Self::Terraform => '\u{e69a}',
            Self::Text => '\u{f0219}',
            Self::Toml => '\u{e6b2}',
            Self::TypeScript | Self::TypeScriptDeclaration => '\u{e628}',
            Self::Video => '\u{e69f}',
            Self::Vue => '\u{e6a0}',
            Self::Wasm => '\u{e6a1}',
            Self::Xml => '\u{f05c0}',
            Self::Yaml => '\u{e8eb}',
            Self::Zig => '\u{e6a9}',
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_known_whole_name_beats_its_extension() {
        assert_eq!(file_type("Cargo.toml"), Some(FileType::Cargo));
        assert_eq!(file_type("README.md"), Some(FileType::Readme));
        assert_eq!(file_type("notes.md"), Some(FileType::Markdown));
        assert_eq!(file_type("Dockerfile"), Some(FileType::Docker));
    }

    #[test]
    fn names_and_extensions_ignore_case() {
        assert_eq!(file_type("makefile"), Some(FileType::Makefile));
        assert_eq!(file_type("Readme.MD"), Some(FileType::Readme));
        assert_eq!(file_type("Photo.JPG"), Some(FileType::Image));
    }

    #[test]
    fn the_last_extension_decides_unless_a_known_double_ends_the_name() {
        assert_eq!(
            file_type("types.d.ts"),
            Some(FileType::TypeScriptDeclaration)
        );
        assert_eq!(file_type("app.test.ts"), Some(FileType::TypeScript));
        assert_eq!(file_type("dump.sql.gz"), Some(FileType::Archive));
    }

    #[test]
    fn a_path_is_typed_by_its_last_component() {
        assert_eq!(file_type("crates/core/Cargo.toml"), Some(FileType::Cargo));
        assert_eq!(file_type("src/ui/mod.rs"), Some(FileType::Rust));
    }

    #[test]
    fn an_unknown_or_extension_only_name_has_no_type() {
        assert_eq!(file_type("notes.xyz"), None);
        assert_eq!(file_type("NOTICE"), None);
        assert_eq!(file_type(".rs"), None);
        assert_eq!(file_type("été"), None);
    }

    #[test]
    fn the_embedded_nerd_font_draws_every_type_glyph() {
        let data = &crate::theme::font_definitions().font_data["jetbrains-mono"];
        let face = ab_glyph::FontRef::try_from_slice(&data.font).unwrap();
        for (entry, kind) in BY_NAME.iter().chain(BY_EXTENSION) {
            let glyph = kind.glyph();
            assert_ne!(
                ab_glyph::Font::glyph_id(&face, glyph).0,
                0,
                "{entry}: U+{:04X} is missing from the Nerd Font",
                glyph as u32
            );
        }
    }
}
