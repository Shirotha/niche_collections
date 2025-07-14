use std::{
    alloc::{Layout, LayoutError},
    marker::PhantomData,
    mem::{ManuallyDrop, MaybeUninit},
    ops::Range,
    ptr::NonNull,
    slice::GetDisjointMutError,
};

use paste::paste;
use thiserror::Error;
use variadics_please::all_tuples_enumerated;

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
pub struct Rows<C, I>(PhantomData<fn(I) -> C>);
impl<C, I> Element for Rows<C, I>
where
    C: Columns,
    I: IntoIndex,
{
    type Index = I;

    type Val = C;
    type Ref<'a>
        = C::Ref<'a, I>
    where
        Self: 'a;
    type Mut<'a>
        = C::Mut<'a, I>
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
pub trait View<E: Element> {
    fn view(&self) -> E::Ref<'_>;
    fn view_mut(&mut self) -> E::Mut<'_>;
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
pub trait SoAStore<C: Columns, I: IntoIndex>:
    View<Rows<C, I>> + Insert<Single<C>> + Resizable
{
}
pub trait ReusableSoAStore<C: Columns, I: IntoIndex>: SoAStore<C, I> + Remove<Single<C>> {}

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

pub trait IntoIndex {
    fn into_index(self) -> Index;
}
impl IntoIndex for Index {
    fn into_index(self) -> Index {
        self
    }
}
/// # Safety
/// This trait is responsible to register its own memory layout
/// and move values in and out of a store
/// using only raw pointers, this is inherently unsafe.
// TODO: change columns slice to COUNT sized array once stable
pub unsafe trait Columns: Sized {
    const COUNT: usize;

    type Ref<'a, I>
    where
        I: IntoIndex + 'a,
        Self: 'a;
    type Mut<'a, I>
    where
        I: IntoIndex + 'a,
        Self: 'a;

    /// Registers each column with the store.
    /// `register` will be called exactly `COUNT` times.
    fn register_layout(rows: Length, register: &mut impl FnMut(Layout)) -> Result<(), LayoutError>;
    /// Moves itself to memory addresses provided by `next_column`.
    /// `next_column` will be called exactly `COUNT` times.
    fn move_into(self, index: Index, columns: &[NonNull<u8>]);
    /// Loads itself from memory addresses provided by `next_column`.
    /// `next_column` will be called exactly `COUNT` times.
    fn take(index: Index, columns: &[NonNull<u8>]) -> Self;
    /// Return reference to n-th row, as a freelist entry.
    /// `get_column` will only be called with values `0..COUNT`.
    #[expect(clippy::mut_from_ref, reason = "trait user is responsible for this")]
    fn as_freelist_entry(index: Index, columns: &[NonNull<u8>]) -> &mut Option<Index>;
    /// Create Ref object to act as an accessor.
    /// This will be called inside of Deref, so this should be cheap.
    fn make_ref<'a, I>(columns: &'a [NonNull<u8>], occupation_ptr: NonNull<u8>) -> Self::Ref<'a, I>
    where
        I: IntoIndex,
        Self: 'a;
    /// Create Mut object to act as a mutable accessor.
    /// This will be called inside of DerefMut, so this should be cheap.
    fn make_mut<'a, I>(columns: &'a [NonNull<u8>], occupation_ptr: NonNull<u8>) -> Self::Mut<'a, I>
    where
        I: IntoIndex,
        Self: 'a;
}
pub union FreelistEntry<T> {
    _data: ManuallyDrop<T>,
    _next: Option<Index>,
}
/// # Safety
/// `occupation_ptr` is not checked,
/// only a pointer passed to `Columns::make_ref` or `Columns::make_mut` should be used.
pub const unsafe fn validate_row_index(occupation_ptr: NonNull<u8>, index: Index) -> SResult<()> {
    // SAFETY: if index is in capacity, then chunk is a valid part of the header
    let chunk = unsafe { occupation_ptr.add(index.get() as usize / 8).read() };
    if chunk >> (index.get() % 8) & 1 == 0 {
        Err(StoreError::AccessAfterFree(index))
    } else {
        Ok(())
    }
}
pub struct TupleRef<'a, T, I>(&'a [NonNull<u8>], NonNull<u8>, PhantomData<fn(I) -> &'a T>);
pub struct TupleMut<'a, T, I>(&'a [NonNull<u8>], NonNull<u8>, PhantomData<fn(I) -> &'a mut T>);
macro_rules! impl_columns {
    ((0, $T0:ident, $t0:ident) $(, ($i:tt, $T:ident, $t:ident))*) => { paste! {
        impl<'a, $T0, $($T,)* I> TupleRef<'a, ($T0, $($T,)*), I>
        where
            I: IntoIndex,
        {
            pub fn col0(&self, index: I) -> SResult<&$T0> {
                let index = index.into_index();
                // SAFETY: self.1 is a valid pointer ot an occupation table
                unsafe { validate_row_index(self.1, index)? };
                Ok(unsafe {
                    self.0[0].cast::<FreelistEntry<$T0>>().add(index.get() as usize)
                        .cast::<$T0>().as_ref()
                })
            }
            pub fn into_col0(self, index: I) -> SResult<&'a $T0> {
                let index = index.into_index();
                // SAFETY: self.1 is a valid pointer ot an occupation table
                unsafe { validate_row_index(self.1, index)? };
                Ok(unsafe {
                    self.0[0].cast::<FreelistEntry<$T0>>().add(index.get() as usize)
                        .cast::<$T0>().as_ref()
                })
            }
            $(
                pub fn [<col $i>](&self, index: I) -> SResult<&$T> {
                    let index = index.into_index();
                    // SAFETY: self.1 is a valid pointer ot an occupation table
                    unsafe { validate_row_index(self.1, index)? };
                    Ok(unsafe {
                        self.0[$i].cast::<$T>().add(index.get() as usize).as_ref()
                    })
                }
                pub fn [<into_col $i>](self, index: I) -> SResult<&'a $T> {
                    let index = index.into_index();
                    // SAFETY: self.1 is a valid pointer ot an occupation table
                    unsafe { validate_row_index(self.1, index)? };
                    Ok(unsafe {
                        self.0[$i].cast::<$T>().add(index.get() as usize).as_ref()
                    })
                }
            )*
        }
        impl<'a, $T0, $($T,)* I> TupleMut<'a, ($T0, $($T,)*), I>
        where
            I: IntoIndex,
        {
            pub fn col0(&self, index: I) -> SResult<&$T0> {
                let index = index.into_index();
                // SAFETY: self.1 is a valid pointer ot an occupation table
                unsafe { validate_row_index(self.1, index)? };
                Ok(unsafe {
                    self.0[0].cast::<FreelistEntry<$T0>>().add(index.get() as usize)
                        .cast::<$T0>().as_ref()
                })
            }
            pub fn col0_mut(&mut self, index: I) -> SResult<&mut $T0> {
                let index = index.into_index();
                // SAFETY: self.1 is a valid pointer ot an occupation table
                unsafe { validate_row_index(self.1, index)? };
                Ok(unsafe {
                    self.0[0].cast::<FreelistEntry<$T0>>().add(index.get() as usize)
                        .cast::<$T0>().as_mut()
                })
            }
            pub fn into_col0(self, index: I) -> SResult<&'a $T0> {
                let index = index.into_index();
                // SAFETY: self.1 is a valid pointer ot an occupation table
                unsafe { validate_row_index(self.1, index)? };
                Ok(unsafe {
                    self.0[0].cast::<FreelistEntry<$T0>>().add(index.get() as usize)
                        .cast::<$T0>().as_ref()
                })
            }
            pub fn into_col0_mut(self, index: I) -> SResult<&'a mut $T0> {
                let index = index.into_index();
                // SAFETY: self.1 is a valid pointer ot an occupation table
                unsafe { validate_row_index(self.1, index)? };
                Ok(unsafe {
                    self.0[0].cast::<FreelistEntry<$T0>>().add(index.get() as usize)
                        .cast::<$T0>().as_mut()
                })
            }
            $(
                pub fn [<col $i>](&self, index: I) -> SResult<&$T> {
                    let index = index.into_index();
                    // SAFETY: self.1 is a valid pointer ot an occupation table
                    unsafe { validate_row_index(self.1, index)? };
                    Ok(unsafe {
                        self.0[$i].cast::<$T>().add(index.get() as usize).as_ref()
                    })
                }
                pub fn [<col $i _mut>](&mut self, index: I) -> SResult<&mut $T> {
                    let index = index.into_index();
                    // SAFETY: self.1 is a valid pointer ot an occupation table
                    unsafe { validate_row_index(self.1, index)? };
                    Ok(unsafe {
                        self.0[$i].cast::<$T>().add(index.get() as usize).as_mut()
                    })
                }
                pub fn [<into_col $i>](self, index: I) -> SResult<&'a $T> {
                    let index = index.into_index();
                    // SAFETY: self.1 is a valid pointer ot an occupation table
                    unsafe { validate_row_index(self.1, index)? };
                    Ok(unsafe {
                        self.0[$i].cast::<$T>().add(index.get() as usize).as_ref()
                    })
                }
                pub fn [<into_col $i _mut>](self, index: I) -> SResult<&'a mut $T> {
                    let index = index.into_index();
                    // SAFETY: self.1 is a valid pointer ot an occupation table
                    unsafe { validate_row_index(self.1, index)? };
                    Ok(unsafe {
                        self.0[$i].cast::<$T>().add(index.get() as usize).as_mut()
                    })
                }
            )*
        }
        unsafe impl<$T0, $($T),*> Columns for ($T0, $($T,)*) {
            const COUNT: usize = 1 $(+{$i;1})*;

            type Ref<'a, I> = TupleRef<'a, ($T0, $($T,)*), I>
            where
                I: IntoIndex + 'a,
                Self: 'a;
            type Mut<'a, I> = TupleMut<'a, ($T0, $($T,)*), I>
            where
                I: IntoIndex + 'a,
                Self: 'a;

            fn register_layout(
                rows: Length,
                register: &mut impl FnMut(Layout),
            ) -> Result<(), LayoutError> {
                register(Layout::from_size_align(
                    rows as usize * size_of::<FreelistEntry<$T0>>(),
                    align_of::<FreelistEntry<$T0>>()
                )?);
                $(register(Layout::from_size_align(
                    rows as usize * size_of::<$T>(),
                    align_of::<$T>()
                )?);)*
                Ok(())
            }
            fn move_into(self, index: Index, columns: &[NonNull<u8>]) {
                let ($t0, $($t,)*) = self;
                {
                    // SAFETY: column 0 was registered to be of type FreelistEntry<$T0>
                    unsafe {
                        columns[0].cast::<FreelistEntry<$T0>>().add(index.get() as usize)
                            .cast::<$T0>().write($t0)
                    };
                }
                $({
                    // SAFETY: column $i was registered to be of type $T
                    unsafe { columns[$i].cast::<$T>().add(index.get() as usize).write($t) };
                })*
            }
            fn take(index: Index, columns: &[NonNull<u8>]) -> Self {
                (
                    {
                        // SAFETY: column 0 was registered to be of type FreelistEntry<$T0> and holds a $T0
                        unsafe {
                            columns[0].cast::<FreelistEntry<$T0>>()
                                .add(index.get() as usize).cast::<$T0>().read()
                        }
                    },
                    $({
                        // SAFETY: column $i was registered to be of type $T
                        unsafe { columns[$i].cast::<$T>().add(index.get() as usize).read() }
                    },)*
                )
            }
            fn as_freelist_entry(index: Index, columns: &[NonNull<u8>]) -> &mut Option<Index> {
                // SAFETY: column 0 was registered to be of type FreelistEntry<$T0> and holds a Option<Index>
                unsafe {
                    columns[0].cast::<FreelistEntry<$T0>>()
                        .add(index.get() as usize).cast::<Option<Index>>().as_mut()
                }
            }
            fn make_ref<'a, I>(columns: &'a [NonNull<u8>], occupation_ptr: NonNull<u8>) -> Self::Ref<'a, I>
            where
                I: IntoIndex + 'a,
                Self: 'a,
            {
                TupleRef(columns, occupation_ptr, PhantomData)
            }
            fn make_mut<'a, I>(columns: &'a [NonNull<u8>], occupation_ptr: NonNull<u8>) -> Self::Mut<'a, I>
            where
                I: IntoIndex + 'a,
                Self: 'a,
            {
                TupleMut(columns, occupation_ptr, PhantomData)
            }
        }
    } };
}
all_tuples_enumerated!(impl_columns, 1, 16, T, t);
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Join<T>(pub T);
pub type Prefix<T, C> = Join<((T,), C)>;
impl<T, C> Prefix<T, C> {
    pub fn new(prefix: T, rest: C) -> Self {
        Self(((prefix,), rest))
    }
    pub fn into_rest(self) -> C {
        self.0.1
    }
}
pub struct JoinRef<'a, T, I>(&'a [NonNull<u8>], NonNull<u8>, PhantomData<fn(I) -> &'a T>);
pub struct JoinMut<'a, T, I>(&'a [NonNull<u8>], NonNull<u8>, PhantomData<fn(I) -> &'a mut T>);
macro_rules! impl_join {
    ((0, $T0:ident) $(,($i:tt, $T:ident))*) => { paste! {
        impl<'a, $T0: Columns, $($T: Columns,)* I> JoinRef<'a, ($T0, $($T,)*), I>
        where
            I: IntoIndex,
        {
            const COUNTS: [usize; 1 $(+{$i;1})*] = [
                $T0::COUNT,
                $($T::COUNT),*
            ];
            const fn offset(mut i: usize) -> usize {
                let mut result = 0;
                while i > 0 {
                    i -= 1;
                    result  += Self::COUNTS[i];
                }
                result
            }
            pub fn part0(&self) -> $T0::Ref<'_, I> {
                $T0::make_ref(&self.0[0..$T0::COUNT], self.1)
            }
            pub fn into_part0(self) -> $T0::Ref<'a, I> {
                $T0::make_ref(&self.0[0..$T0::COUNT], self.1)
            }
            $(
                pub fn [<part $i>](&self) -> $T::Ref<'_, I> {
                    $T::make_ref(&self.0[Self::offset($i)..Self::offset($i + 1)], self.1)
                }
                pub fn [<into_part $i>](self) -> $T::Ref<'a, I> {
                    $T::make_ref(&self.0[Self::offset($i)..Self::offset($i + 1)], self.1)
                }
            )*
        }
        impl<'a, $T0: Columns, $($T: Columns,)* I> JoinMut<'a, ($T0, $($T,)*), I>
        where
            I: IntoIndex,
        {
            const COUNTS: [usize; 1 $(+{$i;1})*] = [
                $T0::COUNT,
                $($T::COUNT),*
            ];
            const fn offset(mut i: usize) -> usize {
                let mut result = 0;
                while i > 0 {
                    i -= 1;
                    result  += Self::COUNTS[i];
                }
                result
            }
            pub fn part0(&self) -> $T0::Ref<'_, I> {
                $T0::make_ref(&self.0[0..$T0::COUNT], self.1)
            }
            pub fn part0_mut(&mut self) -> $T0::Mut<'_, I> {
                $T0::make_mut(&self.0[0..$T0::COUNT], self.1)
            }
            pub fn into_part0(self) -> $T0::Ref<'a, I> {
                $T0::make_ref(&self.0[0..$T0::COUNT], self.1)
            }
            pub fn into_part0_mut(self) -> $T0::Mut<'a, I> {
                $T0::make_mut(&self.0[0..$T0::COUNT], self.1)
            }
            $(
                pub fn [<part $i>](&self) -> $T::Ref<'_, I> {
                    $T::make_ref(&self.0[Self::offset($i)..Self::offset($i + 1)], self.1)
                }
                pub fn [<part $i _mut>](&mut self) -> $T::Mut<'_, I> {
                    $T::make_mut(&self.0[Self::offset($i)..Self::offset($i + 1)], self.1)
                }
                pub fn [<into_part $i>](self) -> $T::Ref<'a, I> {
                    $T::make_ref(&self.0[Self::offset($i)..Self::offset($i + 1)], self.1)
                }
                pub fn [<into_part $i _mut>](self) -> $T::Mut<'a, I> {
                    $T::make_mut(&self.0[Self::offset($i)..Self::offset($i + 1)], self.1)
                }
            )*
        }
        unsafe impl<$T0: Columns, $($T: Columns),*> Columns for Join<($T0, $($T,)*)>
        {
            #![allow(unused_assignments)]
            const COUNT: usize = $T0::COUNT $(+ $T::COUNT)*;

            type Ref<'a, I> = JoinRef<'a, ($T0, $($T,)*), I>
            where
                I: IntoIndex + 'a,
                Self: 'a;
            type Mut<'a, I> = JoinMut<'a, ($T0, $($T,)*), I>
            where
                I: IntoIndex + 'a,
                Self: 'a;

            fn register_layout(
                count: Length,
                register: &mut impl FnMut(Layout),
            ) -> Result<(), LayoutError> {
                $T0::register_layout(count, register)?;
                $($T::register_layout(count, register)?;)*
                Ok(())
            }
            fn move_into(
                self,
                index: Index,
                columns: &[NonNull<u8>],
            ) {
                self.0.0.move_into(index, &columns[0..$T0::COUNT]);
                let mut i0 = $T0::COUNT;
                $({
                    self.0.$i.move_into(index, &columns[i0..i0+$T::COUNT]);
                    i0 += $T::COUNT;
                })*
            }
            fn take(index: Index, columns: &[NonNull<u8>]) -> Self {
                let mut offsets = [$T0::COUNT; 1 $(+{$i;1})*];
                $(offsets[$i] = offsets[$i - 1] + $T::COUNT;)*
                Self((
                    $T0::take(index, &columns[0..$T0::COUNT]),
                    $($T::take(index, &columns[offsets[$i - 1]..offsets[$i]]),)*
                ))
            }
            fn as_freelist_entry(
                index: Index,
                columns: &[NonNull<u8>],
            ) -> &mut Option<Index> {
                $T0::as_freelist_entry(index, &columns[0..$T0::COUNT])
            }
            fn make_ref<'a, I>(columns: &'a [NonNull<u8>], occupation_ptr: NonNull<u8>) -> Self::Ref<'a, I>
            where
                I: IntoIndex + 'a,
                Self: 'a,
            {
                JoinRef(columns, occupation_ptr, PhantomData)
            }
            fn make_mut<'a, I>(columns: &'a [NonNull<u8>], occupation_ptr: NonNull<u8>) -> Self::Mut<'a, I>
            where
                I: IntoIndex + 'a,
                Self: 'a,
            {
                JoinMut(columns, occupation_ptr, PhantomData)
            }
        }
    }};
}
all_tuples_enumerated!(impl_join, 2, 16, T);
