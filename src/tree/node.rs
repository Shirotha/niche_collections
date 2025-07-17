use std::{
    borrow::{Borrow, BorrowMut},
    marker::PhantomData,
    mem::transmute,
};

use derive_macros::Columns;

use crate::{alloc::prelude::*, internal::Sealed};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Color {
    Red   = 0,
    Black = 1,
}
#[cfg(target_has_atomic = "ptr")]
impl AsBits for Color {
    const SIZE: usize = 1;

    fn from_bits(bits: usize) -> Self {
        // SAFETY: bits were created using into_bits and are always valid
        unsafe { transmute(bits as u8) }
    }
    fn into_bits(self) -> usize {
        // SAFETY: transmuting from enum to backing type is always safe
        unsafe { transmute::<Self, u8>(self) as usize }
    }
}

pub trait OrderKind: Sealed {
    type Data<T>;

    fn default<T>() -> Self::Data<T>;
}
pub struct NoOrder;
impl Sealed for NoOrder {}
impl OrderKind for NoOrder {
    type Data<T> = ();

    fn default<T>() -> Self::Data<T> {}
}
pub struct WithOrder;
impl Sealed for WithOrder {}
impl OrderKind for WithOrder {
    type Data<T> = [Option<T>; 2];

    fn default<T>() -> Self::Data<T> {
        [None, None]
    }
}

pub trait Value {
    const NEED_PROPAGATION: bool = false;
    type Singular;
    // NOTE: default to () once its stable to do so
    type Cumulant: Default;
    type Order: OrderKind;

    #[inline(always)]
    #[allow(unused_variables, reason = "this is an intential empty default")]
    fn update_cumulant(
        cumulant: &mut Self::Cumulant,
        singular: &Self::Singular,
        children: [&Self::Cumulant; 2],
    ) {
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Singular<T, O: OrderKind = NoOrder>(T, PhantomData<fn() -> O>);
impl<T, O> From<T> for Singular<T, O>
where
    O: OrderKind,
{
    fn from(value: T) -> Self {
        Self(value, PhantomData)
    }
}
// FIXME: ambiguous impl
// impl<T, O> From<Singular<T, O>> for T
// where
//     O: OrderKind,
// {
//     fn from(value: Singular<T, O>) -> Self {
//         value.0
//     }
// }
impl<T, O> AsRef<T> for Singular<T, O>
where
    O: OrderKind,
{
    fn as_ref(&self) -> &T {
        &self.0
    }
}
impl<T, O> AsMut<T> for Singular<T, O>
where
    O: OrderKind,
{
    fn as_mut(&mut self) -> &mut T {
        &mut self.0
    }
}
impl<T, O> Borrow<T> for Singular<T, O>
where
    O: OrderKind,
{
    fn borrow(&self) -> &T {
        &self.0
    }
}
impl<T, O> BorrowMut<T> for Singular<T, O>
where
    O: OrderKind,
{
    fn borrow_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T, O> Value for Singular<T, O>
where
    O: OrderKind,
{
    type Singular = T;

    type Cumulant = ();

    type Order = O;
}

#[derive(Columns)]
#[internal]
pub(super) struct Node<'id, K, V>
where
    // NOTE: using AsRef would be prefered here, but without it being reflective it can cause problems
    K: Ord,
    V: Value,
{
    key:      K,
    #[cfg_attr(target_has_atomic = "ptr", as_bits)]
    color:    Color,
    singular: V::Singular,
    cumulant: V::Cumulant,
    parent:   Option<Ref<'id, K, V>>,
    #[freelist_entry]
    children: [Option<Ref<'id, K, V>>; 2],
    order:    <V::Order as OrderKind>::Data<Ref<'id, K, V>>,
}
pub(super) type Ref<'id, K, V> = VHandle<'id, Node<'id, K, V>>;
impl<K, V> Node<'_, K, V>
where
    K: Ord,
    V: Value,
{
    pub fn new(key: K, value: V::Singular, color: Color) -> Self {
        Self {
            key,
            color,
            singular: value,
            cumulant: V::Cumulant::default(),
            parent: None,
            children: [None, None],
            order: V::Order::default(),
        }
    }
}

fn test<'id, 'man, K: Ord, V: Value>(
    arena: &Arena<'id, 'man, SoA<Node<'id, K, V>>, Versioned<true>>,
    handle: Ref<'id, K, V>,
) -> Option<Color> {
    let lock = arena.read();
    let view = lock.view();
    view.color(handle).ok().map(|c| c.load())
}
