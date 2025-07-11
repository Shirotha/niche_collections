use std::{
    alloc::{Layout, alloc, dealloc, handle_alloc_error},
    ptr::NonNull,
};

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ColumnsData {
    pub typeid: TypeId,
    pub layout: Layout,
    pub offset: usize,
}
impl ColumnsData {
    pub fn new(typeid: TypeId, layout: Layout) -> Self {
        Self { typeid, layout, offset: 0 }
    }
}

// TODO: use this kinda structure for the other stores as well?
// TODO: use this type as only freelist store (support both Typed and Mixed/Slices by using custom headers and specialized impls)
#[derive(Debug)]
pub struct SoAFreelistStore<C> {
    /// # Memory layout
    /// - column data: `0`: `[ColumnsData; C::COUNT]`
    /// - occupation: `size_of(ColumnsData) * cols`: `[u8; cap.div_ceil(8)]`
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
impl<C> SoAFreelistStore<C>
where
    C: Columns,
{
    const DEFAULT_CAPACITY: Length = 16;

    const fn columns_size() -> usize {
        C::COUNT * size_of::<ColumnsData>()
    }
    const fn occupation_size(capacity: Length) -> usize {
        capacity.div_ceil(8) as usize
    }
    const fn header_size(capacity: Length) -> usize {
        Self::columns_size() + Self::occupation_size(capacity)
    }

    const fn is_occupied(occupation: NonNull<u8>, index: Index) -> bool {
        // SAFETY: if index is in capacity, then chunk is a valid part of the header
        let chunk = unsafe { occupation.add(index.get() as usize / 8).read() };
        chunk >> (index.get() % 8) & 1 == 1
    }
    const fn set_occupied(occupation: NonNull<u8>, index: Index) {
        // SAFETY: if index is in capacity, then chunk is a valid part of the header
        let chunk = unsafe { occupation.add(index.get() as usize / 8).as_mut() };
        *chunk |= 1 << (index.get() % 8);
    }
    const fn clear_occupied(occupation: NonNull<u8>, index: Index) {
        // SAFETY: if index is in capacity, then chunk is a valid part of the header
        let chunk = unsafe { occupation.add(index.get() as usize / 8).as_mut() };
        *chunk &= !(1 << (index.get() % 8));
    }

    fn register_columns(capacity: Length) -> Result<(Vec<ColumnsData>, Layout), LayoutError> {
        let mut columns = Vec::with_capacity(C::COUNT);
        C::register_layout(capacity, &mut |typeid: TypeId, layout: Layout| {
            columns.push(ColumnsData::new(typeid, layout));
        })?;
        debug_assert_eq!(columns.len(), C::COUNT);
        let mut offset = Self::header_size(capacity);
        let mut align = align_of::<ColumnsData>();
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
    fn update_columns(
        mut columns: NonNull<ColumnsData>,
        old_capacity: Length,
        new_capacity: Length,
    ) -> Result<(Layout, Layout), LayoutError> {
        let mut offset = Self::header_size(new_capacity);
        let mut old_size = Self::header_size(old_capacity);
        let mut align = align_of::<ColumnsData>();
        C::register_layout(new_capacity, &mut |typeid: TypeId, layout: Layout| {
            let column = unsafe { columns.as_mut() };
            assert_eq!(column.typeid, typeid);
            old_size = old_size.next_multiple_of(column.layout.align()) + column.layout.size();
            column.layout = layout;
            column.offset = offset.next_multiple_of(layout.align());
            offset += layout.size();
            if layout.align() > align {
                align = layout.align();
            }
        })?;
        Ok((Layout::from_size_align(old_size, align)?, Layout::from_size_align(offset, align)?))
    }
    fn allocate_initialized(capacity: Length) -> SResult<NonNull<u8>> {
        let (mut columns, layout) = Self::register_columns(capacity)
            .map_err(|_| StoreError::InvalidLayout("invalid column layout"))?;
        // SAFETY: size is not zero
        let buffer = unsafe { alloc(layout) };
        let Some(buffer) = NonNull::new(buffer) else { handle_alloc_error(layout) };
        // SAFETY: buffer is big enough to hold columns
        unsafe {
            buffer.copy_from_nonoverlapping(
                NonNull::new_unchecked(columns.as_mut_ptr() as *mut u8),
                C::COUNT,
            )
        };
        // SAFETY: buffer is big enough to hold occupation table
        unsafe { buffer.add(Self::columns_size()).write_bytes(0, capacity.div_ceil(8) as usize) };
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
        let (old_layout, new_layout) =
            Self::update_columns(self.buffer.cast::<ColumnsData>(), self.cap, new_capacity)
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
        let mut new_columns = buffer.cast::<ColumnsData>();
        let mut old_offset = old_header_size;
        // NOTE: the old layout has to be re-calculated because the header was overritten in update_columns
        C::register_layout(self.cap, &mut |_, old_layout: Layout| {
            old_offset = old_offset.next_multiple_of(old_layout.align());
            // SAFETY: new_columns is a valid ColumnsData array (initialized in update_columns)
            let new_column = unsafe { new_columns.as_ref() };
            // SAFETY: new column is at least as large as the the old column
            unsafe {
                buffer
                    .add(new_column.offset)
                    .copy_from_nonoverlapping(self.buffer.add(old_offset), old_layout.size())
            };
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
            self.head = *C::as_freelist_entry(head, &mut |col| {
                let column = unsafe { self.buffer.cast::<ColumnsData>().add(col).as_ref() };
                // SAFETY: ColumnsData contains valid column pointers at this point
                unsafe { self.buffer.add(column.offset) }
            });
            head
        } else if self.next_free.get() < self.cap {
            let next_free = self.next_free;
            // SAFETY: all indices within capacity are valid
            self.next_free = unsafe { Index::new_unchecked(next_free.get() + 1) };
            next_free
        } else {
            return Err(element);
        };
        // SAFETY: buffer holds a valid occupation table at this point
        let occupation = unsafe { self.buffer.add(Self::columns_size()) };
        debug_assert!(!Self::is_occupied(occupation, index));
        let mut columns = self.buffer.cast::<ColumnsData>();
        element.move_into(index, &mut || {
            // SAFETY: columns is a valid ColumnsData pointer
            let offset = unsafe { columns.as_ref().offset };
            // SAFETY: move_into guaranties to never access more columns than availible
            columns = unsafe { columns.add(1) };
            // SAFETY: column is a valid pointer to a column
            unsafe { self.buffer.add(offset) }
        });
        Self::set_occupied(occupation, index);
        Ok(index)
    }
}

impl<C> SoAStore<C> for SoAFreelistStore<C> where C: Columns {}

impl<C> Remove<Single<C>> for SoAFreelistStore<C>
where
    C: Columns,
{
    fn remove(&mut self, index: Index) -> SResult<C> {
        if index >= self.next_free {
            return Err(StoreError::OutOfBounds(index, self.next_free.get()));
        }
        let occupation = unsafe { self.buffer.add(Self::columns_size()) };
        if !Self::is_occupied(occupation, index) {
            return Err(StoreError::DoubleFree(index));
        }
        let mut columns = self.buffer.cast::<ColumnsData>();
        let element = C::take(index, &mut || {
            // SAFETY: columns is a valid ColumnsData pointer
            let offset = unsafe { columns.as_ref().offset };
            // SAFETY: move_into guaranties to never access more columns than availible
            columns = unsafe { columns.add(1) };
            // SAFETY: column is a valid pointer to a column
            unsafe { self.buffer.add(offset) }
        });
        Self::clear_occupied(occupation, index);
        Ok(element)
    }
}
impl<C> ReusableSoAStore<C> for SoAFreelistStore<C> where C: Columns {}
