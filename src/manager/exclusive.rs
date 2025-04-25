use std::marker::PhantomData;

use generativity::{Guard, Id};

use super::ManagerError;
use crate::store::*;

#[derive(Debug)]
pub struct ExclusiveHandle<'man, T: ?Sized> {
    index:   Index,
    manager: Id<'man>,
    _marker: PhantomData<fn() -> T>,
}
pub type XHandle<'man, T> = ExclusiveHandle<'man, T>;

#[derive(Debug)]
pub struct ExclusiveManager<'id, T, S> {
    store:   S,
    id:      Id<'id>,
    _marker: PhantomData<T>,
}
pub type XManager<'id, T, S> = ExclusiveManager<'id, T, S>;
impl<'id, T, S> XManager<'id, T, S>
where
    S: Default,
{
    pub fn new(guard: Guard<'id>) -> Self {
        Self { store: S::default(), id: guard.into(), _marker: PhantomData }
    }
}
impl<'id, T, S: Store<T>> XManager<'id, T, S> {
    pub fn get(&self, handle: &XHandle<'id, T>) -> Result<&T, ManagerError> {
        self.store.get(handle.index).map_err(ManagerError::from)
    }
    pub fn get_mut(&mut self, handle: &mut XHandle<'id, T>) -> Result<&mut T, ManagerError> {
        self.store.get_mut(handle.index).map_err(ManagerError::from)
    }
    pub fn get_disjoint_mut<const N: usize>(
        &mut self,
        handles: [&mut XHandle<'id, T>; N],
    ) -> [&mut T; N] {
        // SAFETY: exclusive handles are disjoint by definition
        unsafe { self.store.get_disjoint_unchecked_mut(handles.map(|handle| handle.index)) }
    }
    pub fn insert_within_capacity(&mut self, data: T) -> Result<XHandle<'id, T>, T> {
        self.store.insert_within_capacity(data).map(|index| XHandle {
            index,
            manager: self.id,
            _marker: PhantomData,
        })
    }
    pub fn reserve(&mut self, additional: usize) -> Result<(), ManagerError> {
        self.store.reserve(additional).map_err(ManagerError::from)
    }
    /// # Safety
    /// This does not invalidate existing [`XHandle`]s.
    /// Using such a handle is undefined behaviour.
    pub unsafe fn force_clear(&mut self) {
        self.store.clear();
    }
    pub fn into_empty(mut self, guard: Guard<'_>) -> XManager<'_, T, S> {
        self.store.clear();
        XManager { store: self.store, id: guard.into(), _marker: PhantomData }
    }
}
impl<'id, T, S: ReusableStore<T>> XManager<'id, T, S> {
    pub fn remove(
        &mut self,
        handle: XHandle<'id, T>,
    ) -> Result<T, (XHandle<'id, T>, ManagerError)> {
        self.store.remove(handle.index).map_err(|err| (handle, err.into()))
    }
}
impl<'id, T: Clone, S: MultiStore<T>> XManager<'id, T, S> {
    // TODO: methods for slice access
    // TODO: methods for mixed type access
}

#[cfg(any(test, doctest))]
mod test {
    use generativity::make_guard;

    use super::*;

    /// Handles can only be used in the manager that created them.
    /// ```compile_fail
    /// make_guard!(guard);
    /// let managerB = XManager::<bool, FreelistStore<bool>>::new(guard);
    /// make_guard!(guard);
    /// let mut managerA = XManager::<bool, FreelistStore<bool>>::new(guard);
    /// managerA.reserve(1)?;
    /// let handle = managerA.insert_within_capacity(true).unwrap();
    /// let val = managerB.get(&handle).unwrap();
    /// ```
    pub struct HandlesAreBranded;

    #[test]
    fn can_use_inner_store() {
        make_guard!(guard);
        let mut manager = XManager::<i16, FreelistStore<i16>>::new(guard);
        assert_eq!(Ok(()), manager.reserve(1));
        let handle = manager
            .insert_within_capacity(42)
            .expect("insert with spare capacity should be successful");
        assert_eq!(Ok(&42), manager.get(&handle));
        assert!(matches!(manager.remove(handle), Ok(42)));
    }
}
