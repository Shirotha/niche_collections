use thiserror::Error;

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

// TODO: what restrictings on T are reasonable?
pub trait Store<T> {
    fn get(&self, index: Index) -> Result<&T, StoreError>;
    fn get_mut(&mut self, index: Index) -> Result<&mut T, StoreError>;
    fn try_insert(&mut self, data: T) -> Option<Index>;
    fn reserve(&mut self, count: usize) -> Result<(), StoreError>;
    fn delete(&mut self, index: Index) -> Result<T, StoreError>;
}

// SAFETY: 1 is always a valid index
const ONE: Index = unsafe { Index::new_unchecked(1) };

pub trait MultiStore<T>: Store<T> {
    fn get_many(&self, index: Index, len: Index) -> Result<&[T], StoreError>;
    fn get_many_mut(&mut self, index: Index, len: Index) -> Result<&mut [T], StoreError>;
    fn try_insert_many(&mut self, data: &[T]) -> Option<Index>;
    // TODO: should this really allocate on heap?!?
    fn delete_many(&mut self, index: Index, len: Index) -> Result<Box<[T]>, StoreError>;

    fn get(&self, index: Index) -> Result<&T, StoreError> {
        self.get_many(index, ONE).map(|xs| &xs[0])
    }

    fn get_mut(&mut self, index: Index) -> Result<&mut T, StoreError> {
        self.get_many_mut(index, ONE).map(|xs| &mut xs[0])
    }

    fn try_insert(&mut self, data: T) -> Option<Index> {
        self.try_insert_many(&[data])
    }

    fn delete(&mut self, index: Index) -> Result<T, StoreError> {
        todo!("how to move out of single length boxed slice")
    }
}
