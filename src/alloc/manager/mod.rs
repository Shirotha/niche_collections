mod exclusive;
use std::{
    array,
    marker::PhantomData,
    mem::{ManuallyDrop, MaybeUninit},
    ops::Range,
    ptr::{copy_nonoverlapping, read_unaligned, write_unaligned},
    slice,
};

pub use exclusive::*;
mod version;
use thiserror::Error;
pub use version::*;

use super::{arena::*, store::*, *};

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ManagerError {
    #[error("store error: {0}")]
    StoreError(#[from] StoreError),
    #[error("bad handle {0}")]
    BadHandle(&'static str),
}
pub type MResult<T> = Result<T, ManagerError>;

pub trait Config {
    type Store;
    type Manager<'id>;
    type Arena<'id, 'man>;
}
pub struct GlobalConfig<K, C>(PhantomData<(K, C)>);
pub type RemoveSliceGuard<'a, U, C> =
    <<GlobalConfig<Slices<U>, C> as Config>::Store as RemoveIndirect<Multi<U>>>::Guard<'a>;
// TODO: replace with Alignment once stable
pub trait RawBytes: Copy {}
macro_rules! impl_RawBytes {
    ($t:ty) => {
        impl RawBytes for $t {}
    };
}
impl_RawBytes!(u8);
impl_RawBytes!(u16);
impl_RawBytes!(u32);
impl_RawBytes!(u64);
impl_RawBytes!(u128);
pub struct Versioned<const REUSE: bool = false, H = Headless, V = ()>(PhantomData<(H, V)>);
pub struct Exclusive<const REUSE: bool = false, V = ()>(PhantomData<V>);

macro_rules! kind {
    ($vis:vis struct $name:ident $(<$($T:ident),*>)? [[$elX:ty, $elV:ty], [$storeS:ident, $storeR:ident]] $(where $($where:tt)*)?) => {
        $vis struct $name$(<$($T),*>(PhantomData<($($T,)*)>))? $(where $($where)*)?;
        impl$(<$($T),*>)? Config for GlobalConfig<$name$(<$($T),*>)?, Exclusive<false>> $(where $($where)*)? {
            type Store = $storeS<$elX>;
            type Manager<'id> = XManager<'id, $name$(<$($T),*>)?, Exclusive<false>>;
            type Arena<'id, 'man> = XArena<'id, $name$(<$($T),*>)?, Exclusive<false>>;
        }
        impl$(<$($T),*>)? Config for GlobalConfig<$name$(<$($T),*>)?, Exclusive<true>> $(where $($where)*)? {
            type Store = $storeR<$elX>;
            type Manager<'id> = XManager<'id, $name$(<$($T),*>)?, Exclusive<true>>;
            type Arena<'id, 'man> = XArena<'id, $name$(<$($T),*>)?, Exclusive<true>>;
        }
        impl<H$(, $($T),*)?> Config for GlobalConfig<$name$(<$($T),*>)?, Versioned<false, H>> $(where $($where)*)? {
            type Store = $storeS<$elV>;
            type Manager<'id> = VManager<'id, $name$(<$($T),*>)?, Versioned<false, H>>;
            type Arena<'id, 'man> = VArena<'id, 'man, $name$(<$($T),*>)?, Versioned<false, H>>;
        }
        impl<H$(, $($T),*)?> Config for GlobalConfig<$name$(<$($T),*>)?, Versioned<true, H>> $(where $($where)*)? {
            type Store = $storeS<$elV>;
            type Manager<'id> = VManager<'id, $name$(<$($T),*>)?, Versioned<true, H>>;
            type Arena<'id, 'man> = VArena<'id, 'man, $name$(<$($T),*>)?, Versioned<true, H>>;
        }
    };
}
kind! {
    pub struct Typed<T>[
        [T, (Version, T)],
        [SimpleStore, FreelistStore]
    ]
}
kind! {
    pub struct SoA<C>[
        [C, Prefix<Version, C>],
        [SoAFreelistStore, SoAFreelistStore]
    ] where C: Columns
}
kind! {
    pub struct Slices<U>[
        [U, U],
        [SimpleStore, IntervaltreeStore]
    ] where U: RawBytes
}
kind! {
    pub struct Mixed<U>[
        [U, U],
        [SimpleStore, IntervaltreeStore]
    ] where U: RawBytes
}

pub struct Manager<'id, K, C>(pub(super) <GlobalConfig<K, C> as Config>::Manager<'id>)
where
    GlobalConfig<K, C>: Config;

pub(super) fn map_result<const N: usize, IN, OUT, E, F>(
    srcs: impl IntoIterator<Item = IN>,
    f: F,
) -> Result<[OUT; N], E>
where
    F: Fn(IN) -> Result<OUT, E>,
{
    let mut results = array::from_fn(|_| MaybeUninit::uninit());
    for (out, src) in results.iter_mut().zip(srcs) {
        out.write(f(src)?);
    }
    // SAFETY: all array elements are initialized in the prior loop
    Ok(results.map(|r| unsafe { r.assume_init() }))
}

impl<U: RawBytes> Slices<U> {
    pub(super) fn header_size<H>() -> Length {
        Self::size_of::<(Length, H)>(1)
    }
    pub(super) fn header_range<H>(index: Index) -> SResult<Range<Index>> {
        let size = Self::header_size::<H>();
        let end = Index::new(index.get() + size)
            .ok_or_else(|| StoreError::OutOfBounds(index, index.get()))?;
        Ok(index..end)
    }
    /// # Safety
    /// `index` has to be a pointer to a valid `H`.
    unsafe fn read_header<H>(
        store: &impl MultiStore<U>,
        index: Index,
    ) -> SResult<((Length, H), Index)> {
        let range = Self::header_range::<H>(index)?;
        let end = range.end;
        // SAFETY: guarantied by caller
        let header = unsafe { read_unaligned(store.get(range)?.as_ptr() as *const (Length, H)) };
        Ok((header, end))
    }
    pub(super) fn size_of<T>(len: Length) -> Length {
        (size_of::<T>() as Length * len).div_ceil(size_of::<U>() as Length)
    }
    fn range_of<T>(index: Index, len: Length) -> SResult<Range<Index>> {
        let size = Self::size_of::<T>(len);
        let end = Index::new(index.get() + size)
            .ok_or_else(|| StoreError::OutOfBounds(index, index.get() + size - 1))?;
        Ok(index..end)
    }
    /// # Safety
    /// `index` and `len` are not checked (results of `read_header` are always valid).
    unsafe fn get_slice<T>(store: &impl MultiStore<U>, index: Index, len: Length) -> SResult<&[T]> {
        let range = Self::range_of::<T>(index, len)?;
        // SAFETY: guarantied by caller
        Ok(unsafe { slice::from_raw_parts(store.get(range)?.as_ptr() as *const T, len as usize) })
    }
    /// # Safety
    /// `index` and `len` are not checked (results of `read_header` are always valid).
    unsafe fn get_slice_mut<T>(
        store: &mut impl MultiStore<U>,
        index: Index,
        len: Length,
    ) -> SResult<&mut [T]> {
        let range = Self::range_of::<T>(index, len)?;
        // SAFETY: guarantied by caller
        Ok(unsafe {
            slice::from_raw_parts_mut(store.get_mut(range)?.as_mut_ptr() as *mut T, len as usize)
        })
    }
    /// # Safety
    /// Does not check if `indices` are valid and distinct.
    unsafe fn get_disjoint_mut<const N: usize, T, H, E>(
        store: &mut (impl MultiStore<U> + GetDisjointMut<Multi<U>>),
        indices: [Index; N],
        validate: impl Fn(&[((Length, H), Index)]) -> Result<(), E>,
    ) -> Result<[&mut [T]; N], E>
    where
        E: From<StoreError>,
    {
        // SAFETY: guarantied by caller
        let headers: [_; N] =
            map_result(indices, |index| unsafe { Self::read_header::<H>(&*store, index) })?;
        validate(&headers)?;
        let ranges: [_; N] =
            map_result(&headers, |((len, _), index)| Self::range_of::<T>(*index, *len))?;
        // SAFETY: guarantied by caller
        let mut data = unsafe { store.get_disjoint_unchecked_mut(ranges) };
        // SAFETY: always valid for data written by `write_slice`
        Ok(array::from_fn(|i| unsafe {
            slice::from_raw_parts_mut(data[i].as_mut_ptr() as *mut T, headers[i].0.0 as usize)
        }))
    }
    fn write_header<H>(
        len: Length,
        extra_header: H,
        dst: &mut [MaybeUninit<U>],
    ) -> &mut [MaybeUninit<U>] {
        let header_size = Self::header_size::<H>() as usize;
        // SAFETY: panics when not enough space
        unsafe { write_unaligned(dst[0..header_size].as_mut_ptr() as *mut _, (len, extra_header)) };
        &mut dst[header_size..]
    }
    /// # Safety
    /// `src` and `dst` have to have compatible sizes and can't overlap.
    // NOTE: T has to be Copy because this cannot consume an unsized [T], so the original might be dropped while also stored here
    unsafe fn write_slice<T: Copy, H>(src: &[T], extra_header: H, dst: &mut [MaybeUninit<U>]) {
        assert!(align_of::<U>() >= align_of::<T>(), "incompatible alignment");
        // SAFETY: guarantied by caller
        let dst = Self::write_header(src.len() as Length, extra_header, dst);
        // SAFETY: guarantied by caller
        unsafe { copy_nonoverlapping(src.as_ptr(), dst.as_mut_ptr() as *mut T, src.len()) };
    }
    /// # Safety
    /// `index` and `len` are not checked (results of `read_header` are always valid).
    unsafe fn delete_slice<'a, T: Copy, S: ReusableMultiStore<U>>(
        store: &'a mut S,
        index: Index,
        len: Length,
    ) -> SResult<<S as RemoveIndirect<Multi<U>>>::Guard<'a>>
    where
        U: 'a,
    {
        let range = Self::range_of::<T>(index, len)?;
        // FIXME: this only removes the data, not the header!
        store.remove_indirect(range)
    }
}
impl<U: RawBytes> Mixed<U> {
    pub(super) fn size_of<T>() -> Length {
        size_of::<T>().div_ceil(size_of::<U>()) as Length
    }
    fn range_of<T>(index: Index) -> SResult<Range<Index>> {
        assert!(align_of::<U>() >= align_of::<T>());
        let size = Self::size_of::<T>();
        let end = Index::new(index.get() + size)
            .ok_or_else(|| StoreError::OutOfBounds(index, index.get() + size - 1))?;
        Ok(index..end)
    }
    /// # Safety
    /// `index` has to be a valid pointer to a T.
    unsafe fn get_instance<T>(store: &impl MultiStore<U>, index: Index) -> SResult<&T> {
        let range = Self::range_of::<T>(index)?;
        // SAFETY: guarnatied by caller
        Ok(unsafe { &*(store.get(range)?.as_ptr() as *const T) })
    }
    /// # Safety
    /// `index` has to be a valid pointer to a T.
    unsafe fn get_instance_mut<T>(store: &mut impl MultiStore<U>, index: Index) -> SResult<&mut T> {
        let range = Self::range_of::<T>(index)?;
        // SAFETY: guarnatied by caller
        Ok(unsafe { &mut *(store.get_mut(range)?.as_mut_ptr() as *mut T) })
    }
    /// # Safety
    /// Does not check if `indices` are valid or distinct.
    unsafe fn get_disjoint_unchecked_mut<const N: usize, T>(
        store: &mut impl GetDisjointMut<Multi<U>>,
        indices: [Index; N],
    ) -> SResult<[&mut T; N]> {
        let ranges: [_; N] = map_result(indices, Self::range_of::<T>)?;
        // SAFETY: guarantied by caller
        let data = unsafe { store.get_disjoint_unchecked_mut(ranges) };
        // SAFETY: always valid for data written by `write_instance`
        Ok(data.map(|d| unsafe { &mut *(d.as_mut_ptr() as *mut T) }))
    }
    /// # Safety
    /// Does not check if `indices` are valid.
    unsafe fn get_disjoint_mut<const N: usize, T>(
        store: &mut impl GetDisjointMut<Multi<U>>,
        indices: [Index; N],
    ) -> SResult<[&mut T; N]> {
        let ranges: [_; N] = map_result(indices, Self::range_of::<T>)?;
        let data = store.get_disjoint_mut(ranges)?;
        // SAFETY: always valid for data written by `write_instance`
        Ok(data.map(|d| unsafe { &mut *(d.as_mut_ptr() as *mut T) }))
    }
    /// # Safety
    /// `src` and `dst` have to have compatible sizes and can't overlap.
    unsafe fn write_instance<T>(src: T, dst: &mut [MaybeUninit<U>]) {
        let src = ManuallyDrop::new(src);
        assert!(align_of::<U>() >= align_of::<T>(), "incompatible alignment");
        // SAFETY: guarantied by caller
        unsafe { copy_nonoverlapping(&*src as *const T, dst.as_mut_ptr() as *mut T, 1) };
    }
    /// # Safety
    /// `index` has to be a valid pointer to a T.
    unsafe fn delete_instance<T>(
        store: &mut impl ReusableMultiStore<U>,
        index: Index,
    ) -> SResult<T> {
        let range = Self::range_of::<T>(index)?;
        let lock = store.remove_indirect(range)?;
        let mut result = MaybeUninit::uninit();
        // SAFETY: guarantied by caller
        unsafe { copy_nonoverlapping(lock.as_ref().as_ptr() as *const T, result.as_mut_ptr(), 1) };
        // SAFETY: previous line always writes a valid T into result
        Ok(unsafe { result.assume_init() })
    }
}
