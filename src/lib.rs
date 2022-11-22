#![feature(
    ptr_metadata,
    alloc_layout_extra,
    layout_for_ptr,
    slice_ptr_get,
    pointer_byte_offsets,
    slice_index_methods
)]

use std::{
    alloc::{alloc, dealloc, handle_alloc_error, Layout, LayoutError},
    marker::PhantomData,
    mem::transmute,
    ops::{Index, IndexMut},
    ptr::{self, addr_of_mut, drop_in_place, from_raw_parts_mut},
};

#[repr(C)]
pub struct DstData<H: Sized, F: Sized> {
    header: H,
    footer: [F],
}

impl<H, F> DstData<H, F> {
    pub fn get_header(&self) -> &H {
        &self.header
    }

    pub fn get_header_mut(&mut self) -> &mut H {
        &mut self.header
    }

    pub fn get_footer(&self) -> &[F] {
        &self.footer
    }

    pub fn get_mut_footer(&mut self) -> &mut [F] {
        &mut self.footer
    }

    fn layout_of(count: usize) -> Result<Layout, LayoutError> {
        let (mut layout, _) = Layout::new::<H>().extend(Layout::array::<F>(count)?)?;
        layout = layout.pad_to_align();

        Ok(layout)
    }

    ///Returns a pointer to an uninitialized Dst
    unsafe fn alloc_self(count: usize) -> *mut Self {
        let layout = Self::layout_of(count).unwrap();

        let ptr = alloc(layout);

        if ptr.is_null() {
            handle_alloc_error(layout);
        } else {
            //Needed to make the pointer a fat pointer
            let ptr = from_raw_parts_mut::<DstData<H, F>>(ptr as *mut (), count);

            ptr
        }
    }

    ///Returns pointer to array of arraySize members where [F] has count elements (members are uninitialized)
    ///
    ///Also returns distance between each member of the array
    ///
    unsafe fn alloc_self_array(count: usize, array_size: usize) -> *mut Self {
        let (layout, _usize) = Self::layout_of(count).unwrap().repeat(array_size).unwrap();

        let ptr = alloc(layout);

        let ptr = ptr::slice_from_raw_parts(ptr, count) as *mut DstData<H, F>;

        ptr
    }

    unsafe fn get_footer_slice(ptr: *mut Self) -> *mut [F] {
        addr_of_mut!((*ptr).footer)
    }

    unsafe fn get_header_ptr(ptr: *mut Self) -> *mut H {
        addr_of_mut!((*ptr).header)
    }

    unsafe fn get_len(ptr: *const Self) -> usize {
        unsafe { (*ptr).footer.len() }
    }
}

impl<H, F> Drop for DstData<H, F> {
    fn drop(&mut self) {
        unsafe {
            drop_in_place(DstData::get_footer_slice(self));
        }
    }
}

pub struct MaybeUninitDst<H: Sized, F: Sized> {
    ptr: *mut DstData<H, F>,
}

impl<H, F> MaybeUninitDst<H, F> {
    pub fn new(count: usize) -> MaybeUninitDst<H, F> {
        MaybeUninitDst {
            ptr: unsafe { DstData::alloc_self(count) },
        }
    }

    pub fn write_header(&mut self, header: H) {
        unsafe {
            self.get_header_ptr_mut().write(header);
        }
    }

    pub fn write_footer(&mut self, footer: &[F]) {
        unsafe {
            let footer_ptr = self.get_footer_ptr_mut();
            let footer_len = self.get_footer_len();

            assert!(footer.len() == footer_len);

            ptr::copy_nonoverlapping(footer.as_ptr(), footer_ptr.as_mut_ptr(), footer_len);
        }
    }

    pub fn write_footer_element(&mut self, index: usize, element: F) {
        unsafe {
            let footer_len = self.get_footer_len();
            assert!(index < footer_len);

            let footer_ptr = self.get_footer_element_ptr_mut(index);

            footer_ptr.write(element);
        }
    }

    pub unsafe fn assume_init(self) -> Dst<H, F> {
        Dst { ptr: self.ptr }
    }

    ///Reading from this pointer or turning it into a reference is undefined behavior
    ///unless the header has been initialized
    pub fn get_header_ptr(&self) -> *const H {
        unsafe { DstData::get_header_ptr(self.ptr) as *const H }
    }

    ///Reading from this pointer or turning it into a reference is undefined behavior
    ///unless the header has been initialized
    pub fn get_header_ptr_mut(&mut self) -> *mut H {
        unsafe { DstData::get_header_ptr(self.ptr) }
    }

    ///Reading from this pointer or turning it into a reference is undefined behavior
    ///unless the footer has been initialized
    pub fn get_footer_ptr(&self) -> *const [F] {
        unsafe { DstData::get_footer_slice(self.ptr) as *const [F] }
    }

    ///Reading from this pointer or turning it into a reference is undefined behavior
    ///unless the footer has been initialized
    pub fn get_footer_ptr_mut(&mut self) -> *mut [F] {
        unsafe { DstData::get_footer_slice(self.ptr) }
    }

    ///Reading from this pointer or turning it into a reference is undefined behavior
    ///unless the element has been initialized
    pub fn get_footer_element_ptr(&self, index: usize) -> *const F {
        unsafe {
            (DstData::get_footer_slice(self.ptr) as *const [F])
                .as_ptr()
                .add(index)
        }
    }

    ///Reading from this pointer or turning it into a reference is undefined behavior
    ///unless the element has been initialized
    pub fn get_footer_element_ptr_mut(&self, index: usize) -> *mut F {
        unsafe {
            (DstData::get_footer_slice(self.ptr) as *mut [F])
                .as_mut_ptr()
                .add(index)
        }
    }

    pub fn get_footer_len(&self) -> usize {
        unsafe { DstData::get_len(self.ptr) }
    }
}

pub struct Dst<H: Sized, F: Sized> {
    ptr: *mut DstData<H, F>,
}

impl<H, F> Dst<H, F> {
    pub fn get_header_ref<'a>(&'a self) -> &'a H {
        unsafe { &(*self.ptr).header }
    }

    pub fn get_header_ref_mut<'a>(&'a mut self) -> &'a mut H {
        unsafe { &mut (*self.ptr).header }
    }

    pub fn get_footer_ref<'a>(&'a self) -> &'a [F] {
        unsafe { &(*self.ptr).footer }
    }

    pub fn get_footer_ref_mut<'a>(&'a mut self) -> &'a mut [F] {
        unsafe { &mut (*self.ptr).footer }
    }

    pub fn get_footer_len(&self) -> usize {
        self.get_footer_ref().len()
    }
}

impl<H, F> Drop for Dst<H, F> {
    fn drop(&mut self) {
        let layout = DstData::<H, F>::layout_of(self.get_footer_len()).unwrap();

        unsafe {
            drop_in_place(self.ptr);

            dealloc(self.ptr as *mut u8, layout);
        };
    }
}

pub struct MaybeUninitDstArray<H: Sized, F: Sized> {
    len: usize,
    ptr: *mut DstData<H, F>,
}

impl<H, F> MaybeUninitDstArray<H, F> {
    pub fn new(count: usize, array_size: usize) -> MaybeUninitDstArray<H, F> {
        MaybeUninitDstArray {
            len: array_size,
            ptr: unsafe { DstData::alloc_self_array(count, array_size) },
        }
    }

    fn get_stride(&self) -> usize {
        DstData::<H, F>::layout_of(self.get_footer_len())
            .unwrap()
            .size()
    }

    fn get_element(&self, arr_index: usize) -> MaybeUninitDst<H, F> {
        assert!(arr_index < self.len);

        let stride = self.get_stride();

        let ptr = unsafe { self.ptr.byte_add(stride * arr_index) };

        MaybeUninitDst { ptr }
    }

    pub unsafe fn assume_init(self) -> DstArray<H, F> {
        DstArray {
            len: self.len,
            ptr: self.ptr,
        }
    }

    fn get_footer_len(&self) -> usize {
        MaybeUninitDst { ptr: self.ptr }.get_footer_len()
    }

    pub fn write_header(&mut self, arr_index: usize, header: H) {
        self.get_element(arr_index).write_header(header);
    }

    pub fn write_footer(&mut self, arr_index: usize, footer: &[F]) {
        self.get_element(arr_index).write_footer(footer);
    }

    pub fn write_footer_element(&mut self, arr_index: usize, footer_index: usize, element: F) {
        self.get_element(arr_index)
            .write_footer_element(footer_index, element);
    }

    ///Reading from this pointer or turning it into a reference is undefined behavior
    ///unless the header of the element has been initialized
    pub fn get_header_ptr(&self, arr_index: usize) -> *const H {
        self.get_element(arr_index).get_header_ptr()
    }

    ///Reading from this pointer or turning it into a reference is undefined behavior
    ///unless the header of the element has been initialized
    pub fn get_header_ptr_mut(&mut self, arr_index: usize) -> *mut H {
        self.get_element(arr_index).get_header_ptr_mut()
    }

    ///Reading from this pointer or turning it into a reference is undefined behavior
    ///unless the footer of the element has been initialized
    pub fn get_footer_ptr(&self, arr_index: usize) -> *const [F] {
        self.get_element(arr_index).get_footer_ptr()
    }

    ///Reading from this pointer or turning it into a reference is undefined behavior
    ///unless the footer of the element has been initialized
    pub fn get_footer_ptr_mut(&mut self, arr_index: usize) -> *mut [F] {
        self.get_element(arr_index).get_footer_ptr_mut()
    }

    ///Reading from this pointer or turning it into a reference is undefined behavior
    ///unless the element has been , arr_index: usizeinitialized
    pub fn get_footer_element_ptr(&self, arr_index: usize, footer_index: usize) -> *const F {
        self.get_element(arr_index)
            .get_footer_element_ptr(footer_index)
    }

    ///Reading from this pointer or turning it into a reference is undefined behavior
    ///unless the element has been initialized
    pub fn get_footer_element_ptr_mut(&mut self, arr_index: usize, footer_index: usize) -> *mut F {
        self.get_element(arr_index)
            .get_footer_element_ptr_mut(footer_index)
    }
}

pub struct DstArray<H, F> {
    len: usize,
    ptr: *mut DstData<H, F>,
}

impl<H, F> DstArray<H, F> {
    fn get_stride(&self) -> usize {
        unsafe { DstData::<H, F>::layout_of(DstData::get_len(self.ptr)).unwrap() }.size()
    }

    pub fn get_header_ref<'a>(&'a self, arr_index: usize) -> &'a H {
        &self[arr_index].header
    }

    pub fn get_header_ref_mut<'a>(&'a mut self, arr_index: usize) -> &'a mut H {
        &mut self[arr_index].header
    }

    pub fn get_footer_ref<'a>(&'a self, arr_index: usize) -> &'a [F] {
        &self[arr_index].footer
    }

    pub fn get_footer_ref_mut<'a>(&'a mut self, arr_index: usize) -> &'a mut [F] {
        &mut self[arr_index].footer
    }

    pub fn get_footer_len(&self) -> usize {
        self.get_footer_ref(0).len()
    }

    pub fn get_mut_slice<'a>(&'a mut self, start: usize, end: usize) -> DstSliceMut<'a, H, F> {
        assert!(start < end);
        assert!(end <= self.len);

        let stride = self.get_stride();

        DstSliceMut {
            start: unsafe { self.ptr.byte_add(stride * start) },
            len: end - start,
            phantom: PhantomData,
        }
    }

    pub fn get_mut_arr_element<'a>(&'a mut self, index: usize) -> &'a mut DstData<H, F> {
        assert!(index < self.len);

        let stride = self.get_stride();

        unsafe {
            transmute::<*mut DstData<H, F>, &'a mut DstData<H, F>>(
                self.ptr.byte_add(stride * index),
            )
        }
    }
    
    pub fn get_arr_element<'a>(&'a self, index: usize) -> &'a DstData<H, F> {
        assert!(index < self.len);

        let stride = self.get_stride();

        unsafe {
            transmute::<*mut DstData<H, F>, &'a DstData<H, F>>(
                self.ptr.byte_add(stride * index),
            )
        }
    }
}

impl<H, F> Drop for DstArray<H, F> {
    fn drop(&mut self) {
        let stride = self.get_stride();

        let mut ptr = unsafe { self.ptr.byte_add(stride) };

        for _ in 0..self.len {
            unsafe {
                drop_in_place(ptr);
                ptr = ptr.byte_add(stride);
            }
        }

        let layout = DstData::<H, F>::layout_of(self.get_footer_len()).unwrap();

        unsafe {
            dealloc(self.ptr as *mut u8, layout);
        }
    }
}

impl<H, F> Index<usize> for DstArray<H, F> {
    type Output = DstData<H, F>;

    fn index(&self, index: usize) -> &DstData<H, F> {
        let stride = unsafe { DstData::<H, F>::layout_of((*self.ptr).footer.len()) }
            .unwrap()
            .size();

        let ptr = unsafe { self.ptr.byte_add(stride * index) };

        assert!(ptr <= unsafe { self.ptr.byte_add(stride * self.len) });

        unsafe { &*ptr }
    }
}

impl<H, F> IndexMut<usize> for DstArray<H, F> {
    fn index_mut(&mut self, index: usize) -> &mut DstData<H, F> {
        let stride = unsafe { DstData::<H, F>::layout_of((*self.ptr).footer.len()) }
            .unwrap()
            .size();

        let ptr = unsafe { self.ptr.byte_add(stride * index) };

        assert!(ptr <= unsafe { self.ptr.byte_add(stride * self.len) });

        unsafe { &mut *ptr }
    }
}

pub struct DstSliceMut<'a, H: Sized, F: Sized> {
    start: *mut DstData<H, F>,
    len: usize,
    phantom: PhantomData<&'a mut DstData<H, F>>,
}

impl<'a, H, F> Index<usize> for DstSliceMut<'a, H, F> {
    type Output = DstData<H, F>;

    fn index(&self, index: usize) -> &DstData<H, F> {
        assert!(index < self.len);

        let stride = unsafe { DstData::<H, F>::layout_of((*self.start).footer.len()) }
            .unwrap()
            .size();

        let ptr = unsafe { self.start.byte_add(stride * index) };

        unsafe { &*ptr }
    }
}

impl<'a, H, F> IndexMut<usize> for DstSliceMut<'a, H, F> {
    fn index_mut(&mut self, index: usize) -> &mut DstData<H, F> {
        assert!(index < self.len);

        let stride = unsafe { DstData::<H, F>::layout_of((*self.start).footer.len()) }
            .unwrap()
            .size();

        let ptr = unsafe { self.start.byte_add(stride * index) };

        unsafe { &mut *ptr }
    }
}

impl<'a, H, F> DstSliceMut<'a, H, F> {
    pub fn split_at_mut(&self, mid: usize) -> (DstSliceMut<H, F>, DstSliceMut<H, F>) {
        assert!(mid <= self.len);
        unsafe {
            let stride = DstData::<H, F>::layout_of((*self.start).footer.len())
                .unwrap()
                .size();
            let ptr = self.start.byte_add(mid * stride);

            let first = DstSliceMut {
                start: self.start,
                len: mid,
                phantom: PhantomData,
            };

            let second = DstSliceMut {
                start: ptr,
                len: self.len - mid,
                phantom: PhantomData,
            };

            (first, second)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writing() {
        let mut dst = MaybeUninitDst::<u32, u64>::new(2);

        dst.write_header(2);

        let header_ref = dst.get_header_ptr();

        unsafe { assert!(*header_ref == 2) }

        let footer = [1, 2];

        dst.write_footer(&footer);

        let footer_ref = dst.get_footer_ptr();

        unsafe {
            assert!((*footer_ref)[0] == 1);
            assert!((*footer_ref)[1] == 2);
        }
    }

    #[test]
    #[should_panic]
    fn invalid_footer_write() {
        let mut dst = MaybeUninitDst::<u32, u64>::new(2);

        let footer = [1, 2, 3];

        dst.write_footer(&footer);
    }

    #[test]
    #[should_panic]
    fn invalid_element_write() {
        let mut dst = MaybeUninitDst::<u32, u64>::new(2);

        dst.write_footer_element(3, 1);
    }

    #[test]
    fn element_write() {
        let mut dst = MaybeUninitDst::<u32, u64>::new(2);

        dst.write_footer_element(1, 1);

        let footer_element_ref = dst.get_footer_element_ptr(1);

        unsafe { assert!(*footer_element_ref == 1) }
    }

    #[test]
    fn element_write2() {
        let mut dst = MaybeUninitDst::<u32, u64>::new(3);

        let footer = [1, 2, 3];

        dst.write_footer(&footer);

        dst.write_footer_element(1, 5);

        let footer_ptr = dst.get_footer_ptr();

        unsafe {
            assert!((*footer_ptr)[0] == 1);
            assert!((*footer_ptr)[1] == 5);
            assert!((*footer_ptr)[2] == 3)
        }
    }

    #[test]
    fn assume_init() {
        let mut dst = MaybeUninitDst::<u8, u64>::new(5);

        dst.write_header(1);

        let footer = [0, 1, 2, 3, 4];

        dst.write_footer(&footer);

        let dst = unsafe { dst.assume_init() };

        assert!(dst.get_footer_len() == 5);

        assert!(dst.get_footer_ref().eq(&footer));

        assert!(*dst.get_header_ref() == 1);
    }

    #[test]
    fn array() {
        let mut dst_arr = MaybeUninitDstArray::<u32, u8>::new(2, 2);

        let mut arr = [0, 1];

        dst_arr.write_header(0, 0);
        dst_arr.write_footer(0, &arr);

        arr[1] = 5;

        dst_arr.write_header(1, 1);
        dst_arr.write_footer(1, &arr);

        let dst_arr = unsafe { dst_arr.assume_init() };

        assert!(*dst_arr.get_header_ref(0) == 0);
        assert!(dst_arr.get_footer_ref(0)[0] == 0);
        assert!(dst_arr.get_footer_ref(0)[1] == 1);

        assert!(*dst_arr.get_header_ref(1) == 1);
        assert!(dst_arr.get_footer_ref(1)[0] == 0);
        assert!(dst_arr.get_footer_ref(1)[1] == 5);
    }

    #[test]
    #[should_panic]
    fn array_invalid() {
        let mut dst_arr = MaybeUninitDstArray::<u32, u8>::new(2, 2);

        dst_arr.write_header(2, 1);
    }
}
