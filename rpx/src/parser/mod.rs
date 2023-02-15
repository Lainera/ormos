pub mod http;
pub mod tls;

pub trait Parser<O, E> {
    fn parse(&mut self, input: &[u8]) -> Result<Option<O>, E>;
}
