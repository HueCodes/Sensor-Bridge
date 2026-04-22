//! CSV sink with pluggable row formatting.
//!
//! Writes rows as they arrive and flushes periodically, sized for the
//! common case of 100 Hz-1 kHz sensor streams. Rotation is out of scope
//! here; if you need log rotation, wrap this in an outer sink that swaps
//! the inner `CsvSink` when size/time thresholds are hit.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::error::{PipelineError, Result};

use super::Sink;

/// A value that knows how to render itself as a row of CSV fields.
///
/// Implementations should produce strings free of embedded commas and
/// newlines; the sink performs no escaping. Header is fixed for the
/// lifetime of the writer.
pub trait CsvRow {
    /// Column names written exactly once, before any rows.
    fn header() -> &'static [&'static str];

    /// One row's worth of fields, in the same order as [`CsvRow::header`].
    /// The `Vec` is returned so implementations can own the stringified
    /// fields without pulling in a formatter dependency.
    fn row(&self) -> Vec<String>;
}

/// Buffered CSV writer.
///
/// Construct with [`CsvSink::create`] for a fresh file, or
/// [`CsvSink::append`] to append to an existing file (header only
/// written if the file is empty). A call to [`Sink::write`] formats the
/// row synchronously; the underlying file is flushed on [`Sink::flush`]
/// or on drop.
pub struct CsvSink<T: CsvRow> {
    writer: BufWriter<File>,
    path: PathBuf,
    rows_written: u64,
    _marker: core::marker::PhantomData<fn(T)>,
}

impl<T: CsvRow> CsvSink<T> {
    /// Creates (or truncates) a file at `path` and writes the header.
    ///
    /// # Errors
    ///
    /// Returns an IO error if the file cannot be created or the header
    /// cannot be written.
    pub fn create<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = File::create(&path).map_err(io)?;
        let mut writer = BufWriter::new(file);
        write_header::<T>(&mut writer)?;
        Ok(Self {
            writer,
            path,
            rows_written: 0,
            _marker: core::marker::PhantomData,
        })
    }

    /// Opens `path` for append. If the file is empty the header is
    /// written; otherwise existing rows are left alone and new rows are
    /// appended.
    ///
    /// # Errors
    ///
    /// Returns an IO error if the file cannot be opened.
    pub fn append<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&path)
            .map_err(io)?;
        let len = file.metadata().map_err(io)?.len();
        let mut writer = BufWriter::new(file);
        if len == 0 {
            write_header::<T>(&mut writer)?;
        }
        Ok(Self {
            writer,
            path,
            rows_written: 0,
            _marker: core::marker::PhantomData,
        })
    }

    /// Returns the path this sink is writing to.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the number of rows written since this sink was created.
    #[must_use]
    pub const fn rows_written(&self) -> u64 {
        self.rows_written
    }
}

impl<T: CsvRow + Send> Sink<T> for CsvSink<T> {
    fn write(&mut self, item: T) -> Result<()> {
        let fields = item.row();
        let mut line = fields.join(",");
        line.push('\n');
        self.writer.write_all(line.as_bytes()).map_err(io)?;
        self.rows_written += 1;
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        self.writer.flush().map_err(io)
    }
}

fn write_header<T: CsvRow>(w: &mut BufWriter<File>) -> Result<()> {
    let joined = T::header().join(",");
    writeln!(w, "{joined}").map_err(io)
}

fn io(_e: std::io::Error) -> PipelineError {
    PipelineError::StageError("csv sink io error")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::read_to_string;

    struct Row {
        t_us: u64,
        x: f32,
    }

    impl CsvRow for Row {
        fn header() -> &'static [&'static str] {
            &["t_us", "x"]
        }
        fn row(&self) -> Vec<String> {
            vec![self.t_us.to_string(), format!("{:.3}", self.x)]
        }
    }

    #[test]
    fn writes_header_and_rows() {
        let dir = tempdir();
        let path = dir.join("out.csv");
        let mut sink = CsvSink::<Row>::create(&path).unwrap();
        sink.write(Row { t_us: 10, x: 1.5 }).unwrap();
        sink.write(Row { t_us: 20, x: -2.0 }).unwrap();
        sink.flush().unwrap();

        let contents = read_to_string(&path).unwrap();
        assert_eq!(contents, "t_us,x\n10,1.500\n20,-2.000\n");
        assert_eq!(sink.rows_written(), 2);
    }

    #[test]
    fn append_preserves_existing_rows() {
        let dir = tempdir();
        let path = dir.join("out.csv");
        {
            let mut sink = CsvSink::<Row>::create(&path).unwrap();
            sink.write(Row { t_us: 1, x: 0.5 }).unwrap();
            sink.flush().unwrap();
        }
        {
            let mut sink = CsvSink::<Row>::append(&path).unwrap();
            sink.write(Row { t_us: 2, x: 1.0 }).unwrap();
            sink.flush().unwrap();
        }
        let contents = read_to_string(&path).unwrap();
        assert_eq!(contents, "t_us,x\n1,0.500\n2,1.000\n");
    }

    fn tempdir() -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "sensor-bridge-csv-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}
