//! How glibc's allocator is told to hand memory back, once, before any thread is started.
//!
//! A buffer of a megabyte or more - a decoded photo, a level, a preview - is mapped on its
//! own and unmapped when it is freed. glibc's default does that too, but slides the
//! threshold up to the size of each mapped block freed (as far as 32 MB), so after the first
//! photos every later buffer comes out of a thread's arena instead, and an arena gives back
//! only its top: an import of 48 photos of 12 MP left 165 MB resident that held 4 MB still
//! in use, and `malloc_trim` returned none of it. With the threshold fixed the same import
//! left 4 MB, and peaked at 230 MB instead of 380, in the same time (2026-10-17, through the
//! engine's 8 thumbnail workers). Setting it switches the sliding off; the arenas stay.
//!
//! Only glibc: musl, Windows and macOS each have an allocator of their own, and no such
//! setting.

/// From this size up a buffer is mapped on its own, and unmapped when freed.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
const MMAP_THRESHOLD: libc::c_int = 1 << 20;

/// Fixes the threshold, for every thread's arena at once. Called first thing in `main`, before
/// any buffer of a photo is made under the sliding threshold.
pub fn tune() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: `mallopt` only sets a parameter of the allocator; it reads no memory of ours.
    unsafe {
        libc::mallopt(libc::M_MMAP_THRESHOLD, MMAP_THRESHOLD);
    }
}

#[cfg(all(test, target_os = "linux", target_env = "gnu"))]
mod tests {
    use super::*;

    /// How many blocks are mapped on their own now.
    fn mapped() -> usize {
        // SAFETY: `mallinfo2` returns a struct by value and has no preconditions.
        unsafe { libc::mallinfo2() }.hblks
    }

    // A large buffer freed leaves the threshold where it was, so the next one is mapped on
    // its own again - by glibc's default, the first freed had moved the threshold past it,
    // and the second came out of the arena, where it stays resident once freed.
    #[test]
    fn a_large_buffer_is_mapped_on_its_own_after_one_is_freed() {
        tune();
        let size = 4 << 20;
        drop(std::hint::black_box(vec![1u8; size]));
        let before = mapped();
        let second = std::hint::black_box(vec![1u8; size]);
        assert_eq!(mapped(), before + 1);
        drop(second);
    }
}
