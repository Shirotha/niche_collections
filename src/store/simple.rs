use super::*;

#[derive(Debug)]
struct SimpleStore<T> {
    data: Vec<T>,
}
impl<T> SimpleStore<T> {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_capacity(capacity: usize) -> Self {
        Self { data: Vec::with_capacity(capacity) }
    }
}
impl<T> Default for SimpleStore<T> {
    fn default() -> Self {
        Self { data: Vec::new() }
    }
}
impl<T> Store<T> for SimpleStore<T> {
    fn get(&self, index: Index) -> Result<&T, StoreError> {
        self.data.get(index.get() as usize).ok_or(StoreError::OutOfBounds(index, self.data.len()))
    }

    fn get_mut(&mut self, index: Index) -> Result<&mut T, StoreError> {
        let len = self.data.len();
        self.data.get_mut(index.get() as usize).ok_or(StoreError::OutOfBounds(index, len))
    }

    fn insert_within_capacity(&mut self, data: T) -> Result<Index, T> {
        let index = self.data.len();
        if index == self.data.capacity() {
            return Err(data);
        }
        match Index::new(index as u32) {
            Some(index) => {
                self.data.push(data);
                Ok(index)
            },
            None => Err(data),
        }
    }

    fn reserve(&mut self, additional: usize) -> Result<(), StoreError> {
        if self.data.len() + additional >= Index::MAX.get() as usize {
            return Err(StoreError::OutofMemory(additional, self.data.len()));
        }
        self.data.reserve(additional);
        Ok(())
    }

    #[expect(unused_variables)]
    fn delete(&mut self, index: Index) -> Result<T, StoreError> {
        panic!("unsupported, use clear instead")
    }

    fn clear(&mut self) {
        self.data.clear();
    }
}

impl<T: Clone> MultiStore<T> for SimpleStore<T> {
    fn get_many(&self, index: Index, len: Index) -> Result<&[T], StoreError> {
        let i = index.get() as usize;
        let n = len.get() as usize;
        if i + n > self.data.len().min(Index::MAX.get() as usize) {
            return Err(StoreError::OutOfBounds(index, self.data.len()));
        }
        Ok(&self.data[i..i + n])
    }

    fn get_many_mut(&mut self, index: Index, len: Index) -> Result<&mut [T], StoreError> {
        let i = index.get() as usize;
        let n = len.get() as usize;
        if i + n > self.data.len().min(Index::MAX.get() as usize) {
            return Err(StoreError::OutOfBounds(index, self.data.len()));
        }
        Ok(&mut self.data[i..i + n])
    }

    fn insert_many_within_capacity(&mut self, data: &[T]) -> Option<Index> {
        if self.data.len() + data.len() > self.data.capacity() {
            return None;
        }
        let index =
            Index::new(self.data.len() as u32).expect("index is in capacity and should be valid");
        self.data.extend_from_slice(data);
        Some(index)
    }

    #[expect(unreachable_code, unused_variables)]
    fn delete_many(
        &mut self,
        index: Index,
        len: Index,
    ) -> Result<BeforeDeleteMany<'_, T, impl FnOnce()>, StoreError> {
        panic!("unsupported, use clear instead");
        // NOTE: needed to prevent explicit type annotations
        Ok(BeforeDeleteMany { data: self.get_many(index, len)?, commit_delete: Some(|| ()) })
    }
}
