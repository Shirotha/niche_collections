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

impl<T> Get<Multi<T>> for IntervaltreeStore<T> {
    fn get(&self, index: <Multi<T> as Element>::Index) -> SResult<<Multi<T> as Element>::Ref<'_>> {
        todo!()
    }

    fn get_mut(
        &mut self,
        index: <Multi<T> as Element>::Index,
    ) -> SResult<<Multi<T> as Element>::Mut<'_>> {
        todo!()
    }
}
impl<T> InsertIndirect<Multi<T>> for IntervaltreeStore<T> {
    type Guard<'a>
        = InsertIndirectGuard<'a, T>
    where
        Self: 'a;

    fn insert_indirect_within_capacity(
        &mut self,
        args: Length,
    ) -> Option<(<Multi<T> as Element>::Index, Self::Guard<'_>)> {
        todo!()
    }
}
impl<T> Resizable for IntervaltreeStore<T> {
    fn capacity(&self) -> Length {
        todo!()
    }

    fn widen(&mut self, new_capacity: Length) -> SResult<()> {
        todo!()
    }
    fn clear(&mut self) {
        todo!()
    }
}
impl<T> MultiStore<T> for IntervaltreeStore<T> {}

pub struct IntervaltreeRemoveGuard<'a, T>(PhantomData<&'a T>);
impl<'a, T> AsRef<&'a [T]> for IntervaltreeRemoveGuard<'a, T> {
    fn as_ref(&self) -> &&'a [T] {
        todo!()
    }
}
impl<T> RemoveIndirect<Multi<T>> for IntervaltreeStore<T> {
    type Guard<'a>
        = IntervaltreeRemoveGuard<'a, T>
    where
        Self: 'a;

    fn remove_indirect(&mut self, index: <Multi<T> as Element>::Index) -> SResult<Self::Guard<'_>> {
        todo!()
    }
}
impl<T> ReusableMultiStore<T> for IntervaltreeStore<T> {}
