use std::{
    fmt::Write,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

const MAX_FILE_BYTES: u64 = 256 * 1024;

pub(super) fn append(report: &mut String, path: &Path) {
    writeln!(report, "\n--- Log file: {} ---", path.display()).unwrap();
    let result = (|| -> std::io::Result<(u64, u64, Vec<u8>)> {
        let mut file = File::open(path)?;
        let length = file.metadata()?.len();
        let offset = length.saturating_sub(MAX_FILE_BYTES);
        file.seek(SeekFrom::Start(offset))?;
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES).read_to_end(&mut bytes)?;
        Ok((length, offset, bytes))
    })();
    match result {
        Ok((length, offset, bytes)) => {
            writeln!(
                report,
                "file_bytes_at_open={length} included_bytes={} omitted_prefix_bytes={offset}",
                bytes.len()
            )
            .unwrap();
            if offset != 0 {
                writeln!(report, "[Only the final {MAX_FILE_BYTES} bytes are included; the first record may be partial.]").unwrap();
            }
            if bytes.is_empty() {
                writeln!(report, "[Empty log file]").unwrap();
            }
            match String::from_utf8(bytes) {
                Ok(text) => report.push_str(&text),
                Err(error) => {
                    writeln!(
                        report,
                        "[Invalid UTF-8 bytes are displayed as replacement characters.]"
                    )
                    .unwrap();
                    report.push_str(&String::from_utf8_lossy(error.as_bytes()));
                }
            }
            report.push('\n');
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            writeln!(report, "[Log file does not exist; kind=NotFound]").unwrap();
        }
        Err(error) => {
            writeln!(
                report,
                "[Unable to read log: {error}; kind={:?}; os_error={:?}]",
                error.kind(),
                error.raw_os_error()
            )
            .unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn includes_actual_rotated_and_current_log_contents() {
        let dir = tempfile::tempdir().unwrap();
        let previous = dir.path().join("diagnostic.previous.log");
        let current = dir.path().join("diagnostic.log");
        std::fs::write(
            &previous,
            "previous native stage=LoadLibraryExW HRESULT=0x80070005\n",
        )
        .unwrap();
        std::fs::write(&current, "current entry\n错误详情：hook timeout\n").unwrap();
        let mut report = String::new();
        append(&mut report, &previous);
        append(&mut report, &current);
        assert!(report.contains("previous native stage=LoadLibraryExW HRESULT=0x80070005"));
        assert!(report.contains("错误详情：hook timeout"));
        assert!(report.contains("omitted_prefix_bytes=0"));
    }
    #[test]
    fn oversized_files_are_bounded_and_truncation_is_explicit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("diagnostic.log");
        let bytes = [
            vec![b'A'; MAX_FILE_BYTES as usize + 64],
            b"\nlast error HRESULT=0x80004005\n".to_vec(),
        ]
        .concat();
        std::fs::write(&path, &bytes).unwrap();
        let mut report = String::new();
        append(&mut report, &path);
        assert!(report.contains("Only the final 262144 bytes"));
        assert!(report.contains("last error HRESULT=0x80004005"));
        assert!(report.len() < MAX_FILE_BYTES as usize + 1024);
    }
    #[test]
    fn read_failures_and_invalid_utf8_remain_visible_in_the_report() {
        let dir = tempfile::tempdir().unwrap();
        let mut report = String::new();
        append(&mut report, &dir.path().join("missing.log"));
        assert!(report.contains("Log file does not exist"));
        append(&mut report, dir.path());
        assert!(report.contains("Unable to read log:"));
        assert!(report.contains("NotFound"));
        let invalid = dir.path().join("invalid.log");
        std::fs::write(&invalid, b"before\xffafter").unwrap();
        append(&mut report, &invalid);
        assert!(report.contains("Invalid UTF-8"));
        assert!(report.contains("before�after"));
    }
}
