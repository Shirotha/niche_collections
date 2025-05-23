// - use a tree that stores allocated memory as Range<Index>
// - order by data.begin
// - also store as cumulant:
//   - total range
//     leaf = data
//     branch = left.total.begin..right.total.end
//   - biggest gap inside of total range
//     leaf = 0
//     branch = max[left.gap, data.begin - left.total.end, right.total.begin - data.end, right.gap]
// - find leftmost fitting gap by traversing tree:
//   - left.gap is big enough => go left
//   - data.begin - left.total.end is big enough => found gap
//   - right.total.begin - data.end is big enough => found gap
//   - right.gap is big enough => go right
//   - else no fitting gap exists (can only happen on root)
// TODO: if the leftmost element does not start at 0 the space cannot be reclaimed
//   => check first.data.begin >= gap manually before searching

use super::*;

pub struct IntervaltreeStore<T> {
    // HACK: temporary get rid of compiler error
    x: T,
}

impl<T> Store<T> for IntervaltreeStore<T> {
    fn get(&self, index: Index) -> Result<&T, StoreError> {
        todo!()
    }

    fn get_mut(&mut self, index: Index) -> Result<&mut T, StoreError> {
        todo!()
    }

    fn get_disjoint_mut<const N: usize>(
        &mut self,
        indices: [Index; N],
    ) -> Result<[&mut T; N], StoreError> {
        todo!()
    }

    unsafe fn get_disjoint_unchecked_mut<const N: usize>(
        &mut self,
        indices: [Index; N],
    ) -> [&mut T; N] {
        todo!()
    }

    fn insert_within_capacity(&mut self, data: T) -> Result<Index, T> {
        todo!()
    }

    fn reserve(&mut self, additional: Length) -> Result<(), StoreError> {
        todo!()
    }

    fn clear(&mut self) {
        todo!()
    }
}
impl<T> ReusableStore<T> for IntervaltreeStore<T> {
    fn remove(&mut self, index: Index) -> Result<T, StoreError> {
        todo!()
    }
}

impl<T: Clone> MultiStore<T> for IntervaltreeStore<T> {
    fn get_many(&self, index: Range<Index>) -> Result<&[T], StoreError> {
        todo!()
    }

    fn get_many_mut(&mut self, index: Range<Index>) -> Result<&mut [T], StoreError> {
        todo!()
    }

    fn get_many_disjoint_mut<const N: usize>(
        &mut self,
        indices: [Range<Index>; N],
    ) -> Result<[&mut [T]; N], StoreError> {
        todo!()
    }

    unsafe fn get_many_disjoint_unchecked_mut<const N: usize>(
        &mut self,
        indices: [Range<Index>; N],
    ) -> [&mut [T]; N] {
        todo!()
    }

    fn insert_many_within_capacity(
        &mut self,
        len: Length,
    ) -> Option<(Index, BeforeInsertMany<'_, T>)> {
        todo!()
    }
}
// impl<T: Clone> ReusableMultiStore<T> for IntervaltreeStore<T> {
//     fn remove_many(
//         &mut self,
//         index: Range<Index>,
//     ) -> Result<BeforeRemoveMany<'_, T, impl FnOnce()>, StoreError> {
//         todo!()
//     }
// }
