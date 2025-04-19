use std::ops::Deref;

use thiserror::Error;

mod simple;
pub use simple::*;

mod freelist;
pub use freelist::*;

mod intervaltree;
pub use intervaltree::*;

pub type Index = nonmax::NonMaxU32;

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum StoreError {
    #[error("Tried to access data at index {0} when length was {1}.")]
    OutOfBounds(Index, usize),
    #[error("Tried to access previously freed data at index {0}.")]
    AccessAfterFree(Index),
    #[error("Tried to free already freed data at index {0}.")]
    DoubleFree(Index),
    #[error("Tried to allocate {0} items when length was {1}")]
    OutofMemory(usize, usize),
}

pub trait Store<T> {
    fn get(&self, index: Index) -> Result<&T, StoreError>;
    fn get_mut(&mut self, index: Index) -> Result<&mut T, StoreError>;
    fn insert_within_capacity(&mut self, data: T) -> Result<Index, T>;
    fn reserve(&mut self, additional: usize) -> Result<(), StoreError>;
    fn clear(&mut self);
}
pub trait ReusableStore<T>: Store<T> {
    fn remove(&mut self, index: Index) -> Result<T, StoreError>;
}

// SAFETY: 1 is always a valid index
const ONE: Index = unsafe { Index::new_unchecked(1) };

#[derive(Debug)]
pub struct BeforeRemoveMany<'a, T, F: FnOnce()> {
    data:           &'a [T],
    commit_removal: Option<F>,
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

// TODO: is Clone here really needed?
pub trait MultiStore<T: Clone> {
    fn get_many(&self, index: Index, len: Index) -> Result<&[T], StoreError>;
    fn get_many_mut(&mut self, index: Index, len: Index) -> Result<&mut [T], StoreError>;
    fn insert_many_within_capacity(&mut self, data: &[T]) -> Option<Index>;
}
pub trait ReusableMultiStore<T: Clone>: MultiStore<T> {
    fn remove_many(
        &mut self,
        index: Index,
        len: Index,
    ) -> Result<BeforeRemoveMany<'_, T, impl FnOnce()>, StoreError>;
}
