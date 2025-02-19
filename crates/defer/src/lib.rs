#[derive(Clone)]
#[must_use]
pub struct Defer<T, F>
where
    T: ?Sized,
    F: FnMut(&mut T),
{
    deferred: F,
    value: T,
}

impl<T, F> Defer<T, F>
where
    F: FnMut(&mut T),
{
    pub const fn new(value: T, deferred: F) -> Self {
        Self { deferred, value }
    }
}

impl<T, F> Defer<T, F>
where
    T: Copy,
    F: FnMut(&mut T) + Copy,
{
    pub const fn copied(&self) -> Self {
        Self { ..*self }
    }
}

impl<T, F> AsRef<T> for Defer<T, F>
where
    T: ?Sized,
    F: FnMut(&mut T),
{
    fn as_ref(&self) -> &T {
        &self.value
    }
}

impl<T, F> AsMut<T> for Defer<T, F>
where
    T: ?Sized,
    F: FnMut(&mut T),
{
    fn as_mut(&mut self) -> &mut T {
        &mut self.value
    }
}

impl<T, F> Drop for Defer<T, F>
where
    T: ?Sized,
    F: FnMut(&mut T),
{
    fn drop(&mut self) {
        () = (self.deferred)(&mut self.value);
    }
}
