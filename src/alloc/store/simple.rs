use super::*;

#[derive(Debug)]
pub struct SimpleStore<T> {
    data: Vec<T>,
}
impl<T> SimpleStore<T> {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_capacity(capacity: Length) -> Self {
        Self { data: Vec::with_capacity(capacity as usize) }
    }
}
impl<T> Default for SimpleStore<T> {
    fn default() -> Self {
        Self { data: Vec::new() }
    }
}
impl<T> Get<Single<T>> for SimpleStore<T> {
    fn get(&self, index: Index) -> SResult<&T> {
        self.data
            .get(index.get() as usize)
            .ok_or(StoreError::OutOfBounds(index, self.data.len() as Length))
    }

    fn get_mut(&mut self, index: Index) -> SResult<&mut T> {
        let len = self.data.len() as Length;
        self.data.get_mut(index.get() as usize).ok_or(StoreError::OutOfBounds(index, len))
    }
}
impl<T> GetDisjointMut<Single<T>> for SimpleStore<T> {
    fn get_disjoint_mut<const N: usize>(&mut self, indices: [Index; N]) -> SResult<[&mut T; N]> {
        self.data.get_disjoint_mut(indices.map(|i| i.get() as usize)).map_err(StoreError::from)
    }

    unsafe fn get_disjoint_unchecked_mut<const N: usize>(
        &mut self,
        indices: [Index; N],
    ) -> [&mut T; N] {
        // SAFETY: assumptions guarantied by caller
        unsafe { self.data.get_disjoint_unchecked_mut(indices.map(|i| i.get() as usize)) }
    }
}
impl<T> Insert<Single<T>> for SimpleStore<T> {
    fn insert_within_capacity(&mut self, data: T) -> Result<Index, T> {
        let index = self.data.len();
        if index == self.data.capacity() {
            return Err(data);
        }
        self.data.push(data);
        // SAFETY: all indices within capacity are valid
        Ok(unsafe { Index::new_unchecked(index as u32) })
    }
}
impl<T> Resizable for SimpleStore<T> {
    fn capacity(&self) -> Length {
        self.data.capacity() as Length
    }

    fn widen(&mut self, new_capacity: Length) -> SResult<()> {
        let capacity = self.data.capacity() as Length;
        let target = new_capacity.max(2 * capacity).min(Index::MAX.get() + 1);
        if target < new_capacity {
            return Err(StoreError::OutofMemory(capacity, new_capacity));
        }
        self.data.reserve_exact((target - capacity) as usize);
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
impl<T> Store<T> for SimpleStore<T> {}

impl<T> Get<Multi<T>> for SimpleStore<T> {
    fn get(&self, index: Range<Index>) -> SResult<&[T]> {
        let a = index.start.get();
        let b = index.end.get();
        self.data
            .get(a as usize..b as usize)
            .ok_or(StoreError::OutOfBounds(index.start, b.saturating_sub(a)))
    }

    fn get_mut(&mut self, index: Range<Index>) -> SResult<&mut [T]> {
        let a = index.start.get();
        let b = index.end.get();
        self.data
            .get_mut(a as usize..b as usize)
            .ok_or(StoreError::OutOfBounds(index.start, b.saturating_sub(a)))
    }
}
impl<T> GetDisjointMut<Multi<T>> for SimpleStore<T> {
    fn get_disjoint_mut<const N: usize>(
        &mut self,
        indices: [Range<Index>; N],
    ) -> SResult<[&mut [T]; N]> {
        self.data
            .get_disjoint_mut(indices.map(|i| i.start.get() as usize..i.end.get() as usize))
            .map_err(StoreError::from)
    }

    unsafe fn get_disjoint_unchecked_mut<const N: usize>(
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
}
impl<T> InsertIndirect<Multi<T>> for SimpleStore<T> {
    type Guard<'a>
        = InsertIndirectGuard<'a, T>
    where
        Self: 'a;

    fn insert_indirect_within_capacity(
        &mut self,
        len: Length,
    ) -> Option<(Range<Index>, InsertIndirectGuard<'_, T>)> {
        let len = len as usize;
        if self.data.len() + len > self.data.capacity() {
            return None;
        }
        // SAFETY: all indices within capacity are valid
        let begin = unsafe { Index::new_unchecked(self.data.len() as u32) };
        // SAFETY: all indices within capacity are valid
        let end = unsafe { Index::new_unchecked((self.data.len() + len) as u32) };
        Some((begin..end, InsertIndirectGuard { data: &mut self.data, len }))
    }
}
impl<T> MultiStore<T> for SimpleStore<T> {}
