#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("unexpected end of data at offset {offset:#x} (wanted {wanted} bytes)")]
    Eof { offset: usize, wanted: usize },
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, Error>;
