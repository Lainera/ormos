#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Service is not supported `{0}`")]
    NotSupported(String),
}
