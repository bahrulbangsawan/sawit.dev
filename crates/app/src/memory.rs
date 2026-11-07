//! Sawit's memory allocator: the system's, except that a block of [`LARGE`] bytes or more is
//! mapped from the kernel on its own and unmapped when freed.
//!
//! macOS's allocator keeps freed large blocks in a cache to hand out again, still dirty and still
//! counted against the process, and only reuses one for a request of about the same size. The
//! canvas makes a frame of a few tens of MB each time it renders (a 27 MB BGRA image with the
//! window filling a Retina screen), in a new size whenever the canvas is resized or zoomed, and the
//! frames it frees piled up there: 200 MB and more of freed memory with only the canvas open
//! (`Malloc Large (empty)` in `vmmap`). Mapped blocks go back to the system as soon as they are
//! freed. A new block costs its page faults (about 0.75 ms for 27 MB), small beside the render
//! that fills it.

use std::alloc::{GlobalAlloc, Layout, System};
use std::ptr;

/// Blocks this large or larger are mapped on their own.
pub const LARGE: usize = 4 << 20;

pub struct SawitAlloc;

/// The smallest page macOS has (Intel's; Apple silicon's are 16 KB): every mapping is aligned to
/// at least this.
const MIN_PAGE: usize = 4096;

/// Whether `layout` is mapped rather than given by the system allocator. A mapping is aligned to
/// the page; a block that asks for more stays with the system.
#[inline]
fn mapped(layout: Layout) -> bool {
    layout.size() >= LARGE && layout.align() <= MIN_PAGE
}

/// `size` bytes of fresh, zeroed pages, or null.
#[cold]
#[inline(never)]
unsafe fn map(size: usize) -> *mut u8 {
    // SAFETY: an anonymous private mapping at an address the kernel picks.
    let at = unsafe {
        libc::mmap(
            ptr::null_mut(),
            size,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANON,
            -1,
            0,
        )
    };
    if at == libc::MAP_FAILED {
        ptr::null_mut()
    } else {
        at.cast()
    }
}

/// Gives back a mapping [`map`] made of `size` bytes.
#[cold]
#[inline(never)]
unsafe fn unmap(at: *mut u8, size: usize) {
    // SAFETY: `at` and `size` are a whole mapping `map` made.
    unsafe {
        libc::munmap(at.cast(), size);
    }
}

// SAFETY: every block is either the system allocator's or a mapping of its own, decided by its
// layout alone (`mapped`), which is the same when the block is freed or resized as when it was
// made; mappings are page aligned, which covers every alignment `mapped` accepts. The methods are
// kept out of line: inlined into every allocation in the program they cost megabytes of code.
unsafe impl GlobalAlloc for SawitAlloc {
    #[inline(never)]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if mapped(layout) {
            unsafe { map(layout.size()) }
        } else {
            unsafe { System.alloc(layout) }
        }
    }

    #[inline(never)]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if mapped(layout) {
            // Fresh pages are zero already.
            unsafe { map(layout.size()) }
        } else {
            unsafe { System.alloc_zeroed(layout) }
        }
    }

    #[inline(never)]
    unsafe fn dealloc(&self, at: *mut u8, layout: Layout) {
        if mapped(layout) {
            unsafe { unmap(at, layout.size()) }
        } else {
            unsafe { System.dealloc(at, layout) }
        }
    }

    #[inline(never)]
    unsafe fn realloc(&self, at: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: the caller's promise for `realloc`: `new_size`, rounded up to the alignment,
        // doesn't overflow `isize`.
        let new_layout = unsafe { Layout::from_size_align_unchecked(new_size, layout.align()) };
        if !mapped(layout) && !mapped(new_layout) {
            return unsafe { System.realloc(at, layout, new_size) };
        }
        unsafe { move_block(at, layout, new_layout) }
    }
}

/// A resize to or from a mapped block: a new block, the bytes copied, the old one freed.
#[cold]
#[inline(never)]
unsafe fn move_block(at: *mut u8, layout: Layout, new_layout: Layout) -> *mut u8 {
    let new = unsafe { SawitAlloc.alloc(new_layout) };
    if !new.is_null() {
        unsafe {
            ptr::copy_nonoverlapping(at, new, layout.size().min(new_layout.size()));
            SawitAlloc.dealloc(at, layout);
        }
    }
    new
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Small and large blocks, made, zeroed, grown across the line and shrunk back, keep their
    /// bytes, and large ones are page aligned.
    #[test]
    fn blocks_on_both_sides_of_the_line_keep_their_bytes() {
        let a = SawitAlloc;
        unsafe {
            let small = Layout::from_size_align(1000, 8).unwrap();
            let p = a.alloc(small);
            ptr::write_bytes(p, 7, 1000);

            // Grown past LARGE: mapped, the old bytes kept.
            let big = a.realloc(p, small, LARGE + 10);
            assert!(!big.is_null());
            assert_eq!(big as usize % MIN_PAGE, 0);
            assert!((0..1000).all(|i| *big.add(i) == 7));
            ptr::write_bytes(big.add(1000), 9, LARGE + 10 - 1000);

            // Larger still, then back under the line.
            let big_layout = Layout::from_size_align(LARGE + 10, 8).unwrap();
            let bigger = a.realloc(big, big_layout, 2 * LARGE);
            assert!(*bigger.add(999) == 7 && *bigger.add(LARGE + 9) == 9);
            let bigger_layout = Layout::from_size_align(2 * LARGE, 8).unwrap();
            let back = a.realloc(bigger, bigger_layout, 500);
            assert!((0..500).all(|i| *back.add(i) == 7));
            a.dealloc(back, Layout::from_size_align(500, 8).unwrap());

            // Zeroed, both kinds.
            for size in [64, LARGE, 3 * LARGE + 1] {
                let layout = Layout::from_size_align(size, 16).unwrap();
                let z = a.alloc_zeroed(layout);
                assert!((0..size).step_by(4093).all(|i| *z.add(i) == 0));
                a.dealloc(z, layout);
            }
        }
    }

    /// A block that asks for more than page alignment stays with the system allocator.
    #[test]
    fn over_aligned_blocks_stay_with_the_system() {
        let layout = Layout::from_size_align(LARGE, MIN_PAGE * 8).unwrap();
        assert!(!mapped(layout));
        unsafe {
            let a = SawitAlloc;
            let p = a.alloc(layout);
            assert_eq!(p as usize % (MIN_PAGE * 8), 0);
            a.dealloc(p, layout);
        }
    }
}
