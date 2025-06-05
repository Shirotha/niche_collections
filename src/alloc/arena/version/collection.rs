use std::mem::transmute;

use super::*;

/// # Safety
/// The `'id` lifetime has to correspond to a lifetime provideed by a [`Guard`],
/// otherwise a type implementing this trait can violate the aliasing rule.
pub unsafe trait NeedGuard {
    type Type<'id>;
}

/// This type provides a mechanism to store multiple instances of a type holding a [`Guard`].
/// Usually these instances would be incompatible because of the different guard lifetimes.
///
/// This Type only guaranties that there will never be two distinct instances with the same lifetime,
/// but it will *not* guaranty that two instances with different lifetimes are distinct.
// ASSERT: there is no way to retrive a `T::Type<'g>` from a `Guarded<'g, T>`
#[repr(transparent)]
#[derive(Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Guarded<'g, T>(T::Type<'g>)
where
    T: NeedGuard;
impl<'g, T> Guarded<'g, T>
where
    T: NeedGuard,
{
    pub fn from<'id>(value: T::Type<'id>) -> Self {
        // SAFETY: this is safe under this types assertion
        Self(unsafe { transmute::<T::Type<'id>, T::Type<'g>>(value) })
    }
    pub fn from_ref<'a, 'id>(value: &'a T::Type<'id>) -> &'a Self {
        // SAFETY: Guarded has the same layout as T::Type
        unsafe { transmute::<&'a T::Type<'id>, &'a Guarded<'g, T>>(value) }
    }
    pub fn from_mut<'a, 'id>(value: &'a mut T::Type<'id>) -> &'a mut Self {
        // SAFETY: Guarded has the same layout as T::Type
        unsafe { transmute::<&'a mut T::Type<'id>, &'a mut Guarded<'g, T>>(value) }
    }
    pub fn into<'id>(self, _guard: Guard<'id>) -> T::Type<'id> {
        // SAFETY: 'id is a valid guarded lifetime
        unsafe { transmute::<T::Type<'g>, T::Type<'id>>(self.0) }
    }
    pub fn get<'id>(&self, _guard: Guard<'id>) -> &T::Type<'id> {
        // SAFETY: 'id is a valid guarded lifetime
        unsafe { transmute::<&T::Type<'g>, &T::Type<'id>>(&self.0) }
    }
    pub fn get_mut<'id>(&mut self, _guard: Guard<'id>) -> &mut T::Type<'id> {
        // SAFETY: 'id is a valid guarded lifetime
        unsafe { transmute::<&mut T::Type<'g>, &mut T::Type<'id>>(&mut self.0) }
    }
}

// SAFETY: 'id is a valid guarded lifetime
unsafe impl<'man, K: Kind, S, H: Header> NeedGuard for VArena<'_, 'man, K, S, H> {
    type Type<'id> = VArena<'id, 'man, K, S, H>;
}

#[cfg(test)]
mod test {
    use generativity::make_guard;

    use super::*;

    #[derive(Debug)]
    struct TestGuard<'id> {
        value: i32,
        _id:   Id<'id>,
    }
    unsafe impl NeedGuard for TestGuard<'_> {
        type Type<'id> = TestGuard<'id>;
    }

    #[test]
    fn can_use_with_vec() {
        make_guard!(a);
        make_guard!(b);
        let a = TestGuard { value: 1, _id: a.into() };
        let b = TestGuard { value: 2, _id: b.into() };
        let vec = vec![Guarded::<'_, TestGuard>::from(a), Guarded::from(b)];
        assert_eq!(
            vec![1, 2],
            vec.into_iter()
                .map(|g| {
                    make_guard!(i);
                    g.get(i).value
                })
                .collect::<Vec<_>>()
        );
    }
}
