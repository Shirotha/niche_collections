use std::{
    alloc::{Layout, alloc_zeroed, dealloc, handle_alloc_error},
    ptr::{NonNull, copy_nonoverlapping},
};

use super::*;

// TODO: use this kinda structure for the other stores as well?
#[derive(Debug)]
pub struct MaskedFreelistStore<M: Maskable, Inner = <M as Maskable>::Tuple> {
    data:      Option<NonNull<u8>>,
    cap:       Length,
    next_free: Index,
    head:      Option<Index>,
    _marker:   PhantomData<(M, Inner)>,
}
impl<M> MaskedFreelistStore<M>
where
    M: Maskable,
{
    const fn align_of() -> usize {
        align_of::<M::Tuple>()
    }

    pub fn new() -> Self {
        Self {
            data:      None,
            cap:       0,
            next_free: Index::ZERO,
            head:      None,
            _marker:   PhantomData,
        }
    }
}
impl<M> Default for MaskedFreelistStore<M>
where
    M: Maskable,
{
    fn default() -> Self {
        Self::new()
    }
}
macro_rules! impl_store {
    ($(($i:tt, $T:ident)),*) => {
        impl<M, $($T),*> MaskedFreelistStore<M, ($($T,)*)>
        where
            M: Maskable<Tuple = ($($T,)*)>,
        {
            const fn header_size(capacity: Length) -> usize {
                ((capacity / 8) as usize).next_multiple_of(Self::align_of())
            }
            const fn is_occupied(base: NonNull<u8>, index: Index) -> bool {
                // SAFETY: if index is in capacity, then chunk is a valid part of the header
                let chunk = unsafe { base.add(index.get() as usize / 8).read() };
                chunk >> (index.get() % 8) & 1 == 1
            }
            const fn set_occupied(base: NonNull<u8>, index: Index) {
                // SAFETY: if index is in capacity, then chunk is a valid part of the header
                let chunk = unsafe { base.add(index.get() as usize / 8).as_mut() };
                *chunk |= 1 << (index.get() % 8);
            }
            const fn clear_occupied(base: NonNull<u8>, index: Index) {
                // SAFETY: if index is in capacity, then chunk is a valid part of the header
                let chunk = unsafe { base.add(index.get() as usize / 8).as_mut() };
                *chunk &= !(1 << (index.get() % 8));
            }
            const fn needed_space(capacity: Length) -> usize {
                Self::offset_of(u8::MAX, capacity)
            }
            const fn size_of(i: u8) -> usize {
                [$({
                    let mut size = size_of::<$T>();
                    if $i == 0 && size < size_of::<Option<Index>>() {
                        size = size_of::<Option<Index>>();
                    }
                    size
                }),*][i as usize]
            }
            /// Returns the offset to the start of i-th array.
            /// Passing an ouf-of-bounds value for i will return an offset pointing behind the last array.
            const fn offset_of(i: u8, capacity: Length) -> usize {
                let mut result = Self::header_size(capacity);
                let capacity = capacity as usize;
                $({
                    result = result.next_multiple_of(align_of::<$T>());
                    if i == $i { return result }
                    let mut size = size_of::<$T>();
                    if i == 0 && size < size_of::<Index>() {
                        size = size_of::<Index>();
                    }
                    result += size * capacity;
                })*
                result
            }
            const fn layout(capacity: Length) -> Option<Layout> {
                let size = Self::needed_space(capacity);
                let align = Self::align_of();
                match Layout::from_size_align(size, align) {
                    Ok(layout) => Some(layout),
                    Err(_) => None
                }
            }
            fn allocate(capacity: Length) -> Option<NonNull<u8>> {
                let layout = Self::layout(capacity)?;
                // SAFETY: size is not zero
                let buffer = unsafe { alloc_zeroed(layout) };
                NonNull::new(buffer)
            }
            const fn copy(src: NonNull<u8>, dst: NonNull<u8>, size: usize) {
                unsafe { copy_nonoverlapping(src.as_ptr(), dst.as_ptr(), size) }
            }
            fn deallocate(base: NonNull<u8>, capacity: Length) {
                // SAFETY: base was allocated with the same layout
                let layout = unsafe { Self::layout(capacity).unwrap_unchecked() };
                // SAFETY: base and layout are both valid
                unsafe { dealloc(base.as_ptr(), layout) }
            }
            const fn pointer_for(base: NonNull<u8>, i: u8, capacity: Length) -> NonNull<u8> {
                assert!((i as usize) < M::Tuple::LEN, "i is out of bounds");
                // SAFETY: offset_of will always produce a aligned pointer that is inbounds, if base was allocated using allocate_for_capacity(capacity)
                unsafe { base.add(Self::offset_of(i, capacity)) }
            }

            pub fn with_capacity(capacity: Length) -> Self {
                Self {
                    data:    Self::allocate(capacity),
                    cap:     capacity,
                    next_free: Index::ZERO,
                    head:    None,
                    _marker: PhantomData,
                }
            }
        }
        impl<M, $($T),*> Get<Masked<M>> for MaskedFreelistStore<M, ($($T,)*)>
        where
            M: Maskable<Tuple = ($($T,)*)>,
        {
            fn get(&self, index: (Index, Mask)) -> SResult<M::Ref<'_>> {
                if let Some(base) = self.data {
                    if index.0.get() >= self.cap {
                        return Err(StoreError::OutOfBounds(index.0, self.cap));
                    }
                    if !Self::is_occupied(base, index.0) {
                        return Err(StoreError::AccessAfterFree(index.0));
                    }
                    Ok(M::Ref::from(($({
                        if (index.1 >> $i) & 1 == 1 {
                            let array = Self::pointer_for(base, $i, self.cap);
                            // SAFETY: array is always a valid T pointer and index.0 is checked to be in-bounds
                            Some(unsafe { array.add(index.0.get() as usize * Self::size_of($i)).cast::<$T>().as_ref() })
                        } else { None }
                    },)*)))
                } else { Err(StoreError::OutOfBounds(index.0, 0)) }
            }
            fn get_mut(&mut self, index: (Index, Mask)) -> SResult<M::Mut<'_>> {
                if let Some(base) = self.data {
                    if index.0.get() >= self.cap {
                        return Err(StoreError::OutOfBounds(index.0, self.cap));
                    }
                    if !Self::is_occupied(base, index.0) {
                        return Err(StoreError::AccessAfterFree(index.0));
                    }
                    Ok(M::Mut::from(($({
                        if (index.1 >> $i) & 1 == 1 {
                            let array = Self::pointer_for(base, $i, self.cap);
                            // SAFETY: array is always a valid T pointer and index.0 is checked to be in-bounds
                            Some(unsafe { array.add(index.0.get() as usize * Self::size_of($i)).cast::<$T>().as_mut() })
                        } else { None }
                    },)*)))
                } else { Err(StoreError::OutOfBounds(index.0, 0)) }
            }
        }
        impl<M, $($T),*> Insert<Single<M>> for MaskedFreelistStore<M, ($($T,)*)>
        where
            M: Maskable<Tuple = ($($T,)*)> + Into<($($T,)*)>,
        {
            fn insert_within_capacity(&mut self, element: M) -> Result<Index, M> {
                if let Some(base) = self.data {
                    let index = if let Some(index) = self.head {
                        let freelist = Self::pointer_for(base, 0, self.cap);
                        // SAFETY: index is in-bounds and elements from the 0-th array are always valid freelist entries
                        let head = unsafe { freelist.add(index.get() as usize * Self::size_of(0)).cast::<Option<Index>>().read() };
                        self.head = head;
                        index
                    } else if self.next_free.get() < self.cap {
                        let index = self.next_free;
                        if let Some(next_free) = Index::new(index.get() + 1) {
                            self.next_free = next_free
                        } else {
                            return Err(element);
                        }
                        index
                    } else {
                        return Err(element);
                    };
                    let tuple: ($($T,)*) = element.into();
                    let i = index.get() as usize;
                    $({
                        let array = Self::pointer_for(base, $i, self.cap);
                        // SAFETY: array is always a valid T pointer and index is checked to be in-bounds
                        unsafe { array.add(i * Self::size_of($i)).cast::<$T>().write(tuple.$i) }
                    })*
                    Self::set_occupied(base, index);
                    Ok(index)
                } else { Err(element) }
            }
        }
        impl<M, $($T),*> Resizable for MaskedFreelistStore<M, ($($T,)*)>
        where
            M: Maskable<Tuple = ($($T,)*)> + Into<($($T,)*)>,
        {
            fn capacity(&self) -> Length {
                self.cap
            }

            fn widen(&mut self, new_capacity: Length) -> SResult<()> {
                if new_capacity <= self.cap {
                    return Err(StoreError::Narrow(new_capacity, self.cap));
                }
                let capacity = self.cap;
                let target = new_capacity.max(2 * capacity).min(Index::MAX.get() + 1);
                if target < new_capacity {
                    return Err(StoreError::OutofMemory(capacity, new_capacity));
                }
                let new_data = Self::allocate(target);
                let Some(new_base) = new_data else {
                    handle_alloc_error(Self::layout(target).unwrap())
                };
                if let Some(base) = self.data {
                    Self::copy(base, new_base, Self::header_size(capacity));
                    $({
                        let array = Self::pointer_for(base, $i, capacity);
                        let new_array = Self::pointer_for(new_base, $i, new_capacity);
                        Self::copy(array, new_array, capacity as usize * Self::size_of($i));
                    })*
                    Self::deallocate(base, capacity);
                }
                self.data = new_data;
                Ok(())
            }
            fn clear(&mut self) {
                self.head = None;
                self.next_free = Index::ZERO;
            }
        }
        impl<M, $($T),*> MaskedStore<M> for MaskedFreelistStore<M, ($($T,)*)>
        where
            M: Maskable<Tuple = ($($T,)*)> + Into<($($T,)*)>, {}
        impl<M, $($T),*> Remove<Single<M>> for MaskedFreelistStore<M, ($($T,)*)>
        where
            M: Maskable<Tuple = ($($T,)*)> + Into<($($T,)*)>,
        {
            fn remove(&mut self, index: Index) -> SResult<M> {
                if let Some(base) = self.data {
                    if index.get() >= self.cap {
                        return Err(StoreError::OutOfBounds(index, self.cap));
                    }
                    if !Self::is_occupied(base, index) {
                        return Err(StoreError::DoubleFree(index));
                    }
                    let i = index.get() as usize;
                    let result = M::from(($({
                        let array = Self::pointer_for(base, $i, self.cap);
                        // SAFETY: array is always a valid T pointer and index is checked to be in-bounds
                        unsafe { array.add(i * Self::size_of($i)).cast::<$T>().read() }
                    },)*));
                    let freelist = Self::pointer_for(base, 0, self.cap);
                    // SAFETY: index is in-bounds and elements from the 0-th array are always valid freelist entries
                    unsafe { freelist.add(i * Self::size_of(0)).cast::<Option<Index>>().write(self.head); }
                    self.head = Some(index);
                    Self::clear_occupied(base, index);
                    Ok(result)
                } else { Err(StoreError::OutOfBounds(index, 0)) }
            }
        }
        impl<M, $($T),*> ReusableMaskedStore<M> for MaskedFreelistStore<M, ($($T,)*)>
        where
            M: Maskable<Tuple = ($($T,)*)> + Into<($($T,)*)>, {}
    };
}
all_tuples_enumerated!(impl_store, 2, 16, T);
