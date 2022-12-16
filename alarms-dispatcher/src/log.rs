use std::io::{Result, Write};

pub struct CombinedWriter<T, U>(T, U)
where
    T: Write + Send + Sync + 'static,
    U: Write + Send + Sync + 'static;

impl<T, U> CombinedWriter<T, U>
where
    T: Write + Send + Sync + 'static,
    U: Write + Send + Sync + 'static,
{
    pub fn new(first: T, second: U) -> Self {
        Self(first, second)
    }
}

impl<T, U> Write for CombinedWriter<T, U>
where
    T: Write + Send + Sync + 'static,
    U: Write + Send + Sync + 'static,
{
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        self.0.write(buf).and(self.1.write(buf))
    }

    fn flush(&mut self) -> Result<()> {
        self.0.flush().and(self.1.flush())
    }
}
