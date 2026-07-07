//! WASM Memory Management
//!
//! Utilities for managing memory between host and WASM guest

use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, warn};

use super::types::{WasmError, WasmResult};

/// Guest pointer type (32-bit address in WASM linear memory)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GuestPtr(pub u32);

impl GuestPtr {
    pub fn new(addr: u32) -> Self {
        Self(addr)
    }

    pub fn offset(&self, bytes: u32) -> Self {
        Self(self.0.saturating_add(bytes))
    }

    pub fn as_usize(&self) -> usize {
        self.0 as usize
    }

    pub fn is_null(&self) -> bool {
        self.0 == 0
    }
}

impl From<u32> for GuestPtr {
    fn from(addr: u32) -> Self {
        Self(addr)
    }
}

impl From<GuestPtr> for u32 {
    fn from(ptr: GuestPtr) -> Self {
        ptr.0
    }
}

/// Guest slice (pointer + length)
#[derive(Debug, Clone, Copy)]
pub struct GuestSlice {
    pub ptr: GuestPtr,
    pub len: u32,
}

impl GuestSlice {
    pub fn new(ptr: GuestPtr, len: u32) -> Self {
        Self { ptr, len }
    }

    pub fn from_raw(ptr: u32, len: u32) -> Self {
        Self {
            ptr: GuestPtr(ptr),
            len,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn end(&self) -> GuestPtr {
        self.ptr.offset(self.len)
    }
}

/// Memory region descriptor
#[derive(Debug, Clone)]
pub struct MemoryRegion {
    /// Start address
    pub start: GuestPtr,
    /// Size in bytes
    pub size: u32,
    /// Is allocated
    pub allocated: bool,
    /// Optional tag for debugging
    pub tag: Option<String>,
}

impl MemoryRegion {
    pub fn new(start: GuestPtr, size: u32) -> Self {
        Self {
            start,
            size,
            allocated: true,
            tag: None,
        }
    }

    pub fn with_tag(mut self, tag: &str) -> Self {
        self.tag = Some(tag.to_string());
        self
    }

    pub fn contains(&self, addr: GuestPtr) -> bool {
        // Use subtraction instead of addition to avoid u32 overflow when the
        // region is positioned near the top of the 32-bit address space.
        addr.0 >= self.start.0 && addr.0 - self.start.0 < self.size
    }

    pub fn end(&self) -> GuestPtr {
        self.start.offset(self.size)
    }
}

/// WASM memory abstraction
pub struct WasmMemory {
    /// Memory data (simulated for now, actual implementation uses wasmtime Memory)
    data: Vec<u8>,
    /// Current size in pages (64KB each)
    pages: u32,
    /// Maximum pages
    max_pages: Option<u32>,
    /// Allocated regions tracking
    regions: Vec<MemoryRegion>,
    /// Next allocation address
    heap_base: u32,
}

impl WasmMemory {
    const PAGE_SIZE: u32 = 65536; // 64KB

    pub fn new(initial_pages: u32, max_pages: Option<u32>) -> Self {
        let size = initial_pages as usize * Self::PAGE_SIZE as usize;
        Self {
            data: vec![0u8; size],
            pages: initial_pages,
            max_pages,
            regions: Vec::new(),
            heap_base: Self::PAGE_SIZE, // Reserve first page for stack/globals
        }
    }

    /// Get current size in bytes
    pub fn size(&self) -> u32 {
        // saturating_mul prevents u32 overflow if pages ever approaches u32::MAX.
        self.pages.saturating_mul(Self::PAGE_SIZE)
    }

    /// Get current size in pages
    pub fn pages(&self) -> u32 {
        self.pages
    }

    /// Grow memory by delta pages
    pub fn grow(&mut self, delta: u32) -> WasmResult<u32> {
        let new_pages =
            self.pages
                .checked_add(delta)
                .ok_or_else(|| WasmError::AllocationFailed {
                    size: delta * Self::PAGE_SIZE,
                })?;

        if let Some(max) = self.max_pages
            && new_pages > max
        {
            return Err(WasmError::ResourceLimitExceeded(format!(
                "Memory growth would exceed max pages: {} > {}",
                new_pages, max
            )));
        }

        let old_pages = self.pages;
        let new_size = new_pages as usize * Self::PAGE_SIZE as usize;
        self.data.resize(new_size, 0);
        self.pages = new_pages;

        debug!("Memory grown from {} to {} pages", old_pages, new_pages);
        Ok(old_pages)
    }

    /// Read bytes from memory
    pub fn read(&self, ptr: GuestPtr, len: u32) -> WasmResult<&[u8]> {
        let start = ptr.as_usize();
        let end = start
            .checked_add(len as usize)
            .ok_or(WasmError::MemoryOutOfBounds {
                offset: ptr.0,
                size: len,
            })?;

        if end > self.data.len() {
            return Err(WasmError::MemoryOutOfBounds {
                offset: ptr.0,
                size: len,
            });
        }

        Ok(&self.data[start..end])
    }

    /// Read bytes as mutable
    pub fn read_mut(&mut self, ptr: GuestPtr, len: u32) -> WasmResult<&mut [u8]> {
        let start = ptr.as_usize();
        let end = start
            .checked_add(len as usize)
            .ok_or(WasmError::MemoryOutOfBounds {
                offset: ptr.0,
                size: len,
            })?;

        if end > self.data.len() {
            return Err(WasmError::MemoryOutOfBounds {
                offset: ptr.0,
                size: len,
            });
        }

        Ok(&mut self.data[start..end])
    }

    /// Write bytes to memory
    pub fn write(&mut self, ptr: GuestPtr, data: &[u8]) -> WasmResult<()> {
        let start = ptr.as_usize();
        let end = start
            .checked_add(data.len())
            .ok_or(WasmError::MemoryOutOfBounds {
                offset: ptr.0,
                size: data.len() as u32,
            })?;

        if end > self.data.len() {
            return Err(WasmError::MemoryOutOfBounds {
                offset: ptr.0,
                size: data.len() as u32,
            });
        }

        self.data[start..end].copy_from_slice(data);
        Ok(())
    }

    /// Read a string from memory (null-terminated or with length)
    pub fn read_string(&self, ptr: GuestPtr, len: u32) -> WasmResult<String> {
        let bytes = self.read(ptr, len)?;
        String::from_utf8(bytes.to_vec())
            .map_err(|e| WasmError::SerializationError(format!("Invalid UTF-8: {}", e)))
    }

    /// Write a string to memory
    pub fn write_string(&mut self, ptr: GuestPtr, s: &str) -> WasmResult<()> {
        self.write(ptr, s.as_bytes())
    }

    /// Read a value of type T
    pub fn read_value<T: Copy>(&self, ptr: GuestPtr) -> WasmResult<T> {
        let size = std::mem::size_of::<T>() as u32;
        let bytes = self.read(ptr, size)?;

        // Safety: We know the bytes are properly aligned and sized
        Ok(unsafe { std::ptr::read_unaligned(bytes.as_ptr() as *const T) })
    }

    /// Write a value of type T
    pub fn write_value<T: Copy>(&mut self, ptr: GuestPtr, value: T) -> WasmResult<()> {
        let size = std::mem::size_of::<T>();
        let bytes = unsafe { std::slice::from_raw_parts(&value as *const T as *const u8, size) };
        self.write(ptr, bytes)
    }

    /// Allocate memory from the heap
    pub fn alloc(&mut self, size: u32) -> WasmResult<GuestPtr> {
        // Align to 8 bytes
        let aligned_size = (size + 7) & !7;

        // Check if we have enough space
        let end = self
            .heap_base
            .checked_add(aligned_size)
            .ok_or(WasmError::AllocationFailed { size })?;

        if end > self.size() {
            // Try to grow memory
            let needed_pages = (end / Self::PAGE_SIZE) + 1;
            let delta = needed_pages.saturating_sub(self.pages);
            if delta > 0 {
                self.grow(delta)?;
            }
        }

        let ptr = GuestPtr(self.heap_base);
        self.heap_base = end;

        self.regions.push(MemoryRegion::new(ptr, aligned_size));
        debug!("Allocated {} bytes at {:?}", aligned_size, ptr);

        Ok(ptr)
    }

    /// Free allocated memory (basic implementation - doesn't reuse space)
    pub fn free(&mut self, ptr: GuestPtr) -> WasmResult<()> {
        if let Some(region) = self.regions.iter_mut().find(|r| r.start == ptr) {
            region.allocated = false;
            debug!("Freed {} bytes at {:?}", region.size, ptr);
            Ok(())
        } else {
            warn!("Attempted to free unallocated memory at {:?}", ptr);
            Ok(()) // Don't error on double-free
        }
    }

    /// Allocate and write data
    pub fn alloc_bytes(&mut self, data: &[u8]) -> WasmResult<GuestSlice> {
        let size = u32::try_from(data.len())
            .map_err(|_| WasmError::AllocationFailed { size: u32::MAX })?;
        let ptr = self.alloc(size)?;
        self.write(ptr, data)?;
        Ok(GuestSlice::new(ptr, size))
    }

    /// Allocate and write string
    pub fn alloc_string(&mut self, s: &str) -> WasmResult<GuestSlice> {
        self.alloc_bytes(s.as_bytes())
    }
}

impl Default for WasmMemory {
    fn default() -> Self {
        Self::new(1, Some(256))
    }
}

/// Shared memory buffer for inter-module communication
pub struct SharedMemoryBuffer {
    /// Buffer data
    data: Arc<RwLock<Vec<u8>>>,
    /// Maximum size
    max_size: usize,
}

impl SharedMemoryBuffer {
    pub fn new(max_size: usize) -> Self {
        Self {
            data: Arc::new(RwLock::new(Vec::with_capacity(max_size))),
            max_size,
        }
    }

    pub async fn write(&self, data: &[u8]) -> WasmResult<()> {
        if data.len() > self.max_size {
            return Err(WasmError::ResourceLimitExceeded(format!(
                "Data size {} exceeds buffer max {}",
                data.len(),
                self.max_size
            )));
        }

        let mut buf = self.data.write().await;
        buf.clear();
        buf.extend_from_slice(data);
        Ok(())
    }

    pub async fn read(&self) -> Vec<u8> {
        self.data.read().await.clone()
    }

    pub async fn clear(&self) {
        self.data.write().await.clear();
    }

    pub async fn len(&self) -> usize {
        self.data.read().await.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.data.read().await.is_empty()
    }
}

/// Memory allocator for managing guest memory
pub struct MemoryAllocator {
    /// Free list of blocks
    free_blocks: Vec<MemoryRegion>,
    /// Minimum allocation size
    min_block_size: u32,
    /// Total allocated bytes
    allocated_bytes: u64,
    /// Peak allocated bytes
    peak_bytes: u64,
}

impl MemoryAllocator {
    pub fn new(min_block_size: u32) -> Self {
        Self {
            free_blocks: Vec::new(),
            min_block_size,
            allocated_bytes: 0,
            peak_bytes: 0,
        }
    }

    /// Find a suitable free block or return None
    pub fn find_free_block(&mut self, size: u32) -> Option<GuestPtr> {
        let aligned_size = self.align_size(size);

        // First-fit algorithm
        for (i, block) in self.free_blocks.iter().enumerate() {
            if block.size >= aligned_size {
                let ptr = block.start;
                let remaining = block.size - aligned_size;

                if remaining >= self.min_block_size {
                    // Split the block
                    self.free_blocks[i] = MemoryRegion::new(ptr.offset(aligned_size), remaining);
                } else {
                    // Use entire block
                    self.free_blocks.remove(i);
                }

                self.allocated_bytes += aligned_size as u64;
                self.peak_bytes = self.peak_bytes.max(self.allocated_bytes);

                return Some(ptr);
            }
        }

        None
    }

    /// Return a block to the free list
    pub fn return_block(&mut self, region: MemoryRegion) {
        self.allocated_bytes = self.allocated_bytes.saturating_sub(region.size as u64);

        // Try to coalesce with adjacent blocks
        let mut coalesced = region;
        let mut i = 0;

        while i < self.free_blocks.len() {
            let block = &self.free_blocks[i];

            // Check if blocks are adjacent
            if block.end() == coalesced.start {
                // Block is immediately before; saturating_add avoids overflow
                // when coalescing regions that span the end of the address space.
                coalesced = MemoryRegion::new(block.start, block.size.saturating_add(coalesced.size));
                self.free_blocks.remove(i);
            } else if coalesced.end() == block.start {
                // Block is immediately after
                coalesced = MemoryRegion::new(coalesced.start, coalesced.size.saturating_add(block.size));
                self.free_blocks.remove(i);
            } else {
                i += 1;
            }
        }

        self.free_blocks.push(coalesced);
    }

    /// Add initial memory region
    pub fn add_region(&mut self, region: MemoryRegion) {
        self.free_blocks.push(region);
    }

    fn align_size(&self, size: u32) -> u32 {
        (size + 7) & !7
    }

    pub fn allocated_bytes(&self) -> u64 {
        self.allocated_bytes
    }

    pub fn peak_bytes(&self) -> u64 {
        self.peak_bytes
    }
}

impl Default for MemoryAllocator {
    fn default() -> Self {
        Self::new(16)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_guest_ptr() {
        let ptr = GuestPtr::new(100);
        assert_eq!(ptr.0, 100);

        let offset_ptr = ptr.offset(50);
        assert_eq!(offset_ptr.0, 150);

        assert!(!ptr.is_null());
        assert!(GuestPtr::new(0).is_null());
    }

    #[test]
    fn test_wasm_memory_read_write() {
        let mut mem = WasmMemory::new(1, Some(16));

        // Write some data
        let data = b"Hello, WASM!";
        let ptr = GuestPtr::new(1024);
        mem.write(ptr, data).unwrap();

        // Read it back
        let read = mem.read(ptr, data.len() as u32).unwrap();
        assert_eq!(read, data);
    }

    #[test]
    fn test_wasm_memory_string() {
        let mut mem = WasmMemory::new(1, Some(16));

        let ptr = GuestPtr::new(1024);
        let s = "Hello, 世界!";
        mem.write_string(ptr, s).unwrap();

        let read = mem.read_string(ptr, s.len() as u32).unwrap();
        assert_eq!(read, s);
    }

    #[test]
    fn test_wasm_memory_alloc() {
        let mut mem = WasmMemory::new(1, Some(16));

        let ptr1 = mem.alloc(100).unwrap();
        let ptr2 = mem.alloc(200).unwrap();

        assert_ne!(ptr1.0, ptr2.0);
        assert!(ptr2.0 > ptr1.0);
    }

    #[test]
    fn test_wasm_memory_grow() {
        let mut mem = WasmMemory::new(1, Some(4));
        assert_eq!(mem.pages(), 1);

        let old = mem.grow(2).unwrap();
        assert_eq!(old, 1);
        assert_eq!(mem.pages(), 3);
    }

    #[test]
    fn test_memory_bounds() {
        let mem = WasmMemory::new(1, Some(1));

        // Reading beyond memory should fail
        let result = mem.read(GuestPtr::new(65536), 100);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_shared_buffer() {
        let buf = SharedMemoryBuffer::new(1024);

        buf.write(b"test data").await.unwrap();
        assert_eq!(buf.read().await, b"test data");

        buf.clear().await;
        assert!(buf.is_empty().await);
    }

    #[test]
    fn contains_does_not_overflow_near_u32_max() {
        // Regions near the top of the u32 address space previously caused
        // `start + size` to wrap in release mode, making low-address pointers
        // appear to be inside the region.
        let region = MemoryRegion::new(GuestPtr::new(u32::MAX - 10), 11);

        // Addresses that are genuinely inside the region.
        assert!(region.contains(GuestPtr::new(u32::MAX - 10))); // start
        assert!(region.contains(GuestPtr::new(u32::MAX - 5))); // mid-range
        assert!(region.contains(GuestPtr::new(u32::MAX))); // last byte

        // Low addresses must NOT be falsely matched after a wrap-around.
        assert!(!region.contains(GuestPtr::new(0)));
        assert!(!region.contains(GuestPtr::new(5)));
        // One byte before the start also must not match.
        assert!(!region.contains(GuestPtr::new(u32::MAX - 11)));
    }

    #[test]
    fn wasm_memory_size_saturates_instead_of_overflowing() {
        // `pages * PAGE_SIZE` would wrap to 0 with plain `*` if pages == 65536.
        // With saturating_mul it must return u32::MAX instead of 0.
        let mut mem = WasmMemory::new(0, None);
        mem.pages = u32::MAX;
        assert_eq!(mem.size(), u32::MAX);
    }

    #[test]
    fn return_block_coalescing_saturates_instead_of_overflowing() {
        let mut allocator = MemoryAllocator::new(16);

        // Two adjacent regions whose combined size would overflow u32.
        allocator.free_blocks.push(MemoryRegion {
            start: GuestPtr::new(0),
            size: u32::MAX - 10,
            allocated: false,
            tag: None,
        });
        let second = MemoryRegion {
            start: GuestPtr::new(u32::MAX - 10),
            size: 11,
            allocated: false,
            tag: None,
        };

        // Returning the second block triggers coalescing.  Must not panic or
        // produce a wrapped-around (smaller) size.
        allocator.return_block(second);
        assert_eq!(allocator.free_blocks.len(), 1);
        assert_eq!(allocator.free_blocks[0].size, u32::MAX);
    }
}
