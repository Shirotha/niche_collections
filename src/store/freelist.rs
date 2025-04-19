use std::mem::replace;

use super::*;

#[derive(Debug, PartialEq, Eq)]
enum Entry<T> {
    Occupied(T),
    Free(Option<Index>),
}

#[derive(Debug)]
pub struct FreelistStore<T> {
    data: Vec<Entry<T>>,
    head: Option<Index>,
}
impl<T> FreelistStore<T> {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_capacity(capacity: usize) -> Self {
        if capacity > Index::MAX.get() as usize {
            panic!("capacity exceeds largest possible index!")
        }
        Self { data: Vec::with_capacity(capacity), head: None }
    }
}
impl<T> Default for FreelistStore<T> {
    fn default() -> Self {
        Self { data: Vec::new(), head: None }
    }
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

    fn insert_within_capacity(&mut self, data: T) -> Result<Index, T> {
        if let Some(index) = self.head.take() {
            let old = replace(&mut self.data[index.get() as usize], Entry::Occupied(data));
            let Entry::Free(new_head) = old else {
                unreachable!("freelist head should always point to a free Entry")
            };
            self.head = new_head;
            Ok(index)
        } else {
            let len = self.data.len();
            // HACK: https://github.com/rust-lang/rust/issues/100486
            if len == self.data.capacity() {
                return Err(data);
            }
            let index = Index::new(len as u32).expect("index is in capacity and should be valid");
            self.data.push(Entry::Occupied(data));
            Ok(index)
        }
    }

    fn reserve(&mut self, additional: usize) -> Result<(), StoreError> {
        if self.data.len() + additional >= Index::MAX.get() as usize {
            return Err(StoreError::OutofMemory(additional, self.data.len()));
        }
        self.data.reserve(additional);
        Ok(())
    }

    fn clear(&mut self) {
        self.data.clear();
        self.head = None;
    }
}

impl<T> ReusableStore<T> for FreelistStore<T> {
    fn remove(&mut self, index: Index) -> Result<T, StoreError> {
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

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn can_insert_without_alloc() {
        let mut store = FreelistStore::with_capacity(1);
        let index = store
            .insert_within_capacity(42)
            .expect("store with capacity should not have to allocate");
        assert_eq!(0, index.get(), "insert under capacity will append");
        assert_eq!(Ok(&42), store.get(index), "stored value should not change");
    }

    #[test]
    fn can_expand_capacity() {
        let mut store = FreelistStore::new();
        assert_eq!(Err(42), store.insert_within_capacity(42));
        assert_eq!(Ok(()), store.reserve(1));
    }

    #[test]
    fn can_reuse_slot() {
        let mut store = FreelistStore::with_capacity(1);
        let index = store
            .insert_within_capacity(42)
            .expect("store with capacity should not have to allocate");
        assert_eq!(Ok(42), store.remove(index), "remove should return original value");
        let index2 = store
            .insert_within_capacity(42)
            .expect("freed space should be reused without allocation needed");
        assert_eq!(index, index2, "index should be reused");
    }
}
