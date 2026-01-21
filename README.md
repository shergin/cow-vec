# CowVec

A vector-like container optimized for efficient cloning with copy-on-write semantics.

## Motivation

In many algorithms, we need to clone a vector and then make small modifications to the clone. With a standard `Vec`, cloning is O(n) - every element must be copied. For large vectors with frequent cloning, this becomes a performance bottleneck.

`CowVec` solves this by:
1. Storing all values in a shared arena (via `Arc`)
2. Each instance maintains only a vector of pointers into the arena
3. Cloning copies only the pointer vector, not the actual data

This makes cloning O(n) in pointer copies rather than O(n) in element copies - significantly faster for large or complex types.

## Use Cases

- **Backtracking algorithms**: Clone state, explore a branch, discard or keep
- **Undo/redo systems**: Maintain history of states efficiently
- **Parallel exploration**: Share base state across threads, each making local modifications
- **Immutable data structures**: Build persistent vectors with structural sharing

## API Overview

```rust
use cow_vec::CowVec;

// Create from Vec
let mut vec = CowVec::from(vec![1, 2, 3, 4, 5]);

// Clone is cheap - shares the arena
let mut clone = vec.clone();

// Modifications are independent (copy-on-write)
clone.set(0, 100);
assert_eq!(vec[0], 1);      // Original unchanged
assert_eq!(clone[0], 100);  // Clone has new value

// Standard vector operations
vec.push(6);
vec.pop();
vec.reverse();
let v: Vec<i32> = vec.to_vec();
```

## Limitations

### No Element Removal from Arena

The arena is append-only. When you call `pop()`, `remove()`, `clear()`, or `truncate()`, the values remain allocated in the arena - only the pointers are removed from this instance's view.

```rust,ignore
let mut vec = CowVec::from(vec![1, 2, 3]);
vec.pop();  // Value 3 still exists in arena, just not accessible via `vec`
```

**Implication**: If you repeatedly push and pop elements, memory usage grows. This design is optimized for scenarios where you build up data and clone frequently, not for long-lived mutable collections.

**Mitigation**: Use `clone_with_max_capacity(n)` to compact the arena when cloning:

```rust,ignore
let mut vec = CowVec::from(vec![1, 2, 3]);

// Many operations accumulate garbage in the arena
for i in 0..100 {
    vec.set(0, i);
}
// Arena now has 103 allocations, but only 3 are live

// Clone with compaction if arena exceeds 10 allocations
let compacted = vec.clone_with_max_capacity(10);
// compacted has a fresh arena with only 3 allocations
```

### No Mutable Access

You cannot get `&mut T` references to elements. The `set()` method allocates a new value in the arena rather than mutating in place.

```rust,ignore
// This won't compile:
// let x: &mut i32 = vec.get_mut(0);

// Instead, use set():
vec.set(0, new_value);  // Allocates new value, updates pointer
```

### Clone Requires T: Clone for set()

The `set()` method requires `T: Clone` because it allocates a new copy in the arena.

### Memory Overhead

Each `CowVec` instance stores:
- `Arc<CowArena<T>>` (pointer + reference count)
- `Vec<*const T>` (pointer per element)

For very small types (e.g., `u8`), the pointer overhead may exceed the element size. Consider using standard `Vec` for small, cheap-to-copy types.

## Implementation Details

### Arena Storage

Values are stored in a `typed_arena::Arena<T>` wrapped in `Mutex` for thread-safe allocation:

```rust,ignore
struct CowArena<T> {
    arena: Mutex<Arena<T>>,
}
```

The arena guarantees that allocated values are never moved or deallocated until the arena itself is dropped. This allows us to store raw pointers safely.

### Pointer Storage

Each `CowVec` instance maintains a vector of raw pointers:

```rust,ignore
pub struct CowVec<T> {
    arena: Arc<CowArena<T>>,
    items: Vec<*const T>,
}
```

### Safety Invariants

The use of raw pointers is safe because:

1. **Stable addresses**: `typed_arena` guarantees values are never moved once allocated
2. **Lifetime guarantee**: Values are only dropped when the `Arena` drops, which only happens when all `Arc` references are gone
3. **No dangling pointers**: As long as a `CowVec` exists, it holds an `Arc` to the arena, keeping all values alive

### Thread Safety

`CowVec<T>` implements `Send` and `Sync` when `T: Send + Sync`:

- Arena mutations (push) are protected by `Mutex`
- Reading via pointers requires no synchronization (immutable access)
- Each thread's `CowVec` instance has its own pointer vector

### Copy-on-Write

The `set()` method implements copy-on-write:

```rust,ignore
pub fn set(&mut self, index: usize, value: T) {
    let ptr = self.arena.alloc(value);  // Allocate new value
    self.items[index] = ptr;             // Update only this instance's pointer
}
```

Other clones continue pointing to the original value.

## Performance Characteristics

| Operation | Time Complexity | Notes |
|-----------|-----------------|-------|
| `new()` | O(1) | |
| `clone()` | O(n) | Pointer memcpy only - extremely fast |
| `clone_with_max_capacity()` | O(n) | Pointer memcpy if under limit, element clones if over |
| `push()` | O(1) amortized | Arena allocation + vec push |
| `get()` | O(1) | Pointer dereference |
| `set()` | O(1) | Arena allocation + pointer update |
| `pop()` | O(1) | |
| `remove()` | O(n) | Pointer memcpy to shift elements |
| `reverse()` | O(n) | In-place pointer swap |
| `iter()` | O(1) | Iterator creation |

Note: All O(n) operations work on the pointer vector (8 bytes per element), not on the actual data.

**Example**: A vector of 42 million objects, where each element contains nested `Vec`s, `HashMap`s, and `String`s:
- **Regular `Vec` clone**: Runs 42M `Clone::clone()` calls, each allocating memory, copying nested structures, updating reference counts, and potentially triggering the allocator
- **`CowVec` clone**: A single `memcpy` of 42M × 8 = **~320 MB** of pointers - no allocations, no `Clone` trait calls, no nested structure traversal

These bulk copies are hardware-optimized on modern CPUs:
- **x86 (Ivy Bridge+)**: Uses ERMSB (Enhanced REP MOVSB) with automatic vectorization
- **ARM64**: Uses optimized LDP/STP (load/store pair) sequences copying 16+ bytes per cycle

At ~50 GB/s memory bandwidth, cloning 320 MB of pointers takes ~6ms - regardless of how complex the elements are.

## Related Work: Persistent Data Structures

`CowVec` implements a form of **persistent data structure** with **structural sharing** - a well-known pattern in functional programming where data structures preserve previous versions when modified.

### Terminology

- **Persistent Data Structure**: A data structure that always preserves the previous version of itself when modified.
- **Structural Sharing**: When copies share most of their memory, only allocating new memory for changed parts.
- **Copy-on-Write (COW)**: The lazy copying strategy where clone is cheap and actual copying happens on modification.

### Existing Rust Crates

Several mature crates implement persistent vectors with more sophisticated algorithms:

| Crate | Description | Implementation |
|-------|-------------|----------------|
| `im` / `im-rc` | Most popular, feature-complete | RRB trees (relaxed radix balanced) |
| `imbl` | Maintained fork of `im` | Same as `im` |
| `rpds` | Persistent data structures, `no_std` support | Bitmapped vector trie |
| `shared_vector` | Simpler, reference-counted | Atomic ref-counting |

### How CowVec Differs

`CowVec` is a **simpler, specialized variant** optimized for low-latency access and cheap cloning of large elements:

| Aspect | CowVec | im/rpds |
|--------|--------|---------|
| Clone | O(n) pointer copies (trivially cheap) | O(1) or O(log n) |
| Random access | O(1) direct lookup | O(log n) tree traversal |
| Cache locality | Excellent (contiguous pointer array) | Poor (scattered tree nodes) |
| Access latency | Single pointer dereference | Multiple pointer chases |
| Allocations per insert | 1 arena bump (very fast) | O(log n) tree nodes |
| Modification | Arena allocation | Tree node allocation |
| Memory reclaim | Manual via `clone_with_max_capacity` | Automatic via ref-counting |
| Best for | Frequent clones, few modifications | Many modifications |

**Key trade-offs:**

- `CowVec` clone is **O(n) but trivially cheap** - it copies only pointers (8 bytes each), not element data. For a 1000-element vector, clone copies ~8KB of pointers regardless of element size.
- `CowVec` has **O(1) random access** (direct pointer lookup) vs O(log n) for tree-based structures.
- `CowVec` uses **arena allocation** (bump pointer, no syscalls) vs tree-based structures that allocate O(log n) nodes per modification through the standard allocator.
- `CowVec` **does not reclaim memory** automatically; use `clone_with_max_capacity()` for compaction.
- `CowVec` is **simpler** with less overhead for small-to-medium sized vectors.

**Cache locality and low-latency access:**

- The pointer vector is **contiguous in memory**, enabling efficient CPU cache prefetching during iteration.
- Random access is a **single pointer dereference** with no tree traversal, ideal for latency-sensitive code.
- Tree-based structures like `im` require following multiple pointers through tree nodes, causing cache misses.
- For hot loops accessing elements by index, `CowVec` provides predictable, low-latency performance.

**Choose `CowVec` when:**
- You clone frequently but modify sparingly.
- You need O(1) indexed access with minimal latency.
- Cache-friendly iteration performance matters.
- Vectors are short-lived or periodically compacted.
- Simplicity matters more than optimal clone performance.

**Choose `im`/`rpds` when:**
- You need O(1) clone operations.
- You perform many modifications across many clones.
- Automatic memory reclamation is important.
- You're building long-lived persistent data structures.

## When to Use CowVec

**Good fit:**
- Large elements (structs, strings, nested containers)
- Frequent cloning with few modifications per clone
- Algorithms that explore multiple branches from a common state
- Short-lived instances where arena memory growth is acceptable

**Poor fit:**
- Small, cheap-to-copy types (use `Vec`)
- Long-lived mutable collections with many add/remove cycles
- Need for mutable element access (`&mut T`)
- Memory-constrained environments where arena growth is problematic
