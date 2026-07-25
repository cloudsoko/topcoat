use ref_cast::RefCast;
use serde::Serialize;

use crate::{Signal, StrSurrogate, Surrogated, impl_surrogate, impl_surrogate_mut, impl_surrogate_ref};

#[derive(RefCast)]
#[repr(transparent)]
pub struct SignalSurrogate<T>(Signal<T>);

impl<T> SignalSurrogate<T> {
    #[inline]
    pub(crate) const fn new(v: Signal<T>) -> Self {
        Self(v)
    }
}

impl<T> SignalSurrogate<T>
where
    for<'b> &'b T: Surrogated,
{
    pub fn read(&self) -> <&T as Surrogated>::Surrogate {
        self.0.read().into_surrogate()
    }
}

impl<T> SignalSurrogate<T>
where
    T: Surrogated + Clone,
{
    pub fn get(&self) -> <T as Surrogated>::Surrogate {
        self.0.get().into_surrogate()
    }
}

impl<T> SignalSurrogate<T>
where
    T: Surrogated,
{
    /// Writes a new value to the signal.
    ///
    /// # Panics
    ///
    /// Always panics; signal writes can only occur in client-side expressions.
    pub fn set(&self, _v: T::Surrogate) {
        panic!("expressions in which a signal is written to cannot be run server-side");
    }
}

impl SignalSurrogate<bool> {
    /// Flips the signal's value.
    ///
    /// # Panics
    ///
    /// Always panics; signal writes can only occur in client-side expressions.
    pub fn toggle(&self) {
        panic!("expressions in which a signal is written to cannot be run server-side");
    }
}

impl SignalSurrogate<f64> {
    /// Adds `1` to the signal's value.
    ///
    /// # Panics
    ///
    /// Always panics; signal writes can only occur in client-side expressions.
    pub fn increment(&self) {
        panic!("expressions in which a signal is written to cannot be run server-side");
    }

    /// Subtracts `1` from the signal's value.
    ///
    /// # Panics
    ///
    /// Always panics; signal writes can only occur in client-side expressions.
    pub fn decrement(&self) {
        panic!("expressions in which a signal is written to cannot be run server-side");
    }
}

impl SignalSurrogate<std::string::String> {
    /// Appends a string to the signal's value.
    ///
    /// # Panics
    ///
    /// Always panics; signal writes can only occur in client-side expressions.
    pub fn push_str(&self, _v: impl core::ops::Deref<Target = StrSurrogate>) {
        panic!("expressions in which a signal is written to cannot be run server-side");
    }
}

impl_surrogate!({T} Signal<T>, SignalSurrogate<T>);
impl_surrogate_ref!({T} Signal<T>, SignalSurrogate<T>);
impl_surrogate_mut!({T} Signal<T>, SignalSurrogate<T>);

impl<T> Serialize for SignalSurrogate<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        #[derive(Serialize)]
        struct TaggedSignal {
            t: &'static str,
            id: std::string::String,
        }

        TaggedSignal {
            t: "Signal",
            id: self.0.id().to_string(),
        }
        .serialize(serializer)
    }
}
