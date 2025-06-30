use super::*;

#[macro_export]
macro_rules! map_handle {
    ($handle:ident<$t:ty> $from:lifetime -> $to:lifetime) => {
        // SAFETY: there is no safety here
        unsafe { std::mem::transmute::<VHandle<$from, $t>, VHandle<$to, $t>>($handle) }
    };
}
pub(super) use map_handle;

#[derive(Debug, Clone, Copy)]
pub struct HandleMap<'from, 'to> {
    pub(super) _from: Id<'from>,
    pub(super) _to:   Id<'to>,
}
impl<'from, 'to> HandleMap<'from, 'to> {
    pub fn apply<M>(self, target: M::Container<'from>) -> M::Container<'to>
    where
        M: MappableHandle,
    {
        let handle = M::handle(&target);
        M::update(target, map_handle!(handle<M::Data> 'from -> 'to))
    }
    pub fn chain<'next>(self, other: HandleMap<'to, 'next>) -> HandleMap<'from, 'next> {
        HandleMap { _from: self._from, _to: other._to }
    }
}
pub trait MappableHandle {
    type Container<'id>;
    type Data: ?Sized;

    fn handle<'id>(target: &Self::Container<'id>) -> VHandle<'id, Self::Data>;

    fn update<'from, 'to>(
        from: Self::Container<'from>,
        to: VHandle<'to, Self::Data>,
    ) -> Self::Container<'to>;
}
impl<T> MappableHandle for VHandle<'_, T> {
    type Container<'id> = VHandle<'id, T>;
    type Data = T;

    fn handle<'id>(target: &Self::Container<'id>) -> VHandle<'id, Self::Data> {
        *target
    }

    fn update<'from, 'to>(
        _from: Self::Container<'from>,
        to: VHandle<'to, Self::Data>,
    ) -> Self::Container<'to> {
        to
    }
}
