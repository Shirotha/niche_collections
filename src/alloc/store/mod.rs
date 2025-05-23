use std::{
    mem::MaybeUninit,
    ops::{Deref, Range},
    slice::{self, GetDisjointMutError},
};

use thiserror::Error;

use super::*;

mod simple;
pub use simple::*;

mod freelist;
pub use freelist::*;

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
    #[error("Tried to allocate {0} items when length was {1}")]
    OutofMemory(Length, Length),
    #[error("Disjoint Error: {0}")]
    DisjointError(#[from] GetDisjointMutError),
}

pub trait Store<T> {
    fn get(&self, index: Index) -> Result<&T, StoreError>;
    fn get_mut(&mut self, index: Index) -> Result<&mut T, StoreError>;
    fn get_disjoint_mut<const N: usize>(
        &mut self,
        indices: [Index; N],
    ) -> Result<[&mut T; N], StoreError>;
    /// # Safety
    /// Does not perform any checks on the indices.
    unsafe fn get_disjoint_unchecked_mut<const N: usize>(
        &mut self,
        indices: [Index; N],
    ) -> [&mut T; N];
    fn insert_within_capacity(&mut self, data: T) -> Result<Index, T>;
    fn reserve(&mut self, additional: Length) -> Result<(), StoreError>;
    fn clear(&mut self);
}
pub trait ReusableStore<T>: Store<T> {
    fn remove(&mut self, index: Index) -> Result<T, StoreError>;
}

#[derive(Debug)]
pub struct BeforeInsertMany<'a, T> {
    data: &'a mut Vec<T>,
    len:  usize,
}
impl<T> BeforeInsertMany<'_, T> {
    pub fn get_mut(&mut self) -> &mut [MaybeUninit<T>] {
        &mut self.data.spare_capacity_mut()[0..self.len]
    }
}
impl<T> Drop for BeforeInsertMany<'_, T> {
    fn drop(&mut self) {
        unsafe { self.data.set_len(self.data.len() + self.len) };
    }
}

#[derive(Debug)]
pub struct BeforeRemoveMany<'a, T, F: FnOnce()> {
    data:           &'a [T],
    commit_removal: Option<F>,
}
impl<'a, T, F: FnOnce()> BeforeRemoveMany<'a, T, F> {
    /// # Safety
    /// `data` has to be a valid `U` pointer and `len` has to be compatible.
    pub(super) unsafe fn transmute<U>(mut self, len: usize) -> BeforeRemoveMany<'a, U, F> {
        BeforeRemoveMany {
            data:           unsafe { slice::from_raw_parts(self.data.as_ptr() as *const U, len) },
            commit_removal: self.commit_removal.take(),
        }
    }
}
impl<T, F: FnOnce()> Deref for BeforeRemoveMany<'_, T, F> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.data
    }
}
impl<T, F: FnOnce()> Drop for BeforeRemoveMany<'_, T, F> {
    fn drop(&mut self) {
        self.commit_removal.take().into_iter().for_each(|f| f());
    }
}

pub trait MultiStore<T: Clone>: Store<T> {
    fn get_many(&self, index: Range<Index>) -> Result<&[T], StoreError>;
    fn get_many_mut(&mut self, index: Range<Index>) -> Result<&mut [T], StoreError>;
    fn get_many_disjoint_mut<const N: usize>(
        &mut self,
        indices: [Range<Index>; N],
    ) -> Result<[&mut [T]; N], StoreError>;
    /// # Safety
    /// Does not perform any checks on the indices.
    unsafe fn get_many_disjoint_unchecked_mut<const N: usize>(
        &mut self,
        indices: [Range<Index>; N],
    ) -> [&mut [T]; N];
    fn insert_many_within_capacity(
        &mut self,
        len: Length,
    ) -> Option<(Index, BeforeInsertMany<'_, T>)>;
}
pub trait ReusableMultiStore<T: Clone>: MultiStore<T> {
    fn remove_many(
        &mut self,
        index: Range<Index>,
    ) -> Result<BeforeRemoveMany<'_, T, impl FnOnce()>, StoreError>;
}
