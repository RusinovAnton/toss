//! Turning a sender's `fileName` into a path we are willing to write to.
//!
//! `fileName` is attacker-controlled. It may legitimately carry a relative
//! path (folder sends put `photos/cat.png` there), so it cannot simply be
//! flattened, but it must never escape the download directory.

use std::fmt;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub enum PathError {
    Empty,
    Traversal,
    Absolute,
    ReservedName,
    InvalidCharacter,
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let reason = match self {
            PathError::Empty => "empty file name",
            PathError::Traversal => "file name escapes the download directory",
            PathError::Absolute => "absolute file names are not accepted",
            PathError::ReservedName => "reserved device name",
            PathError::InvalidCharacter => "invalid character in file name",
        };
        f.write_str(reason)
    }
}

/// Names Windows refuses to treat as ordinary files. Rejected on every
/// platform so behaviour does not differ between them.
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Validates `file_name` and returns it as a relative path.
///
/// Both `/` and `\` count as separators: a Windows-style `..\..\secret` must
/// not slip through on a Unix receiver.
pub fn safe_relative_path(file_name: &str) -> Result<PathBuf, PathError> {
    if file_name.is_empty() {
        return Err(PathError::Empty);
    }
    if file_name.contains('\0') {
        return Err(PathError::InvalidCharacter);
    }
    if file_name.starts_with('/') || file_name.starts_with('\\') {
        return Err(PathError::Absolute);
    }

    let mut path = PathBuf::new();
    for raw in file_name.split(['/', '\\']) {
        // Repeated or trailing separators are collapsed rather than rejected;
        // `a//b` is a sloppy sender, not an attack.
        if raw.is_empty() || raw == "." {
            continue;
        }
        if raw == ".." {
            return Err(PathError::Traversal);
        }
        // A Windows drive letter or an alternate data stream.
        if raw.contains(':') {
            return Err(PathError::Absolute);
        }
        let stem = raw.split('.').next().unwrap_or(raw);
        if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
            return Err(PathError::ReservedName);
        }
        path.push(raw);
    }

    if path.as_os_str().is_empty() {
        return Err(PathError::Empty);
    }
    // Belt and braces: whatever the loop produced must still be relative and
    // contain nothing but plain names.
    if path
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(PathError::Traversal);
    }
    Ok(path)
}

/// Splits a file name into the part a collision suffix goes after, and the
/// extension. `archive.tar.gz` keeps only `.gz` as extension, which matches
/// what Finder and Explorer do.
fn split_extension(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        // A leading dot is part of the name (`.gitignore`), not an extension.
        Some(index) if index > 0 => (&name[..index], &name[index..]),
        _ => (name, ""),
    }
}

/// The name to try after `attempt` collisions: `cat.png`, `cat (1).png`, ...
pub fn candidate_name(name: &str, attempt: u32) -> String {
    if attempt == 0 {
        return name.to_string();
    }
    let (stem, extension) = split_extension(name);
    format!("{stem} ({attempt}){extension}")
}

/// Whether `path` stays inside `base` once `..` and `.` are resolved
/// textually. Used as a last check before creating anything.
pub fn is_inside(base: &Path, path: &Path) -> bool {
    let mut resolved = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                if !resolved.pop() {
                    return false;
                }
            }
            Component::CurDir => {}
            other => resolved.push(other.as_os_str()),
        }
    }
    resolved.starts_with(base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_names_are_accepted() {
        assert_eq!(safe_relative_path("cat.png").unwrap(), PathBuf::from("cat.png"));
    }

    #[test]
    fn relative_folders_are_kept_for_folder_sends() {
        assert_eq!(
            safe_relative_path("photos/2024/cat.png").unwrap(),
            PathBuf::from("photos/2024/cat.png")
        );
    }

    #[test]
    fn parent_directory_traversal_is_rejected() {
        assert_eq!(safe_relative_path("../x").unwrap_err(), PathError::Traversal);
        assert_eq!(
            safe_relative_path("a/../../etc/passwd").unwrap_err(),
            PathError::Traversal
        );
        assert_eq!(
            safe_relative_path("photos/../../../x").unwrap_err(),
            PathError::Traversal
        );
    }

    #[test]
    fn windows_separators_cannot_smuggle_traversal() {
        assert_eq!(
            safe_relative_path("..\\..\\secret.txt").unwrap_err(),
            PathError::Traversal
        );
        assert_eq!(
            safe_relative_path("a\\..\\..\\b").unwrap_err(),
            PathError::Traversal
        );
    }

    #[test]
    fn absolute_paths_are_rejected() {
        assert_eq!(safe_relative_path("/etc/passwd").unwrap_err(), PathError::Absolute);
        assert_eq!(
            safe_relative_path("\\\\server\\share\\x").unwrap_err(),
            PathError::Absolute
        );
        assert_eq!(safe_relative_path("C:\\Windows\\x").unwrap_err(), PathError::Absolute);
    }

    #[test]
    fn empty_and_dot_only_names_are_rejected() {
        assert_eq!(safe_relative_path("").unwrap_err(), PathError::Empty);
        assert_eq!(safe_relative_path(".").unwrap_err(), PathError::Empty);
        assert_eq!(safe_relative_path("./").unwrap_err(), PathError::Empty);
    }

    #[test]
    fn null_bytes_are_rejected() {
        assert_eq!(
            safe_relative_path("ca\0t.png").unwrap_err(),
            PathError::InvalidCharacter
        );
    }

    #[test]
    fn reserved_windows_names_are_rejected() {
        assert_eq!(safe_relative_path("NUL").unwrap_err(), PathError::ReservedName);
        assert_eq!(safe_relative_path("com1.txt").unwrap_err(), PathError::ReservedName);
        assert!(safe_relative_path("console.txt").is_ok());
    }

    #[test]
    fn redundant_separators_are_collapsed() {
        assert_eq!(
            safe_relative_path("a//b/./c.txt").unwrap(),
            PathBuf::from("a/b/c.txt")
        );
    }

    #[test]
    fn collision_suffix_goes_before_the_extension() {
        assert_eq!(candidate_name("cat.png", 0), "cat.png");
        assert_eq!(candidate_name("cat.png", 1), "cat (1).png");
        assert_eq!(candidate_name("cat.png", 2), "cat (2).png");
        assert_eq!(candidate_name("notes", 1), "notes (1)");
        assert_eq!(candidate_name("archive.tar.gz", 1), "archive.tar (1).gz");
        assert_eq!(candidate_name(".gitignore", 1), ".gitignore (1)");
    }

    #[test]
    fn inside_check_resolves_dot_dot() {
        let base = Path::new("/downloads");
        assert!(is_inside(base, Path::new("/downloads/a/b.txt")));
        assert!(!is_inside(base, Path::new("/downloads/../etc/passwd")));
        assert!(!is_inside(base, Path::new("/elsewhere/x")));
    }
}
