use std::{marker::PhantomData, mem::MaybeUninit, ops::Range, slice::GetDisjointMutError};

use thiserror::Error;
use variadics_please::{all_tuples_enumerated, all_tuples_with_size};

use super::*;
use crate::internal::Sealed;

mod simple;
pub use simple::*;

mod freelist;
pub use freelist::*;

mod masked;
pub use masked::*;

mod intervaltree;
pub use intervaltree::*;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum StoreError {
    #[error("Tried to access data at index {0} when length was {1}.")]
    OutOfBounds(Index, Length),
    #[error("Tried to access previously freed data at index {0}.")]
    AccessAfterFree(Index),
    #[error("Tried to free already freed data at index {0}.")]
    DoubleFree(Index),
    #[error("Cannot resize from {0} to {1}, collection too large.")]
    OutofMemory(Length, Length),
    #[error("Disjoint Error: {0}")]
    DisjointError(#[from] GetDisjointMutError),
    #[error("New capacity {0} is smaller then current capacity {1}")]
    Narrow(Length, Length),
}
pub type SResult<T> = Result<T, StoreError>;

pub trait Element {
    type Index;

    type Val;
    type Ref<'a>
    where
        Self: 'a;
    type Mut<'a>
    where
        Self: 'a;
}
pub struct Single<T>(PhantomData<T>);
impl<T> Element for Single<T> {
    type Index = Index;

    type Val = T;
    type Ref<'a>
        = &'a T
    where
        Self: 'a;
    type Mut<'a>
        = &'a mut T
    where
        Self: 'a;
}
pub struct Multi<T>(PhantomData<T>);
impl<T> Element for Multi<T> {
    type Index = Range<Index>;

    type Val = T;
    type Ref<'a>
        = &'a [T]
    where
        Self: 'a;
    type Mut<'a>
        = &'a mut [T]
    where
        Self: 'a;
}
pub struct Masked<M>(PhantomData<M>);
impl<M: Maskable> Element for Masked<M> {
    type Index = (Index, Mask);

    type Val = M;
    type Ref<'a>
        = M::Ref<'a>
    where
        Self: 'a;
    type Mut<'a>
        = M::Mut<'a>
    where
        Self: 'a;
}
pub trait Get<E: Element> {
    fn get(&self, index: E::Index) -> SResult<E::Ref<'_>>;
    fn get_mut(&mut self, index: E::Index) -> SResult<E::Mut<'_>>;
}
pub trait GetDisjointMut<E: Element> {
    fn get_disjoint_mut<const N: usize>(
        &mut self,
        indices: [E::Index; N],
    ) -> SResult<[E::Mut<'_>; N]>;
    /// # Safety
    /// Does not perform any checks on the indices.
    unsafe fn get_disjoint_unchecked_mut<const N: usize>(
        &mut self,
        indices: [E::Index; N],
    ) -> [E::Mut<'_>; N];
}
pub trait Insert<E: Element> {
    fn insert_within_capacity(&mut self, element: E::Val) -> Result<E::Index, E::Val>;
}
pub trait InsertIndirect<E: Element> {
    type Guard<'a>: AsMut<[MaybeUninit<E::Val>]>
    where
        Self: 'a,
        E: 'a;
    fn insert_indirect_within_capacity(
        &mut self,
        size: Length,
    ) -> Option<(E::Index, Self::Guard<'_>)>;
}
pub trait Remove<E: Element> {
    fn remove(&mut self, index: E::Index) -> SResult<E::Val>;
}
pub trait RemoveIndirect<E: Element> {
    type Guard<'a>: AsRef<E::Ref<'a>>
    where
        Self: 'a,
        E: 'a;
    fn remove_indirect(&mut self, index: E::Index) -> SResult<Self::Guard<'_>>;
}
pub trait Resizable {
    fn capacity(&self) -> Length;

    fn widen(&mut self, new_capacity: Length) -> SResult<()>;
    fn clear(&mut self);
}

// TODO: these marker traits should be automatically implemented for all applicable types
// - convert to trait alias, or
// - use auto traits
pub trait Store<T>: Get<Single<T>> + Insert<Single<T>> + Resizable {}
pub trait ReusableStore<T>: Store<T> + Remove<Single<T>> {}
pub trait MultiStore<T>: Get<Multi<T>> + InsertIndirect<Multi<T>> + Resizable {}
pub trait ReusableMultiStore<T>: MultiStore<T> + RemoveIndirect<Multi<T>> {}
pub trait MaskedStore<M: Maskable>: Get<Masked<M>> + Insert<Single<M>> + Resizable {}
pub trait ReusableMaskedStore<M: Maskable>: MaskedStore<M> + Remove<Single<M>> {}

#[derive(Debug)]
pub struct InsertIndirectGuard<'a, T> {
    data: &'a mut Vec<T>,
    len:  usize,
}
impl<T> AsMut<[MaybeUninit<T>]> for InsertIndirectGuard<'_, T> {
    fn as_mut(&mut self) -> &mut [MaybeUninit<T>] {
        &mut self.data.spare_capacity_mut()[0..self.len]
    }
}
impl<T> Drop for InsertIndirectGuard<'_, T> {
    fn drop(&mut self) {
        unsafe { self.data.set_len(self.data.len() + self.len) };
    }
}

pub trait Wrapper {
    type Wrap<'a, T>
    where
        T: 'a;
}
#[macro_export]
macro_rules! wrapper {
    { $vis:vis type &$a:lifetime $T:ident; $($rest:tt)* } => {
        wrapper!{$vis, $a, $T @ $($rest)* }
    };
    {$vis:vis, $a:lifetime, $T:ident @
        $name:ident = $expr:ty;
        $($rest:tt)*
    } => {
        $vis struct $name;
        impl $crate::alloc::store::Wrapper for $name {
            type Wrap<$a, $T> = $expr where $T: $a;
        }
        wrapper!{$vis, $a, $T @ $($rest)* }
    };
    {$vis:vis, $a:lifetime, $T:ident @} => {}
}
pub use wrapper;
pub mod wrap {
    wrapper! {
        pub type &'a T;

        Ref = &'a T;
        Mut = &'a mut T;
        Opt = Option<T>;
        OptRef = Option<&'a T>;
        OptMut = Option<&'a mut T>;
    }
}

pub trait Tuple: Sealed {
    const LEN: usize;

    type Wrapped<'a, W: Wrapper>
    where
        Self: 'a;
}
macro_rules! impl_tuple {
    ($N:tt, $($T:ident),*) => {
        impl<$($T),*> Sealed for ($($T,)*) {}
        impl<$($T),*> Tuple for ($($T,)*) {
            const LEN: usize = $N;

            type Wrapped<'a, W: Wrapper> = ($(W::Wrap<'a, $T>,)*)
            where
                Self: 'a;
        }
    };
}
all_tuples_with_size!(impl_tuple, 0, 16, T);

pub type Mask = u16;
pub trait Maskable: From<Self::Tuple> {
    type Tuple: Tuple + From<Self>;

    type Ref<'a>: From<<Self::Tuple as Tuple>::Wrapped<'a, wrap::OptRef>>
    where
        Self: 'a;
    type Mut<'a>: From<<Self::Tuple as Tuple>::Wrapped<'a, wrap::OptMut>>
    where
        Self: 'a;
}
impl<T: Tuple> Maskable for T {
    type Tuple = T;

    type Ref<'a>
        = T::Wrapped<'a, wrap::OptRef>
    where
        Self: 'a;
    type Mut<'a>
        = T::Wrapped<'a, wrap::OptMut>
    where
        Self: 'a;
}
// HACK: Inner is needed to distinguish the differennt impl blocks
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Prefix<T, M: Maskable, Inner = <M as Maskable>::Tuple>(T, M, PhantomData<Inner>);
macro_rules! impl_prefix {
    ($(($i:tt, $T:ident, $t:ident)),*) => {
        impl<T, M, $($T),*> Prefix<T, M, ($($T,)*)>
        where
            M: Maskable<Tuple = ($($T,)*)>,
        {
            pub fn new(prefix: T, rest: M) -> Self {
                Self(prefix, rest, PhantomData)
            }
            pub fn prefix(&self) -> &T {
                &self.0
            }
            pub fn prefix_mut(&mut self) -> &mut T {
                &mut self.0
            }
            pub fn rest(&self) -> &M {
                &self.1
            }
            pub fn rest_mut(&mut self) -> &mut M {
                &mut self.1
            }
            pub fn into_parts(self) -> (T, M) {
                (self.0, self.1)
            }
        }
        impl<T, M, $($T),*> Maskable for Prefix<T, M, ($($T,)*)>
        where
            M: Maskable<Tuple = ($($T,)*)> + From<($($T,)*)> + Into<($($T,)*)>,
        {
            type Tuple = (T, $($T,)*);

            type Ref<'a>
                = (Option<&'a T>, $(Option<&'a $T>,)*)
            where
                Self: 'a;
            type Mut<'a>
                = (Option<&'a mut T>, $(Option<&'a mut $T>,)*)
            where
                Self: 'a;
        }
        impl<T, M, $($T),*> From<(T, $($T,)*)> for Prefix<T, M, ($($T,)*)>
        where
            M: Maskable + From<($($T,)*)>,
        {
            fn from((t, $($t,)*): (T, $($T,)*)) -> Self {
                Self(t, M::from(($($t,)*)), PhantomData)
            }
        }
        impl<T, M, $($T),*> From<Prefix<T, M, ($($T,)*)>> for (T, $($T,)*)
        where
            M: Maskable + Into<($($T,)*)>,
        {
            fn from(value: Prefix<T, M, ($($T,)*)>) -> Self {
                // NOTE: needed for the M::LEN == 0 case
                #[allow(unused_variables)]
                let data: ($($T,)*) = value.1.into();
                (value.0, $(data.$i,)*)
            }
        }
    };
}
all_tuples_enumerated!(impl_prefix, 0, 15, T, t);
