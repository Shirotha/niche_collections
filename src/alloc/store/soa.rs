use std::{
    alloc::{Layout, alloc, dealloc, handle_alloc_error},
    ptr::NonNull,
    slice,
};

use super::*;

// TODO: use this kinda structure for the other stores as well?
// TODO: use this type as only freelist store (support both Typed and Mixed/Slices by using custom headers and specialized impls)
#[derive(Debug)]
pub struct SoAFreelistStore<C> {
    /// # Memory layout
    /// - column pointers: `0`: `[NonNull<u8>; C::COUNT]`
    /// - layouts: `size_of(NonNull<u8>) * C::COUNT`: `[Layout; C::COUNT]`
    /// - occupation table: `(size_of(NonNull<u8>) + size_of(Layout)) * C::COUNT`: `[u8; cap.div_ceil(8)]`
    /// - for each columns:
    ///   - `+ size_of(last column).next_mul(align_of(this column))`: `size of(this column)`
    buffer:    NonNull<u8>,
    cap:       Length,
    next_free: Index,
    head:      Option<Index>,
    _marker:   PhantomData<C>,
}
impl<C> SoAFreelistStore<C> {}
impl<C> Default for SoAFreelistStore<C>
where
    C: Columns,
{
    fn default() -> Self {
        Self::new()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Header {
    offset: usize,
    layout: Layout,
}
impl Header {
    fn new(layout: Layout) -> Self {
        Self { offset: 0, layout }
    }
}
impl<C> SoAFreelistStore<C>
where
    C: Columns,
{
    const DEFAULT_CAPACITY: Length = 16;

    const fn header_align() -> usize {
        align_of::<(NonNull<u8>, Layout)>()
    }
    // TODO: replace with C::COUNT sized array once stable
    const fn columns_ptr(&self) -> NonNull<NonNull<u8>> {
        // SAFETY: buffer holds the columns array at this point
        self.buffer.cast::<NonNull<u8>>()
    }
    /// # Safety
    /// This will give mutable access to the columns header wihout checks.
    #[expect(clippy::mut_from_ref, reason = "needed to access in parallel with layout")]
    const unsafe fn columns(&self) -> &mut [NonNull<u8>] {
        // SAFETY: buffer holds the columns array at this point
        unsafe { slice::from_raw_parts_mut(self.columns_ptr().as_mut(), C::COUNT) }
    }
    const fn columns_size() -> usize {
        C::COUNT * size_of::<NonNull<u8>>()
    }
    const fn layout_ptr(&self) -> NonNull<Layout> {
        // SAFETY: buffer holds the layout array at this point
        unsafe { self.buffer.add(Self::columns_size()).cast::<Layout>() }
    }
    /// # Safety
    /// This will give mutable access to the columns header wihout checks.
    #[expect(clippy::mut_from_ref, reason = "needed to access in parallel with columns")]
    const unsafe fn layout(&self) -> &mut [Layout] {
        // SAFETY: layout_ptr is a valid layout array
        unsafe { slice::from_raw_parts_mut(self.layout_ptr().as_mut(), C::COUNT) }
    }
    const fn layout_size() -> usize {
        C::COUNT * size_of::<Layout>()
    }
    const fn occupation_ptr(&self) -> NonNull<u8> {
        // SAFETY: buffer holds the occupation table at this point
        unsafe { self.buffer.add(Self::columns_size() + Self::layout_size()) }
    }
    const fn occupation_size(capacity: Length) -> usize {
        capacity.div_ceil(8) as usize
    }
    const fn header_size(capacity: Length) -> usize {
        Self::columns_size() + Self::layout_size() + Self::occupation_size(capacity)
    }

    const fn is_occupied(&self, index: Index) -> bool {
        // SAFETY: if index is in capacity, then chunk is a valid part of the header
        let chunk = unsafe { self.occupation_ptr().add(index.get() as usize / 8).read() };
        chunk >> (index.get() % 8) & 1 == 1
    }
    const fn set_occupied(&mut self, index: Index) {
        // SAFETY: if index is in capacity, then chunk is a valid part of the header
        let chunk = unsafe { self.occupation_ptr().add(index.get() as usize / 8).as_mut() };
        *chunk |= 1 << (index.get() % 8);
    }
    const fn clear_occupied(&mut self, index: Index) {
        // SAFETY: if index is in capacity, then chunk is a valid part of the header
        let chunk = unsafe { self.occupation_ptr().add(index.get() as usize / 8).as_mut() };
        *chunk &= !(1 << (index.get() % 8));
    }

    fn register_columns(capacity: Length) -> Result<(Vec<Header>, Layout), LayoutError> {
        let mut columns = Vec::with_capacity(C::COUNT);
        C::register_layout(capacity, &mut |layout: Layout| {
            columns.push(Header::new(layout));
        })?;
        debug_assert_eq!(columns.len(), C::COUNT);
        let mut offset = Self::header_size(capacity);
        let mut align = Self::header_align();
        for column in &mut columns {
            offset = offset.next_multiple_of(column.layout.align());
            column.offset = offset;
            offset += column.layout.size();
            if column.layout.align() > align {
                align = column.layout.align();
            }
        }
        let Ok(layout) = Layout::from_size_align(offset, align) else { panic!("invalid layout") };
        Ok((columns, layout))
    }
    fn update_columns(&mut self, new_capacity: Length) -> Result<(Layout, Layout), LayoutError> {
        let mut offset = Self::header_size(new_capacity);
        let mut old_size = Self::header_size(self.cap);
        let mut align = Self::header_align();
        // SAFETY: self can be mutable here
        let mut columns = unsafe { self.columns().iter_mut() };
        // SAFETY: self can be mutable here
        let mut layouts = unsafe { self.layout().iter_mut() };
        C::register_layout(new_capacity, &mut |new_layout: Layout| {
            // SAFETY: number of columns can't change
            let column = unsafe { columns.next().unwrap_unchecked() };
            // SAFETY: number of columns can't change
            let layout = unsafe { layouts.next().unwrap_unchecked() };
            old_size = old_size.next_multiple_of(layout.align()) + layout.size();
            *layout = new_layout;
            // SAFETY: this is only a temporary value
            *column = unsafe {
                NonNull::new_unchecked(offset.next_multiple_of(layout.align()) as *mut u8)
            };
            offset += new_layout.size();
            if new_layout.align() > align {
                align = new_layout.align();
            }
        })?;
        Ok((Layout::from_size_align(old_size, align)?, Layout::from_size_align(offset, align)?))
    }
    fn allocate_initialized(capacity: Length) -> SResult<NonNull<u8>> {
        let (headers, layout) = Self::register_columns(capacity)
            .map_err(|_| StoreError::InvalidLayout("invalid column layout"))?;
        // SAFETY: size is not zero
        let buffer = unsafe { alloc(layout) };
        let Some(buffer) = NonNull::new(buffer) else { handle_alloc_error(layout) };
        for (i, header) in headers.into_iter().enumerate() {
            // SAFETY: buffer is big enough to hold header
            unsafe {
                buffer.cast::<NonNull<u8>>().add(i).write(buffer.add(header.offset));
                buffer.add(Self::columns_size()).cast::<Layout>().add(i).write(header.layout);
            }
        }
        // SAFETY: buffer is big enough to hold occupation table
        unsafe {
            buffer
                .add(Self::columns_size() + Self::layout_size())
                .write_bytes(0, capacity.div_ceil(8) as usize)
        };
        Ok(buffer)
    }

    pub fn new() -> Self {
        Self::with_capacity(Self::DEFAULT_CAPACITY)
    }
    pub fn with_capacity(capacity: Length) -> Self {
        let buffer = Self::allocate_initialized(capacity).expect("failed to allocate memory");
        Self { buffer, cap: capacity, next_free: Index::ZERO, head: None, _marker: PhantomData }
    }
}

impl<C> Resizable for SoAFreelistStore<C>
where
    C: Columns,
{
    fn capacity(&self) -> Length {
        self.cap
    }

    fn widen(&mut self, new_capacity: Length) -> SResult<()> {
        if new_capacity <= self.cap {
            return Err(StoreError::Narrow(new_capacity, self.cap));
        }
        let (old_layout, new_layout) = self
            .update_columns(new_capacity)
            .map_err(|_| StoreError::InvalidLayout("invalid layout"))?;
        // SAFETY: layout is valid here
        let buffer = unsafe { alloc(new_layout) };
        let Some(buffer) = NonNull::new(buffer) else {
            handle_alloc_error(new_layout);
        };
        let old_header_size = Self::header_size(self.cap);
        let new_header_size = Self::header_size(new_capacity);
        debug_assert!(new_header_size >= old_header_size);
        // SAFETY: new header is larger than old header
        unsafe { buffer.copy_from_nonoverlapping(self.buffer, old_header_size) };
        // SAFETY: buffer is big enough to hold the header
        unsafe { buffer.add(old_header_size).write_bytes(0, new_header_size - old_header_size) };
        let mut new_columns = buffer.cast::<NonNull<u8>>();
        let mut old_offset = old_header_size;
        // NOTE: the old layout has to be re-calculated because the header was overritten in update_columns
        C::register_layout(self.cap, &mut |old_layout: Layout| {
            old_offset = old_offset.next_multiple_of(old_layout.align());
            // SAFETY: new_columns holds the offset from the base pointer of the new columns (set in update_columns)
            let new_column = unsafe { buffer.add(new_columns.read().as_ptr() as usize) };
            unsafe { new_columns.write(new_column) };
            // SAFETY: new column is at least as large as the the old column
            unsafe {
                new_column.copy_from_nonoverlapping(self.buffer.add(old_offset), old_layout.size())
            };
            // SAFETY: number of columns can't change
            new_columns = unsafe { new_columns.add(1) };
            old_offset += old_layout.size();
        })
        .map_err(|_| StoreError::InvalidLayout("old layout failed to rebuild"))?;
        // SAFETY: pointer and layout match at this point
        unsafe { dealloc(self.buffer.as_ptr(), old_layout) };
        self.buffer = buffer;
        Ok(())
    }

    fn clear(&mut self) {
        // SAFETY: buffer contains a valid occupied table at this point
        unsafe {
            self.buffer.add(Self::columns_size()).write_bytes(0, self.cap.div_ceil(8) as usize)
        };
        self.next_free = Index::ZERO;
        self.head = None;
    }
}
impl<C> Insert<Single<C>> for SoAFreelistStore<C>
where
    C: Columns,
{
    fn insert_within_capacity(&mut self, element: C) -> Result<Index, C> {
        let index = if let Some(head) = self.head {
            // SAFETY: self can be mutable here
            self.head = *C::as_freelist_entry(head, unsafe { self.columns() });
            head
        } else if self.next_free.get() < self.cap {
            let next_free = self.next_free;
            // SAFETY: all indices within capacity are valid
            self.next_free = unsafe { Index::new_unchecked(next_free.get() + 1) };
            next_free
        } else {
            return Err(element);
        };
        debug_assert!(!self.is_occupied(index));
        // SAFETY: self can be mutable here
        element.move_into(index, unsafe { self.columns() });
        self.set_occupied(index);
        Ok(index)
    }
}
impl<C, I> View<Rows<C, I>> for SoAFreelistStore<C>
where
    I: IntoIndex,
    C: Columns,
{
    fn view(&self) -> <Rows<C, I> as Element>::Ref<'_> {
        // SAFETY: columns is restricted to read-only access here
        C::make_ref(unsafe { self.columns() }, self.occupation_ptr())
    }

    fn view_mut(&mut self) -> <Rows<C, I> as Element>::Mut<'_> {
        // SAFETY: mutable access is valid here
        C::make_mut(unsafe { self.columns() }, self.occupation_ptr())
    }
}
impl<C, I> SoAStore<C, I> for SoAFreelistStore<C>
where
    C: Columns,
    I: IntoIndex,
{
}

impl<C> Remove<Single<C>> for SoAFreelistStore<C>
where
    C: Columns,
{
    fn remove(&mut self, index: Index) -> SResult<C> {
        if index >= self.next_free {
            return Err(StoreError::OutOfBounds(index, self.next_free.get()));
        }
        if !self.is_occupied(index) {
            return Err(StoreError::DoubleFree(index));
        }
        // SAFETY: self can be mutable here
        let element = C::take(index, unsafe { self.columns() });
        self.clear_occupied(index);
        Ok(element)
    }
}
impl<C, I> ReusableSoAStore<C, I> for SoAFreelistStore<C>
where
    C: Columns,
    I: IntoIndex,
{
}
