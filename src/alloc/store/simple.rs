use super::*;

#[derive(Debug)]
pub struct SimpleStore<T> {
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
        self.data
            .get(index.get() as usize)
            .ok_or(StoreError::OutOfBounds(index, self.data.len() as Length))
    }

    fn get_mut(&mut self, index: Index) -> Result<&mut T, StoreError> {
        let len = self.data.len() as Length;
        self.data.get_mut(index.get() as usize).ok_or(StoreError::OutOfBounds(index, len))
    }

    fn get_disjoint_mut<const N: usize>(
        &mut self,
        indices: [Index; N],
    ) -> Result<[&mut T; N], StoreError> {
        self.data.get_disjoint_mut(indices.map(|i| i.get() as usize)).map_err(StoreError::from)
    }

    unsafe fn get_disjoint_unchecked_mut<const N: usize>(
        &mut self,
        indices: [Index; N],
    ) -> [&mut T; N] {
        // SAFETY: assumptions guarantied by caller
        unsafe { self.data.get_disjoint_unchecked_mut(indices.map(|i| i.get() as usize)) }
    }

    fn insert_within_capacity(&mut self, data: T) -> Result<Index, T> {
        let index = self.data.len();
        if index == self.data.capacity() {
            return Err(data);
        }
        self.data.push(data);
        // SAFETY: all indices within capacity are valid
        Ok(unsafe { Index::new_unchecked(index as u32) })
    }

    fn reserve(&mut self, additional: Length) -> Result<(), StoreError> {
        let len = self.data.len() as Length;
        let min_target =
            len.checked_add(additional).ok_or(StoreError::OutofMemory(additional, len))?;
        let target = min_target.max(2 * len).min(Index::MAX.get() + 1);
        if target < min_target {
            return Err(StoreError::OutofMemory(additional, len));
        }
        self.data.reserve_exact((target - len) as usize);
        assert!(
            self.data.capacity() <= Index::MAX.get() as usize + 1,
            "capacity exceeds maximum index"
        );
        Ok(())
    }

    /// This will not drop existing items and might cause a memory leak
    fn clear(&mut self) {
        self.data.clear();
    }
}

impl<T: Clone> MultiStore<T> for SimpleStore<T> {
    fn get_many(&self, index: Range<Index>) -> Result<&[T], StoreError> {
        let a = index.start.get();
        let b = index.end.get();
        self.data
            .get(a as usize..b as usize)
            .ok_or(StoreError::OutOfBounds(index.start, b.saturating_sub(a)))
    }

    fn get_many_mut(&mut self, index: Range<Index>) -> Result<&mut [T], StoreError> {
        let a = index.start.get();
        let b = index.end.get();
        self.data
            .get_mut(a as usize..b as usize)
            .ok_or(StoreError::OutOfBounds(index.start, b.saturating_sub(a)))
    }

    fn get_many_disjoint_mut<const N: usize>(
        &mut self,
        indices: [Range<Index>; N],
    ) -> Result<[&mut [T]; N], StoreError> {
        self.data
            .get_disjoint_mut(indices.map(|i| i.start.get() as usize..i.end.get() as usize))
            .map_err(StoreError::from)
    }

    unsafe fn get_many_disjoint_unchecked_mut<const N: usize>(
        &mut self,
        indices: [Range<Index>; N],
    ) -> [&mut [T]; N] {
        // SAFETY: assumptions guarantied by caller
        unsafe {
            self.data.get_disjoint_unchecked_mut(
                indices.map(|i| i.start.get() as usize..i.end.get() as usize),
            )
        }
    }

    fn insert_many_within_capacity(
        &mut self,
        len: Length,
    ) -> Option<(Index, BeforeInsertMany<'_, T>)> {
        let len = len as usize;
        if self.data.len() + len > self.data.capacity() {
            return None;
        }
        // SAFETY: all indices within capacity are valid
        let index = unsafe { Index::new_unchecked(self.data.len() as u32) };
        Some((index, BeforeInsertMany { data: &mut self.data, len }))
    }
}
