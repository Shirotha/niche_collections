use std::cell::UnsafeCell;

use generativity::Guard;
use parking_lot::Mutex;

use super::ArenaError;
use crate::{manager::*, store::*};

macro_rules! manager {
    (ref $this:expr) => {
        // SAFETY: manager always holds a valid value
        unsafe { $this.manager.get().as_ref().unwrap_unchecked() }
    };
    (mut $this:expr) => {
        // SAFETY: manager always holds a valid value
        unsafe { $this.manager.get().as_mut().unwrap_unchecked() }
    };
}

#[derive(Debug)]
pub struct ExclusiveArena<'id, T, S> {
    manager:    UnsafeCell<XManager<'id, T, S>>,
    alloc_lock: Mutex<()>,
}
pub type XArena<'id, T, S> = ExclusiveArena<'id, T, S>;
// SAFETY: XArena is inherently concurrent by design
unsafe impl<T, S> Sync for XArena<'_, T, S> {}
impl<'id, T, S> XArena<'id, T, S>
where
    S: Default,
{
    pub fn new(guard: Guard<'id>) -> Self {
        Self { manager: UnsafeCell::new(XManager::new(guard)), alloc_lock: Mutex::new(()) }
    }
}
impl<'id, T, S> XArena<'id, T, S>
where
    S: Store<T>,
{
    pub fn get(&self, handle: &XHandle<'id, T>) -> Result<&T, ArenaError> {
        manager!(ref self).get(handle).map_err(ArenaError::from)
    }
    pub fn get_mut(&self, handle: &mut XHandle<'id, T>) -> Result<&mut T, ArenaError> {
        manager!(mut self).get_mut(handle).map_err(ArenaError::from)
    }
    pub fn insert_within_capacity(&self, data: T) -> Result<XHandle<'id, T>, T> {
        let _guard = self.alloc_lock.lock();
        manager!(mut self).insert_within_capacity(data)
    }
    pub fn reserve(&mut self, additional: usize) -> Result<(), ArenaError> {
        self.manager.get_mut().reserve(additional).map_err(ArenaError::from)
    }
    pub fn insert(&mut self, data: T) -> Result<XHandle<'id, T>, (T, ArenaError)> {
        match self.insert_within_capacity(data) {
            Ok(handle) => Ok(handle),
            Err(data) => {
                if let Err(err) = self.reserve(1) {
                    return Err((data, err));
                }
                let Ok(handle) = self.insert_within_capacity(data) else {
                    unreachable!("insert after reserve should always be successful")
                };
                Ok(handle)
            },
        }
    }
    /// # Safety
    /// This does not invalidate existing [`XHandle`]s.
    /// Using such a handle is undefined behaviour.
    pub unsafe fn force_clear(&mut self) {
        // SAFETY: assumptions are guarantied by caller
        unsafe { self.manager.get_mut().force_clear() }
    }
    pub fn into_empty(self, guard: Guard<'_>) -> XArena<'_, T, S> {
        XArena {
            manager:    UnsafeCell::new(self.manager.into_inner().into_empty(guard)),
            alloc_lock: self.alloc_lock,
        }
    }
}
impl<'id, T, S> XArena<'id, T, S>
where
    S: ReusableStore<T>,
{
    pub fn remove(&self, handle: XHandle<'id, T>) -> Result<T, (XHandle<'id, T>, ArenaError)> {
        let _guard = self.alloc_lock.lock();
        manager!(mut self).remove(handle).map_err(|(handle, err)| (handle, err.into()))
    }
}
