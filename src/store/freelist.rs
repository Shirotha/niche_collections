use std::mem::replace;

use super::*;

#[derive(Debug)]
enum Entry<T> {
    Occupied(T),
    Free(Option<Index>),
}

#[derive(Debug)]
pub struct FreelistStore<T> {
    data: Vec<Entry<T>>,
    head: Option<Index>,
}
impl<T> Store<T> for FreelistStore<T> {
    fn get(&self, index: Index) -> Result<&T, StoreError> {
        let entry = self
            .data
            .get(index.get() as usize)
            .ok_or(StoreError::OutOfBounds(index, self.data.len()))?;
        match entry {
            Entry::Occupied(x) => Ok(x),
            Entry::Free(_) => Err(StoreError::AccessAfterFree(index)),
        }
    }

    fn get_mut(&mut self, index: Index) -> Result<&mut T, StoreError> {
        // HACK: circumvent borrowchecker false positive
        let len = self.data.len();
        let entry =
            self.data.get_mut(index.get() as usize).ok_or(StoreError::OutOfBounds(index, len))?;
        match entry {
            Entry::Occupied(x) => Ok(x),
            Entry::Free(_) => Err(StoreError::AccessAfterFree(index)),
        }
    }

    fn try_insert(&mut self, data: T) -> Result<Option<Index>, StoreError> {
        if let Some(index) = self.head.take() {
            let old = replace(&mut self.data[index.get() as usize], Entry::Occupied(data));
            let Entry::Free(new_head) = old else {
                unreachable!("freelist head should always point to a free Entry")
            };
            self.head = new_head;
            Ok(Some(index))
        } else {
            let len = self.data.len();
            // HACK: https://github.com/rust-lang/rust/issues/100486
            if len == self.data.capacity() {
                return Ok(None);
            }
            let index = Index::new(len as u32).expect("index is in capacity and should be valid");
            self.data.push(Entry::Occupied(data));
            Ok(Some(index))
        }
    }

    fn reserve(&mut self, additional: usize) -> Result<(), StoreError> {
        if self.data.len() + additional > Index::MAX.get() as usize {
            return Err(StoreError::OutofMemory(additional, self.data.len()));
        }
        self.data.reserve(additional);
        Ok(())
    }

    fn delete(&mut self, index: Index) -> Result<T, StoreError> {
        // HACK: circumvent borrowchecker false positive
        let len = self.data.len();
        let entry =
            self.data.get_mut(index.get() as usize).ok_or(StoreError::OutOfBounds(index, len))?;
        match entry {
            entry @ Entry::Occupied(_) => {
                let old = replace(entry, Entry::Free(self.head.take()));
                self.head = Some(index);
                let Entry::Occupied(data) = old else {
                    unreachable!("this was already checked in the outer match statement");
                };
                Ok(data)
            },
            Entry::Free(_) => Err(StoreError::DoubleFree(index)),
        }
    }
}
