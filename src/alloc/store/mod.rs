use std::{
    alloc::{Layout, LayoutError},
    any::TypeId,
    marker::PhantomData,
    mem::{ManuallyDrop, MaybeUninit},
    ops::Range,
    ptr::NonNull,
    slice::GetDisjointMutError,
};

use thiserror::Error;
use variadics_please::{all_tuples, all_tuples_enumerated, all_tuples_with_size};

use super::*;

mod simple;
pub use simple::*;

mod freelist;
pub use freelist::*;

mod soa;
pub use soa::*;

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
    #[error("Invalid columns layout: {0}")]
    InvalidLayout(&'static str),
    #[error("Invalid query: {0}")]
    InvalidQuery(&'static str),
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
// TODO: tie this together with Element
/// # Safety
/// This trait is responsible to construct an `Output`
/// using only raw pointers, this is inherently unsafe.
pub unsafe trait Query {
    const READONLY: bool;

    type Output<'a>
    where
        Self: 'a;
    type Cache;

    fn get<'a>(cache: &'a Self::Cache, index: Index) -> Self::Output<'a>
    where
        Self: 'a;
    fn build_cache(
        column: &mut impl FnMut(TypeId, ColumnIndex) -> Option<NonNull<u8>>,
    ) -> SResult<Self::Cache>;
}
pub enum ColumnIndex {
    Index(u8),
    Next,
}
// HACK: 'static constraint needed because of TypeId
unsafe impl<T: 'static> Query for &T {
    const READONLY: bool = true;

    type Output<'a>
        = &'a T
    where
        Self: 'a;
    type Cache = NonNull<T>;

    fn get<'a>(cache: &'a Self::Cache, index: Index) -> Self::Output<'a>
    where
        Self: 'a,
    {
        // SAFETY: cache is a column of type T
        unsafe { cache.add(index.get() as usize).as_ref() }
    }

    fn build_cache(
        get_column: &mut impl FnMut(TypeId, ColumnIndex) -> Option<NonNull<u8>>,
    ) -> SResult<Self::Cache> {
        let ptr = get_column(TypeId::of::<T>(), ColumnIndex::Next)
            .ok_or(StoreError::InvalidQuery("column not found"))?;
        Ok(ptr.cast())
    }
}
// HACK: 'static constraint needed because of TypeId
unsafe impl<T: 'static> Query for &mut T {
    const READONLY: bool = false;

    type Output<'a>
        = &'a mut T
    where
        Self: 'a;
    type Cache = NonNull<T>;

    fn get<'a>(cache: &'a Self::Cache, index: Index) -> Self::Output<'a>
    where
        Self: 'a,
    {
        // SAFETY: cache is a column of type T
        unsafe { cache.add(index.get() as usize).as_mut() }
    }

    fn build_cache(
        array: &mut impl FnMut(TypeId, ColumnIndex) -> Option<NonNull<u8>>,
    ) -> SResult<Self::Cache> {
        let ptr = array(TypeId::of::<T>(), ColumnIndex::Next)
            .ok_or(StoreError::InvalidQuery("column not found"))?;
        Ok(ptr.cast())
    }
}
// TODO: make a Nth{Mut}<N, T> type that accesses ColumnIndex::Index(N)
// TODO: make a Bits{Mut}<N> type that accesses bits of a BitArray (use AtomicU8 as backing type)
macro_rules! impl_query {
    ($(($T:ident, $c:ident)),*) => {
        unsafe impl<$($T: Query),*> Query for ($($T,)*) {
            #![allow(unused_variables,  clippy::unused_unit, reason = "needed for the 0-tuple case")]

            const READONLY: bool = true $(&& $T::READONLY)*;

            type Output<'a>
                = ($($T::Output<'a>,)*)
            where
                Self: 'a;
            type Cache = ($($T::Cache,)*);

            fn get<'a>(($($c,)*): &'a Self::Cache, index: Index) -> Self::Output<'a>
            where
                Self: 'a
            {
                ($($T::get($c, index),)*)
            }
            fn build_cache(
                array: &mut impl FnMut(TypeId, ColumnIndex) -> Option<NonNull<u8>>,
            ) -> SResult<Self::Cache> {
                Ok(($($T::build_cache(array)?,)*))
            }
        }
    };
}
all_tuples!(impl_query, 0, 16, T, c);

#[derive(Debug)]
pub struct QueryResult<'a, Q: Query>(Q::Cache, PhantomData<&'a Q::Cache>);
impl<'a, Q: Query> QueryResult<'a, Q> {
    pub fn get(&self, index: Index) -> Q::Output<'_> {
        Q::get(&self.0, index)
    }
}
pub trait Queryable {
    // TODO: constrain with `READONLY = true` once stable
    fn get<Q: Query>(&self) -> SResult<QueryResult<Q>>;
    fn get_mut<Q: Query>(&mut self) -> SResult<QueryResult<Q>>;
}

// TODO: these marker traits should be automatically implemented for all applicable types
// - convert to trait alias, or
// - use auto traits
pub trait Store<T>: Get<Single<T>> + Insert<Single<T>> + Resizable {}
pub trait ReusableStore<T>: Store<T> + Remove<Single<T>> {}
pub trait MultiStore<T>: Get<Multi<T>> + InsertIndirect<Multi<T>> + Resizable {}
pub trait ReusableMultiStore<T>: MultiStore<T> + RemoveIndirect<Multi<T>> {}
pub trait SoAStore<C: Columns>: Queryable + Insert<Single<C>> + Resizable {}
pub trait ReusableSoAStore<C: Columns>: SoAStore<C> + Remove<Single<C>> {}

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
        // SAFETY: user is responsible to initialize the data
        unsafe { self.data.set_len(self.data.len() + self.len) };
    }
}

/// # Safety
/// This trait is responsible to register its own memory layout
/// and move values in and out of a store
/// using only raw pointers, this is inherently unsafe.
pub unsafe trait Columns: Sized {
    const COUNT: usize;

    /// Registers each column with the store.
    /// `register` will be called exactly `COUNT` times.
    fn register_layout(
        count: Length,
        register: &mut impl FnMut(TypeId, Layout),
    ) -> Result<(), LayoutError>;
    /// Moves itself to memory addresses provided by `next_column`.
    /// `next_column` will be called exactly `COUNT` times.
    fn move_into(self, index: Index, next_column: &mut impl FnMut() -> NonNull<u8>);
    /// Loads itself from memory addresses provided by `next_column`.
    /// `next_column` will be called exactly `COUNT` times.
    fn take(index: Index, next_column: &mut impl FnMut() -> NonNull<u8>) -> Self;
    /// Return reference to n-th row, as a freelist entry.
    /// `get_column` will only be called with values `0..COUNT`.
    fn as_freelist_entry(
        index: Index,
        get_column: &mut impl FnMut(usize) -> NonNull<u8>,
    ) -> &mut Option<Index>;
}
pub union FreelistEntry<T> {
    data: ManuallyDrop<T>,
    next: Option<Index>,
}
macro_rules! impl_columns {
    ($N:expr, ($T0:ident, $t0:ident) $(, ($T:ident, $t:ident))*) => {
        // HACK: 'static constraint needed because of TypeId
        unsafe impl<$T0: 'static, $($T: 'static),*> Columns for ($T0, $($T,)*) {
            const COUNT: usize = $N;

            fn register_layout(
                count: Length,
                register: &mut impl FnMut(TypeId, Layout),
            ) -> Result<(), LayoutError> {
                register(
                    TypeId::of::<$T0>(),
                    Layout::from_size_align(
                        count as usize * size_of::<FreelistEntry<$T0>>(),
                        align_of::<FreelistEntry<$T0>>()
                    )?
                );
                $(register(
                    TypeId::of::<$T>(),
                    Layout::from_size_align(count as usize * size_of::<$T>(), align_of::<$T>())?
                );)*
                Ok(())
            }
            fn move_into(
                self,
                index: Index,
                next_column: &mut impl FnMut() -> NonNull<u8>,
            ) {
                let ($t0, $($t,)*) = self;
                {
                    let column = next_column();
                    // SAFETY: column was registered to be of type FreelistEntry<$T0>
                    unsafe {
                        column.cast::<FreelistEntry<$T0>>().add(index.get() as usize)
                            .write(FreelistEntry { data: ManuallyDrop::new($t0) })
                    };
                }
                $({
                    let column = next_column();
                    // SAFETY: column was registered to be of type $T
                    unsafe { column.cast::<$T>().add(index.get() as usize).write($t) };
                })*
            }
            fn take(index: Index, next_column: &mut impl FnMut() -> NonNull<u8>) -> Self {
                (
                    {
                        let column = next_column();
                        // SAFETY: column was registered to be of type FreelistEntry<$T0>
                        unsafe {
                            ManuallyDrop::take(&mut column.cast::<FreelistEntry<$T0>>()
                                .add(index.get() as usize).read().data)
                        }
                    },
                    $({
                        let column = next_column();
                        // SAFETY: column was registered to be of type $T
                        unsafe { column.cast::<$T>().add(index.get() as usize).read() }
                    },)*
                )
            }
            fn as_freelist_entry(
                index: Index,
                get_column: &mut impl FnMut(usize) -> NonNull<u8>,
            ) -> &mut Option<Index> {
                let column = get_column(0);
                // SAFETY: column was registered to be of type FreelistEntry<$T0>
                unsafe {
                    &mut column.cast::<FreelistEntry<$T0>>()
                        .add(index.get() as usize).as_mut().next
                }
            }
        }
    };
}
all_tuples_with_size!(impl_columns, 1, 16, T, t);
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Join<T>(pub T);
pub type Prefix<T, C> = Join<((T,), C)>;
impl<T, C> Prefix<T, C> {
    pub fn new(prefix: T, rest: C) -> Self {
        Self(((prefix,), rest))
    }
}
macro_rules! impl_join {
    ((0, $T0:ident) $(,($i:tt, $T:ident))*) => {

        unsafe impl<$T0: Columns, $($T: Columns),*> Columns for Join<($T0, $($T,)*)>
        {
            const COUNT: usize = $T0::COUNT $(+ $T::COUNT)*;

            fn register_layout(
                count: Length,
                register: &mut impl FnMut(TypeId, Layout),
            ) -> Result<(), LayoutError> {
                $T0::register_layout(count, register)?;
                $($T::register_layout(count, register)?;)*
                Ok(())
            }

            fn move_into(
                self,
                index: Index,
                next_column: &mut impl FnMut() -> NonNull<u8>,
            ) {
                self.0.0.move_into(index, next_column);
                $(self.0.$i.move_into(index, next_column);)*
            }

            fn take(index: Index, next_column: &mut impl FnMut() -> NonNull<u8>) -> Self {
                Self((
                    $T0::take(index, next_column),
                    $($T::take(index, next_column),)*
                ))
            }

            fn as_freelist_entry(
                index: Index,
                get_column: &mut impl FnMut(usize) -> NonNull<u8>,
            ) -> &mut Option<Index> {
                $T0::as_freelist_entry(index, get_column)
            }
        }
    };
}
all_tuples_enumerated!(impl_join, 1, 16, T);
