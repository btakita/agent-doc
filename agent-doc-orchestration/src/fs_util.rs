use anyhow::Result;
use std::path::Path;

pub fn read_optional_text(path: &Path) -> Result<Option<String>> {
    read_optional(path, |path| std::fs::read_to_string(path))
}

pub fn read_optional_bytes(path: &Path) -> Result<Option<Vec<u8>>> {
    read_optional(path, |path| std::fs::read(path))
}

fn read_optional<T, F>(path: &Path, read: F) -> Result<Option<T>>
where
    F: FnOnce(&Path) -> std::io::Result<T>,
{
    match read(path) {
        Ok(value) => Ok(Some(value)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::read_optional;
    use std::path::Path;

    #[test]
    fn read_optional_returns_none_on_not_found() {
        let value: Option<String> = read_optional(Path::new("missing"), |_| {
            Err(std::io::Error::from(std::io::ErrorKind::NotFound))
        })
        .unwrap();
        assert!(value.is_none());
    }

    #[test]
    fn read_optional_preserves_other_errors() {
        let err = read_optional::<String, _>(Path::new("denied"), |_| {
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        })
        .unwrap_err();
        let message = err.to_string().to_ascii_lowercase();
        assert!(
            message.contains("permission denied"),
            "unexpected error: {err}"
        );
    }
}
