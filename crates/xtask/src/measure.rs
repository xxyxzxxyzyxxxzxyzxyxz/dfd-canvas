use anyhow::{Context, Result};
use flate2::{Compression, GzBuilder};
use serde::Serialize;
use std::{fs, io::Write, path::Path};

#[derive(Debug, Serialize)]
pub struct FileSize {
    pub file: String,
    pub bytes: usize,
    pub gzip_9_bytes: usize,
}

#[derive(Debug, Serialize)]
pub struct SizeReport {
    pub files: Vec<FileSize>,
    pub total_bytes: usize,
    pub total_gzip_9_bytes: usize,
    pub note: &'static str,
}

fn gzip(data: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), Compression::best());
    encoder.write_all(data)?;
    Ok(encoder.finish()?)
}

/// Compress each top-level file separately, excluding HTTP headers and transport overhead.
pub fn measure_dist(directory: &Path) -> Result<SizeReport> {
    let mut paths = fs::read_dir(directory)
        .with_context(|| format!("Cannot read release directory {}", directory.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.sort();
    let mut files = Vec::new();
    for path in paths {
        if path.is_file() {
            let data = fs::read(&path)
                .with_context(|| format!("Cannot read release artifact {}", path.display()))?;
            files.push(FileSize {
                file: path
                    .file_name()
                    .context("Release artifact has no file name")?
                    .to_string_lossy()
                    .into_owned(),
                bytes: data.len(),
                gzip_9_bytes: gzip(&data)?.len(),
            });
        }
    }
    Ok(SizeReport {
        total_bytes: files.iter().map(|file| file.bytes).sum(),
        total_gzip_9_bytes: files.iter().map(|file| file.gzip_9_bytes).sum(),
        files,
        note: "Each file gzipped independently; excludes HTTP headers. Not measured network transfer.",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn gzip_is_reproducible_and_preserves_content() {
        let data = "圧縮テスト".repeat(100);
        let compressed = gzip(data.as_bytes()).unwrap();
        assert_eq!(compressed, gzip(data.as_bytes()).unwrap());
        assert_eq!(&compressed[4..8], &[0, 0, 0, 0]);
        let mut restored = String::new();
        flate2::read::GzDecoder::new(compressed.as_slice())
            .read_to_string(&mut restored)
            .unwrap();
        assert_eq!(restored, data);
        assert!(compressed.len() < data.len());
    }

    #[test]
    fn report_sorts_files_ignores_directories_and_totals_individual_sizes() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("z.wasm"), b"wasm contents").unwrap();
        fs::write(directory.path().join("a.css"), b"body{}").unwrap();
        fs::create_dir(directory.path().join("ignored")).unwrap();
        fs::write(directory.path().join("ignored/child"), b"not counted").unwrap();
        let report = measure_dist(directory.path()).unwrap();
        assert_eq!(report.files.len(), 2);
        assert_eq!(report.files[0].file, "a.css");
        assert_eq!(report.files[1].file, "z.wasm");
        assert_eq!(report.total_bytes, 19);
        assert_eq!(
            report.total_gzip_9_bytes,
            report
                .files
                .iter()
                .map(|file| file.gzip_9_bytes)
                .sum::<usize>()
        );
        let json = serde_json::to_value(report).unwrap();
        assert_eq!(json["files"][0]["bytes"], 6);
        assert!(json["note"].as_str().unwrap().contains("Not measured"));
        assert!(measure_dist(&directory.path().join("missing")).is_err());
    }
}
