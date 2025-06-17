use parking_lot::{RwLockReadGuard, RwLockUpgradableReadGuard, RwLockWriteGuard};

use super::*;

#[derive(Debug)]
pub struct VArenaReadGuard<'a, 'id, 'man, K: Kind, S, H: Header> {
    pub(super) manager: RwLockReadGuard<'a, ManagerCell<'man, K, S>>,
    pub(super) port:    RwLockReadGuard<'a, (Id<'id>, H)>,
}
#[derive(Debug)]
pub struct VArenaWriteGuard<'a, 'id, 'man, K: Kind, S, H: Header> {
    pub(super) manager: RwLockReadGuard<'a, ManagerCell<'man, K, S>>,
    pub(super) port:    RwLockWriteGuard<'a, (Id<'id>, H)>,
}
#[derive(Debug)]
pub struct VArenaAllocGuard<'a, 'id, 'man, K: Kind, S, H: Header> {
    pub(super) manager: RwLockUpgradableReadGuard<'a, ManagerCell<'man, K, S>>,
    pub(super) port:    RwLockWriteGuard<'a, (Id<'id>, H)>,
}
macro_rules! manager {
    (ref $this:ident) => {
        // SAFETY: manager always holds a valid value
        unsafe { $this.manager.get().as_ref().unwrap_unchecked() }
    };
    (mut $this:ident) => {
        // SAFETY: manager always holds a valid value
        unsafe { $this.manager.get().as_mut().unwrap_unchecked() }
    };
    (lock $this:ident |$manager:ident| $body:expr) => {
        $this.manager.with_upgraded(|manager| {
            let $manager = unsafe { manager.get().as_mut().unwrap_unchecked() };
            $body
        })
    };
}
macro_rules! impl_read {
    ($type:ident) => {
        impl<'id, 'man, K, S, H> $type<'_, 'id, 'man, K, S, H>
        where
            K: Kind,
            S: Store<K::VElement>,
            H: Header,
        {
            pub fn header(&self) -> &H {
                &self.port.1
            }
        }
        impl<'id, 'man, T, S, H> $type<'_, 'id, 'man, Typed<T>, S, H>
        where
            S: Store<(Version, T)>,
            H: Header,
        {
            pub fn get(&self, handle: VHandle<'id, T>) -> AResult<&T> {
                Ok(manager!(ref self).get(map_handle!(handle<T> 'id -> 'man))?)
            }
        }
        impl<'id, 'man, U, S, H> $type<'_, 'id, 'man, Slices<U>, S, H>
        where
            U: RawBytes,
            S: MultiStore<U>,
            H: Header,
        {
            pub fn get<T>(&self, handle: VHandle<'id, [T]>) -> AResult<&[T]> {
                Ok(manager!(ref self).get(map_handle!(handle<[T]> 'id -> 'man))?)
            }
        }
        impl<'id, 'man, U, S, H> $type<'_, 'id, 'man, Mixed<U>, S, H>
        where
            U: RawBytes,
            S: MultiStore<U>,
            H : Header,
        {
            pub fn get<T>(&self, handle: VHandle<'id, T>) -> AResult<&T> {
                Ok(manager!(ref self).get(map_handle!(handle<T> 'id -> 'man))?)
            }
        }
    };
}
impl_read!(VArenaReadGuard);
impl_read!(VArenaWriteGuard);
impl_read!(VArenaAllocGuard);
macro_rules! impl_write {
    ($type:ident) => {
        impl<'id, 'man, K, S, H> $type<'_, 'id, 'man, K, S, H>
        where
            K: Kind,
            S: Store<K::VElement>,
            H: Header,
        {
            pub fn header_mut(&mut self) -> &mut H {
                &mut self.port.1
            }
        }
        impl<'id, 'man, T, S, H> $type<'_, 'id, 'man, Typed<T>, S, H>
        where
            S: Store<(Version, T)>,
            H: Header,
        {
            pub fn get_mut(&mut self, handle: VHandle<'id, T>) -> AResult<&mut T> {
                Ok(manager!(mut self).get_mut(map_handle!(handle<T> 'id -> 'man))?)
            }
            pub fn move_to<'to, M>(
                &mut self,
                _to: &mut VArena<'to, 'man, Typed<T>, S, H>,
                target: M::Container<'id>
            ) -> AResult<M::Container<'to>>
            where
                M: MappableHandle<Data = T>
            {
                let handle = M::handle(&target);
                let handle = manager!(mut self).bump_version(map_handle!(handle<T> 'id -> 'man))?;
                Ok(M::update(target, map_handle!(handle<T> 'man -> 'to)))
            }
        }
        impl<'id, 'man, T, S, H> $type<'_, 'id, 'man, Typed<T>, S, H>
        where
            S: Store<(Version, T)> + GetDisjointMut<Single<(Version, T)>>,
            H: Header,
        {
            pub fn get_disjoint_mut<const N: usize>(
                &mut self,
                handles: [VHandle<'id, T>; N],
            ) -> AResult<[&mut T; N]> {
                Ok(manager!(mut self)
                    .get_disjoint_mut(handles.map(|handle| map_handle!(handle<T> 'id -> 'man)))?)
            }
        }
        impl<'id, 'man, U, S, H> $type<'_, 'id, 'man, Slices<U>, S, H>
        where
            U: RawBytes,
            S: MultiStore<U>,
            H: Header,
        {
            pub fn get_mut<T>(&mut self, handle: VHandle<'id, [T]>) -> AResult<&mut [T]> {
                Ok(manager!(mut self).get_mut(map_handle!(handle<[T]> 'id -> 'man))?)
            }
            pub fn move_to<'to, M, T>(
                &mut self,
                _to: &mut VArena<'to, 'man, Slices<U>, S, H>,
                target: M::Container<'id>
            ) -> AResult<M::Container<'to>>
            where
                M: MappableHandle<Data = [T]>
            {
                let handle = M::handle(&target);
                let handle = manager!(mut self).bump_version(map_handle!(handle<[T]> 'id -> 'man))?;
                Ok(M::update(target, map_handle!(handle<[T]> 'man -> 'to)))
            }
        }
        impl<'id, 'man, U, S, H> $type<'_, 'id, 'man, Slices<U>, S, H>
        where
            U: RawBytes,
            S: MultiStore<U> + GetDisjointMut<Multi<U>>,
            H: Header,
        {
            pub fn get_disjoint_mut<const N: usize, T>(
                &mut self,
                handles: [VHandle<'id, [T]>; N],
            ) -> AResult<[&mut [T]; N]> {
                Ok(manager!(mut self)
                    .get_disjoint_mut(handles.map(|handle| map_handle!(handle<[T]> 'id -> 'man)))?)
            }
        }
        impl<'id, 'man, U, S, H> $type<'_, 'id, 'man, Mixed<U>, S, H>
        where
            U: RawBytes,
            S: MultiStore<U>,
            H: Header,
        {
            pub fn get_mut<T>(&mut self, handle: VHandle<'id, T>) -> AResult<&mut T> {
                Ok(manager!(mut self).get_mut(map_handle!(handle<T> 'id -> 'man))?)
            }
            pub fn move_to<'to, M, T>(
                &mut self,
                _to: &mut VArena<'to, 'man, Mixed<U>, S, H>,
                target: M::Container<'id>
            ) -> AResult<M::Container<'to>>
            where
                M: MappableHandle<Data = T>
            {
                let handle = M::handle(&target);
                let handle = manager!(mut self).bump_version(map_handle!(handle<T> 'id -> 'man))?;
                Ok(M::update(target, map_handle!(handle<T> 'man -> 'to)))
            }
        }
        impl<'id, 'man, U, S, H> $type<'_, 'id, 'man, Mixed<U>, S, H>
        where
            U: RawBytes,
            S: MultiStore<U> + GetDisjointMut<Multi<U>>,
            H: Header,
        {
            pub fn get_disjoint_mut<const N: usize, T>(
                &mut self,
                handles: [VHandle<'id, T>; N],
            ) -> AResult<[&mut T; N]> {
                Ok(manager!(mut self)
                    .get_disjoint_mut(handles.map(|handle| map_handle!(handle<T> 'id -> 'man)))?)
            }
        }
    };
}
impl_write!(VArenaWriteGuard);
impl_write!(VArenaAllocGuard);
impl<'a, 'id, 'man, K, S, H> VArenaAllocGuard<'a, 'id, 'man, K, S, H>
where
    S: Resizable,
    K: Kind,
    H: Header,
{
    #[rustfmt::skip]
    pub fn reserve(&mut self, additional: Length) -> AResult<()> {
        manager!(lock self |manager| Ok(manager.reserve(additional)?))
    }
    pub fn downgrade(self) -> VArenaWriteGuard<'a, 'id, 'man, K, S, H> {
        VArenaWriteGuard {
            manager: RwLockUpgradableReadGuard::downgrade(self.manager),
            port:    self.port,
        }
    }
}
impl<'id, 'man, T, S, H> VArenaAllocGuard<'_, 'id, 'man, Typed<T>, S, H>
where
    S: Store<(Version, T)>,
    H: Header,
{
    pub fn insert_within_capacity(&mut self, data: T) -> Result<VHandle<'id, T>, T> {
        let handle = manager!(mut self).insert_within_capacity(data)?;
        Ok(map_handle!(handle<T> 'man -> 'id))
    }
    pub fn insert(&mut self, data: T) -> Result<VHandle<'id, T>, (T, ArenaError)> {
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
}
impl<'id, 'man, T, S, H> VArenaAllocGuard<'_, 'id, 'man, Typed<T>, S, H>
where
    S: ReusableStore<(Version, T)>,
    H: Header,
{
    pub fn remove(&mut self, handle: VHandle<'id, T>) -> AResult<T> {
        Ok(manager!(mut self).remove(map_handle!(handle<T> 'id -> 'man))?)
    }
}
impl<'id, 'man, U, S, H> VArenaAllocGuard<'_, 'id, 'man, Slices<U>, S, H>
where
    U: RawBytes,
    S: MultiStore<U>,
    H: Header,
{
    pub fn insert_within_capacity<T: Copy>(&mut self, data: &[T]) -> Option<VHandle<'id, [T]>> {
        let handle = manager!(mut self).insert_within_capacity(data)?;
        Some(map_handle!(handle<[T]> 'man -> 'id))
    }
    pub fn insert<T: Copy>(&mut self, data: &[T]) -> AResult<VHandle<'id, [T]>> {
        match self.insert_within_capacity(data) {
            Some(handle) => Ok(handle),
            None => {
                self.reserve(
                    Slices::<U>::header_size::<Version>()
                        + Slices::<U>::size_of::<T>(data.len() as Length),
                )?;
                let Some(handle) = self.insert_within_capacity(data) else {
                    unreachable!("insert after reserve should always be successful")
                };
                Ok(handle)
            },
        }
    }
}
impl<'id, 'man, U, S, H> VArenaAllocGuard<'_, 'id, 'man, Slices<U>, S, H>
where
    U: RawBytes,
    S: ReusableMultiStore<U>,
    H: Header,
{
    pub fn remove<T: Copy>(
        &mut self,
        handle: VHandle<'id, [T]>,
    ) -> AResult<<S as RemoveIndirect<Multi<U>>>::Guard<'_>> {
        Ok(manager!(mut self).remove(map_handle!(handle<[T]> 'id -> 'man))?)
    }
}
impl<'id, 'man, U, S, H> VArenaAllocGuard<'_, 'id, 'man, Mixed<U>, S, H>
where
    U: RawBytes,
    S: MultiStore<U>,
    H: Header,
{
    pub fn insert_within_capacity<T>(&mut self, data: T) -> Result<VHandle<'id, T>, T> {
        let handle = manager!(mut self).insert_within_capacity(data)?;
        Ok(map_handle!(handle<T> 'man -> 'id))
    }
    pub fn insert<T>(&mut self, data: T) -> Result<VHandle<'id, T>, (T, ArenaError)> {
        match self.insert_within_capacity(data) {
            Ok(handle) => Ok(handle),
            Err(data) => {
                if let Err(err) = self.reserve(Mixed::<U>::size_of::<(Version, T)>()) {
                    return Err((data, err));
                }
                let Ok(handle) = self.insert_within_capacity(data) else {
                    unreachable!("insert after reserve should always be successful")
                };
                Ok(handle)
            },
        }
    }
}
impl<'id, 'man, U, S, H> VArenaAllocGuard<'_, 'id, 'man, Mixed<U>, S, H>
where
    U: RawBytes,
    S: ReusableMultiStore<U>,
    H: Header,
{
    pub fn remove<T>(&mut self, handle: VHandle<'id, T>) -> AResult<T> {
        Ok(manager!(mut self).remove(map_handle!(handle<T> 'id -> 'man))?)
    }
}
