use std::sync::Arc;
use std::thread;

use crate::CowVec;

#[test]
fn test_new_creates_empty_vec() {
    let vec: CowVec<i32> = CowVec::new();
    assert!(vec.is_empty());
    assert_eq!(vec.len(), 0);
}

#[test]
fn test_with_capacity() {
    let vec: CowVec<i32> = CowVec::with_capacity(100);
    assert!(vec.is_empty());
    assert_eq!(vec.len(), 0);
}

#[test]
fn test_push_and_get() {
    let mut vec = CowVec::new();
    vec.push(1);
    vec.push(2);
    vec.push(3);

    assert_eq!(vec.len(), 3);
    assert_eq!(vec.get(0), Some(&1));
    assert_eq!(vec.get(1), Some(&2));
    assert_eq!(vec.get(2), Some(&3));
    assert_eq!(vec.get(3), None);
}

#[test]
fn test_index_operator() {
    let vec = CowVec::from(vec![10, 20, 30]);
    assert_eq!(vec[0], 10);
    assert_eq!(vec[1], 20);
    assert_eq!(vec[2], 30);
}

#[test]
#[should_panic(expected = "index out of bounds")]
fn test_index_out_of_bounds() {
    let vec = CowVec::from(vec![1, 2, 3]);
    let _ = vec[3];
}

#[test]
fn test_from_vec() {
    let vec = CowVec::from(vec!["a", "b", "c"]);
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0], "a");
    assert_eq!(vec[1], "b");
    assert_eq!(vec[2], "c");
}

#[test]
fn test_clone_shares_arena() {
    let vec1 = CowVec::from(vec![1, 2, 3]);
    let vec2 = vec1.clone();

    // Both should have the same values.
    assert_eq!(vec1.len(), vec2.len());
    assert_eq!(vec1[0], vec2[0]);
    assert_eq!(vec1[1], vec2[1]);
    assert_eq!(vec1[2], vec2[2]);
}

#[test]
fn test_set_copy_on_write() {
    let vec1 = CowVec::from(vec![1, 2, 3]);
    let mut vec2 = vec1.clone();

    // Modify vec2.
    vec2.set(0, 100);

    // vec1 should be unchanged.
    assert_eq!(vec1[0], 1);
    // vec2 should have the new value.
    assert_eq!(vec2[0], 100);
}

#[test]
#[should_panic(expected = "index out of bounds")]
fn test_set_out_of_bounds() {
    let mut vec = CowVec::from(vec![1, 2, 3]);
    vec.set(3, 100);
}

#[test]
fn test_iterator() {
    let vec = CowVec::from(vec![1, 2, 3, 4, 5]);
    let collected: Vec<&i32> = vec.iter().collect();
    assert_eq!(collected, vec![&1, &2, &3, &4, &5]);
}

#[test]
fn test_iterator_size_hint() {
    let vec = CowVec::from(vec![1, 2, 3]);
    let mut iter = vec.iter();
    assert_eq!(iter.size_hint(), (3, Some(3)));
    iter.next();
    assert_eq!(iter.size_hint(), (2, Some(2)));
    iter.next();
    assert_eq!(iter.size_hint(), (1, Some(1)));
    iter.next();
    assert_eq!(iter.size_hint(), (0, Some(0)));
}

#[test]
fn test_into_iterator() {
    let vec = CowVec::from(vec![1, 2, 3]);
    let mut sum = 0;
    for &item in &vec {
        sum += item;
    }
    assert_eq!(sum, 6);
}

#[test]
fn test_default() {
    let vec: CowVec<i32> = CowVec::default();
    assert!(vec.is_empty());
}

#[test]
fn test_with_complex_type() {
    #[derive(Clone, Debug, PartialEq)]
    struct Item {
        id: i32,
        name: String,
    }

    let mut vec = CowVec::new();
    vec.push(Item {
        id: 1,
        name: "first".to_string(),
    });
    vec.push(Item {
        id: 2,
        name: "second".to_string(),
    });

    assert_eq!(vec[0].id, 1);
    assert_eq!(vec[1].name, "second");

    let mut vec2 = vec.clone();
    vec2.set(
        0,
        Item {
            id: 100,
            name: "modified".to_string(),
        },
    );

    assert_eq!(vec[0].id, 1);
    assert_eq!(vec2[0].id, 100);
}

#[test]
fn test_thread_safety() {
    let vec = CowVec::from(vec![1, 2, 3, 4, 5]);
    let vec_arc = Arc::new(vec);

    let handles: Vec<_> = (0..4)
        .map(|i| {
            let vec_clone = Arc::clone(&vec_arc);
            thread::spawn(move || {
                let sum: i32 = vec_clone.iter().sum();
                assert_eq!(sum, 15);
                vec_clone[i % 5]
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }
}

#[test]
fn test_push_after_clone() {
    let vec1 = CowVec::from(vec![1, 2, 3]);
    let mut vec2 = vec1.clone();

    vec2.push(4);

    assert_eq!(vec1.len(), 3);
    assert_eq!(vec2.len(), 4);
    assert_eq!(vec2[3], 4);
}

#[test]
fn test_multiple_clones() {
    let original = CowVec::from(vec![1, 2, 3]);
    let clone1 = original.clone();
    let clone2 = original.clone();
    let mut clone3 = clone1.clone();

    clone3.set(0, 100);

    assert_eq!(original[0], 1);
    assert_eq!(clone1[0], 1);
    assert_eq!(clone2[0], 1);
    assert_eq!(clone3[0], 100);
}

#[test]
fn test_empty_iterator() {
    let vec: CowVec<i32> = CowVec::new();
    let collected: Vec<&i32> = vec.iter().collect();
    assert!(collected.is_empty());
}

#[test]
fn test_first_and_last() {
    let vec = CowVec::from(vec![1, 2, 3]);
    assert_eq!(vec.first(), Some(&1));
    assert_eq!(vec.last(), Some(&3));

    let empty: CowVec<i32> = CowVec::new();
    assert_eq!(empty.first(), None);
    assert_eq!(empty.last(), None);

    let single = CowVec::from(vec![42]);
    assert_eq!(single.first(), Some(&42));
    assert_eq!(single.last(), Some(&42));
}

#[test]
fn test_pop() {
    let mut vec = CowVec::from(vec![1, 2, 3]);
    assert_eq!(vec.pop(), Some(3));
    assert_eq!(vec.len(), 2);
    assert_eq!(vec.pop(), Some(2));
    assert_eq!(vec.pop(), Some(1));
    assert_eq!(vec.pop(), None);
    assert!(vec.is_empty());
}

#[test]
fn test_pop_does_not_affect_clones() {
    let vec1 = CowVec::from(vec![1, 2, 3]);
    let mut vec2 = vec1.clone();

    vec2.pop();
    assert_eq!(vec1.len(), 3);
    assert_eq!(vec2.len(), 2);
}

#[test]
fn test_remove() {
    let mut vec = CowVec::from(vec![1, 2, 3, 4, 5]);
    assert_eq!(vec.remove(2), 3);
    assert_eq!(vec.len(), 4);
    assert_eq!(vec[0], 1);
    assert_eq!(vec[1], 2);
    assert_eq!(vec[2], 4);
    assert_eq!(vec[3], 5);
}

#[test]
fn test_remove_first_and_last() {
    let mut vec = CowVec::from(vec![1, 2, 3]);
    assert_eq!(vec.remove(0), 1);
    assert_eq!(vec[0], 2);

    let mut vec = CowVec::from(vec![1, 2, 3]);
    assert_eq!(vec.remove(2), 3);
    assert_eq!(vec.len(), 2);
}

#[test]
#[should_panic]
fn test_remove_out_of_bounds() {
    let mut vec = CowVec::from(vec![1, 2, 3]);
    vec.remove(3);
}

#[test]
fn test_swap() {
    let mut vec = CowVec::from(vec![1, 2, 3, 4, 5]);
    vec.swap(0, 4);
    assert_eq!(vec[0], 5);
    assert_eq!(vec[4], 1);

    vec.swap(1, 1);
    assert_eq!(vec[1], 2);
}

#[test]
fn test_reverse() {
    let mut vec = CowVec::from(vec![1, 2, 3, 4, 5]);
    vec.reverse();
    assert_eq!(vec[0], 5);
    assert_eq!(vec[1], 4);
    assert_eq!(vec[2], 3);
    assert_eq!(vec[3], 2);
    assert_eq!(vec[4], 1);

    let mut empty: CowVec<i32> = CowVec::new();
    empty.reverse();
    assert!(empty.is_empty());
}

#[test]
fn test_truncate() {
    let mut vec = CowVec::from(vec![1, 2, 3, 4, 5]);
    vec.truncate(3);
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[2], 3);

    vec.truncate(10);
    assert_eq!(vec.len(), 3);

    vec.truncate(0);
    assert!(vec.is_empty());
}

#[test]
fn test_clear() {
    let mut vec = CowVec::from(vec![1, 2, 3]);
    vec.clear();
    assert!(vec.is_empty());
    assert_eq!(vec.len(), 0);
}

#[test]
fn shared_truncate_clear_retain_keep_working_afterwards() {
    // The shared paths build a fresh, private table; make sure it is a
    // fully functional one.
    let base = CowVec::from((0..10).collect::<Vec<i32>>());

    let mut t = base.clone();
    t.truncate(4);
    t.push(100);
    assert_eq!(t.to_vec(), vec![0, 1, 2, 3, 100]);
    assert!(!t.is_structure_shared());

    let mut c = base.clone();
    c.clear();
    c.push(7);
    assert_eq!(c.to_vec(), vec![7]);

    let mut r = base.clone();
    r.retain(|x| x % 3 == 0);
    r.push(-1);
    assert_eq!(r.to_vec(), vec![0, 3, 6, 9, -1]);

    // Truncating to the current length or beyond does not even diverge.
    let mut n = base.clone();
    n.truncate(10);
    n.truncate(50);
    assert!(n.is_structure_shared());

    assert_eq!(base.to_vec(), (0..10).collect::<Vec<i32>>());
}

#[test]
fn test_clear_does_not_affect_clones() {
    let vec1 = CowVec::from(vec![1, 2, 3]);
    let mut vec2 = vec1.clone();

    vec2.clear();
    assert_eq!(vec1.len(), 3);
    assert!(vec2.is_empty());
}

#[test]
fn test_extend() {
    let mut vec = CowVec::from(vec![1, 2]);
    vec.extend(vec![3, 4, 5]);
    assert_eq!(vec.len(), 5);
    assert_eq!(vec[2], 3);
    assert_eq!(vec[3], 4);
    assert_eq!(vec[4], 5);
}

#[test]
fn test_extend_empty() {
    let mut vec: CowVec<i32> = CowVec::new();
    vec.extend(vec![1, 2, 3]);
    assert_eq!(vec.len(), 3);
}

#[test]
fn test_position_via_iterator() {
    let vec = CowVec::from(vec![1, 2, 3, 4, 5]);
    assert_eq!(vec.iter().position(|&x| x == 3), Some(2));
    assert_eq!(vec.iter().position(|&x| x == 10), None);
    assert_eq!(vec.iter().position(|&x| x > 3), Some(3));
}

#[test]
fn test_contains() {
    let vec = CowVec::from(vec![1, 2, 3, 4, 5]);
    assert!(vec.contains(&3));
    assert!(!vec.contains(&10));
}

#[test]
fn test_contains_with_strings() {
    let vec = CowVec::from(vec!["hello", "world"]);
    assert!(vec.contains(&"hello"));
    assert!(!vec.contains(&"foo"));
}

#[test]
fn test_to_vec() {
    let cow_vec = CowVec::from(vec![1, 2, 3]);
    let regular_vec = cow_vec.to_vec();
    assert_eq!(regular_vec, vec![1, 2, 3]);
}

#[test]
fn test_to_vec_empty() {
    let cow_vec: CowVec<i32> = CowVec::new();
    let regular_vec = cow_vec.to_vec();
    assert!(regular_vec.is_empty());
}

#[test]
fn test_operations_chain() {
    let mut vec = CowVec::from(vec![5, 3, 1, 4, 2]);
    vec.reverse();
    vec.pop();
    vec.push(10);
    vec.swap(0, 1);

    assert_eq!(vec.to_vec(), vec![4, 2, 1, 3, 10]);
}

#[test]
fn test_clone_compacted_shares_arena_when_under_limit() {
    let vec1 = CowVec::from(vec![1, 2, 3]);
    let vec2 = vec1.clone_compacted(10);

    // Both should have the same values.
    assert_eq!(vec1.to_vec(), vec2.to_vec());

    // They should share the same arena (Arc points to same allocation).
    // We can verify this indirectly: modifications to vec2 via set should
    // NOT affect vec1 (copy-on-write), but they share the base arena.
    let mut vec3 = vec2.clone();
    vec3.set(0, 100);
    assert_eq!(vec1[0], 1);
    assert_eq!(vec2[0], 1);
    assert_eq!(vec3[0], 100);
}

#[test]
fn test_clone_compacted_creates_new_arena_when_over_limit() {
    let mut vec1 = CowVec::from(vec![1, 2, 3]);

    // Make many allocations to exceed the limit.
    for i in 0..10 {
        vec1.set(0, i);
    }
    // Now arena has 3 (initial) + 10 (sets) = 13 allocations.

    // Clone with max_capacity of 5 should create a new arena.
    let vec2 = vec1.clone_compacted(5);

    // Values should be the same.
    assert_eq!(vec1.to_vec(), vec2.to_vec());
    assert_eq!(vec2[0], 9);

    // The new arena should have only 3 allocations (the current elements).
    // Further sets on vec2 should not affect vec1.
    let mut vec3 = vec2.clone();
    vec3.set(0, 999);
    assert_eq!(vec1[0], 9);
    assert_eq!(vec2[0], 9);
    assert_eq!(vec3[0], 999);
}

#[test]
fn test_clone_compacted_compacts_after_pop() {
    let mut vec1 = CowVec::from(vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);

    // Pop most elements (they remain in arena as garbage).
    for _ in 0..8 {
        vec1.pop();
    }
    // Now vec1 has 2 elements but arena has 10 allocations.

    // Clone with max_capacity of 5 should create a fresh arena.
    let vec2 = vec1.clone_compacted(5);

    assert_eq!(vec2.len(), 2);
    assert_eq!(vec2.to_vec(), vec![1, 2]);
}

#[test]
fn test_clone_compacted_at_exact_limit() {
    let vec1 = CowVec::from(vec![1, 2, 3]);
    // Arena has exactly 3 allocations.

    // Clone with max_capacity of 3 should share arena (not exceed).
    let vec2 = vec1.clone_compacted(3);
    assert_eq!(vec2.to_vec(), vec![1, 2, 3]);

    // Clone with max_capacity of 2 should create new arena (exceeds).
    let vec3 = vec1.clone_compacted(2);
    assert_eq!(vec3.to_vec(), vec![1, 2, 3]);
}

#[test]
fn test_make_mut_basic() {
    let mut vec = CowVec::from(vec![1, 2, 3]);
    *vec.make_mut(0) = 100;
    assert_eq!(vec[0], 100);
    assert_eq!(vec[1], 2);
    assert_eq!(vec[2], 3);
}

#[test]
fn test_make_mut_in_place_mutation() {
    let mut vec = CowVec::from(vec![String::from("ab")]);
    vec.make_mut(0).push('c');
    assert_eq!(vec[0], "abc");
}

#[test]
fn test_make_mut_does_not_affect_clones() {
    let vec1 = CowVec::from(vec![1, 2, 3]);
    let mut vec2 = vec1.clone();

    *vec2.make_mut(0) = 100;

    // vec1 should be unchanged (copy-on-write).
    assert_eq!(vec1[0], 1);
    assert_eq!(vec2[0], 100);
}

#[test]
#[should_panic(expected = "index out of bounds")]
fn test_make_mut_out_of_bounds() {
    let mut vec = CowVec::from(vec![1, 2, 3]);
    *vec.make_mut(3) = 100;
}

#[test]
fn test_iterator_exact_size() {
    let vec = CowVec::from(vec![1, 2, 3, 4, 5]);
    let iter = vec.iter();
    assert_eq!(iter.len(), 5);

    let mut iter = vec.iter();
    iter.next();
    iter.next();
    assert_eq!(iter.len(), 3);
}

#[test]
#[should_panic]
fn test_swap_out_of_bounds() {
    let mut vec = CowVec::from(vec![1, 2, 3]);
    vec.swap(0, 5);
}

#[test]
fn test_remove_does_not_affect_clones() {
    let vec1 = CowVec::from(vec![1, 2, 3, 4, 5]);
    let mut vec2 = vec1.clone();

    vec2.remove(2);
    assert_eq!(vec1.len(), 5);
    assert_eq!(vec1[2], 3);
    assert_eq!(vec2.len(), 4);
}

#[test]
fn test_truncate_does_not_affect_clones() {
    let vec1 = CowVec::from(vec![1, 2, 3, 4, 5]);
    let mut vec2 = vec1.clone();

    vec2.truncate(2);
    assert_eq!(vec1.len(), 5);
    assert_eq!(vec2.len(), 2);
}

#[test]
fn test_reverse_does_not_affect_clones() {
    let vec1 = CowVec::from(vec![1, 2, 3]);
    let mut vec2 = vec1.clone();

    vec2.reverse();
    assert_eq!(vec1[0], 1);
    assert_eq!(vec1[2], 3);
    assert_eq!(vec2[0], 3);
    assert_eq!(vec2[2], 1);
}

#[test]
fn test_swap_does_not_affect_clones() {
    let vec1 = CowVec::from(vec![1, 2, 3]);
    let mut vec2 = vec1.clone();

    vec2.swap(0, 2);
    assert_eq!(vec1[0], 1);
    assert_eq!(vec1[2], 3);
    assert_eq!(vec2[0], 3);
    assert_eq!(vec2[2], 1);
}

#[test]
fn test_extend_does_not_affect_clones() {
    let vec1 = CowVec::from(vec![1, 2]);
    let mut vec2 = vec1.clone();

    vec2.extend(vec![3, 4, 5]);
    assert_eq!(vec1.len(), 2);
    assert_eq!(vec2.len(), 5);
}

#[test]
fn test_position_empty() {
    let vec: CowVec<i32> = CowVec::new();
    assert_eq!(vec.iter().position(|&x| x == 1), None);
}

#[test]
fn test_contains_empty() {
    let vec: CowVec<i32> = CowVec::new();
    assert!(!vec.contains(&1));
}

#[test]
fn test_as_slice_basic() {
    let vec = CowVec::from(vec![1, 2, 3, 4, 5]);
    let slice: &[&i32] = vec.as_slice();

    assert_eq!(slice.len(), 5);
    assert_eq!(*slice[0], 1);
    assert_eq!(*slice[1], 2);
    assert_eq!(*slice[2], 3);
    assert_eq!(*slice[3], 4);
    assert_eq!(*slice[4], 5);
}

#[test]
fn test_as_slice_empty() {
    let vec: CowVec<i32> = CowVec::new();
    let slice = vec.as_slice();
    assert!(slice.is_empty());
}

#[test]
fn test_as_slice_single_element() {
    let vec = CowVec::from(vec![42]);
    let slice = vec.as_slice();
    assert_eq!(slice.len(), 1);
    assert_eq!(*slice[0], 42);
}

#[test]
fn test_as_slice_after_modifications() {
    let mut vec = CowVec::from(vec![1, 2, 3]);
    vec.set(1, 20);
    vec.push(4);

    let slice = vec.as_slice();
    assert_eq!(slice.len(), 4);
    assert_eq!(*slice[0], 1);
    assert_eq!(*slice[1], 20);
    assert_eq!(*slice[2], 3);
    assert_eq!(*slice[3], 4);
}

#[test]
fn test_as_slice_clone_independence() {
    let vec1 = CowVec::from(vec![1, 2, 3]);
    let mut vec2 = vec1.clone();
    vec2.set(0, 100);

    let slice1 = vec1.as_slice();
    let slice2 = vec2.as_slice();

    // Slices should reflect their respective CowVec states.
    assert_eq!(*slice1[0], 1);
    assert_eq!(*slice2[0], 100);
}

#[test]
fn test_as_slice_with_strings() {
    let vec = CowVec::from(vec!["hello", "world", "rust"]);
    let slice = vec.as_slice();

    assert_eq!(slice.len(), 3);
    assert_eq!(*slice[0], "hello");
    assert_eq!(*slice[1], "world");
    assert_eq!(*slice[2], "rust");
}

#[test]
fn test_as_slice_can_be_iterated() {
    let vec = CowVec::from(vec![1, 2, 3, 4, 5]);
    let slice = vec.as_slice();

    let sum: i32 = slice.iter().map(|&&x| x).sum();
    assert_eq!(sum, 15);
}

#[test]
fn test_as_slice_supports_slice_methods() {
    let vec = CowVec::from(vec![5, 2, 8, 1, 9]);
    let slice = vec.as_slice();

    // Test various slice methods.
    assert_eq!(slice.first(), Some(&&5));
    assert_eq!(slice.last(), Some(&&9));
    assert!(!slice.is_empty());

    // Test slicing.
    let sub_slice = &slice[1..4];
    assert_eq!(sub_slice.len(), 3);
    assert_eq!(*sub_slice[0], 2);
    assert_eq!(*sub_slice[1], 8);
    assert_eq!(*sub_slice[2], 1);
}

#[test]
fn test_debug_basic() {
    let vec = CowVec::from(vec![1, 2, 3]);
    let debug_str = format!("{:?}", vec);
    assert_eq!(debug_str, "[1, 2, 3]");
}

#[test]
fn test_debug_empty() {
    let vec: CowVec<i32> = CowVec::new();
    let debug_str = format!("{:?}", vec);
    assert_eq!(debug_str, "[]");
}

#[test]
fn test_debug_single_element() {
    let vec = CowVec::from(vec![42]);
    let debug_str = format!("{:?}", vec);
    assert_eq!(debug_str, "[42]");
}

#[test]
fn test_debug_with_strings() {
    let vec = CowVec::from(vec!["hello", "world"]);
    let debug_str = format!("{:?}", vec);
    assert_eq!(debug_str, "[\"hello\", \"world\"]");
}

#[test]
fn test_debug_pretty_print() {
    let vec = CowVec::from(vec![1, 2, 3]);
    let debug_str = format!("{:#?}", vec);
    assert_eq!(debug_str, "[\n    1,\n    2,\n    3,\n]");
}

// ============ insert tests ============

#[test]
fn test_insert_middle() {
    let mut vec = CowVec::from(vec![1, 2, 3]);
    vec.insert(1, 10);
    assert_eq!(vec.to_vec(), vec![1, 10, 2, 3]);
}

#[test]
fn test_insert_beginning() {
    let mut vec = CowVec::from(vec![1, 2, 3]);
    vec.insert(0, 10);
    assert_eq!(vec.to_vec(), vec![10, 1, 2, 3]);
}

#[test]
fn test_insert_end() {
    let mut vec = CowVec::from(vec![1, 2, 3]);
    vec.insert(3, 10);
    assert_eq!(vec.to_vec(), vec![1, 2, 3, 10]);
}

#[test]
fn test_insert_empty() {
    let mut vec: CowVec<i32> = CowVec::new();
    vec.insert(0, 42);
    assert_eq!(vec.to_vec(), vec![42]);
}

#[test]
fn test_insert_does_not_affect_clones() {
    let vec1 = CowVec::from(vec![1, 2, 3]);
    let mut vec2 = vec1.clone();
    vec2.insert(1, 10);
    assert_eq!(vec1.to_vec(), vec![1, 2, 3]);
    assert_eq!(vec2.to_vec(), vec![1, 10, 2, 3]);
}

#[test]
#[should_panic]
fn test_insert_out_of_bounds() {
    let mut vec = CowVec::from(vec![1, 2, 3]);
    vec.insert(4, 10);
}

// ============ retain tests ============

#[test]
fn test_retain_even() {
    let mut vec = CowVec::from(vec![1, 2, 3, 4, 5, 6]);
    vec.retain(|&x| x % 2 == 0);
    assert_eq!(vec.to_vec(), vec![2, 4, 6]);
}

#[test]
fn test_retain_all() {
    let mut vec = CowVec::from(vec![1, 2, 3]);
    vec.retain(|_| true);
    assert_eq!(vec.to_vec(), vec![1, 2, 3]);
}

#[test]
fn test_retain_none() {
    let mut vec = CowVec::from(vec![1, 2, 3]);
    vec.retain(|_| false);
    assert!(vec.is_empty());
}

#[test]
fn test_retain_empty() {
    let mut vec: CowVec<i32> = CowVec::new();
    vec.retain(|_| true);
    assert!(vec.is_empty());
}

#[test]
fn test_retain_does_not_affect_clones() {
    let vec1 = CowVec::from(vec![1, 2, 3, 4, 5]);
    let mut vec2 = vec1.clone();
    vec2.retain(|&x| x > 2);
    assert_eq!(vec1.to_vec(), vec![1, 2, 3, 4, 5]);
    assert_eq!(vec2.to_vec(), vec![3, 4, 5]);
}

#[test]
fn test_retain_with_strings() {
    let mut vec = CowVec::from(vec!["apple", "banana", "cherry", "apricot"]);
    vec.retain(|s| s.starts_with('a'));
    assert_eq!(vec.to_vec(), vec!["apple", "apricot"]);
}

// ============ split_off tests ============

#[test]
fn test_split_off_middle() {
    let mut vec = CowVec::from(vec![1, 2, 3, 4, 5]);
    let tail = vec.split_off(3);
    assert_eq!(vec.to_vec(), vec![1, 2, 3]);
    assert_eq!(tail.to_vec(), vec![4, 5]);
}

#[test]
fn test_split_off_beginning() {
    let mut vec = CowVec::from(vec![1, 2, 3]);
    let tail = vec.split_off(0);
    assert!(vec.is_empty());
    assert_eq!(tail.to_vec(), vec![1, 2, 3]);
}

#[test]
fn test_split_off_end() {
    let mut vec = CowVec::from(vec![1, 2, 3]);
    let tail = vec.split_off(3);
    assert_eq!(vec.to_vec(), vec![1, 2, 3]);
    assert!(tail.is_empty());
}

#[test]
fn test_split_off_shares_arena() {
    let mut vec1 = CowVec::from(vec![1, 2, 3, 4, 5]);
    let vec2 = vec1.split_off(2);

    // Both should work independently
    assert_eq!(vec1[0], 1);
    assert_eq!(vec1[1], 2);
    assert_eq!(vec2[0], 3);
    assert_eq!(vec2[1], 4);
    assert_eq!(vec2[2], 5);
}

#[test]
fn test_split_off_does_not_affect_original_clones() {
    let original = CowVec::from(vec![1, 2, 3, 4, 5]);
    let mut to_split = original.clone();
    let tail = to_split.split_off(2);

    assert_eq!(original.to_vec(), vec![1, 2, 3, 4, 5]);
    assert_eq!(to_split.to_vec(), vec![1, 2]);
    assert_eq!(tail.to_vec(), vec![3, 4, 5]);
}

#[test]
#[should_panic]
fn test_split_off_out_of_bounds() {
    let mut vec = CowVec::from(vec![1, 2, 3]);
    vec.split_off(4);
}

// ============ splice tests ============

#[test]
fn test_splice_replace_middle() {
    let mut vec = CowVec::from(vec![1, 2, 3, 4, 5]);
    let removed: Vec<i32> = vec.splice(1..3, vec![10, 20, 30]);
    assert_eq!(removed, vec![2, 3]);
    assert_eq!(vec.to_vec(), vec![1, 10, 20, 30, 4, 5]);
}

#[test]
fn test_splice_remove_only() {
    let mut vec = CowVec::from(vec![1, 2, 3, 4, 5]);
    let removed: Vec<i32> = vec.splice(1..4, vec![]);
    assert_eq!(removed, vec![2, 3, 4]);
    assert_eq!(vec.to_vec(), vec![1, 5]);
}

#[test]
fn test_splice_insert_only() {
    let mut vec = CowVec::from(vec![1, 2, 3]);
    let removed: Vec<i32> = vec.splice(1..1, vec![10, 20]);
    assert!(removed.is_empty());
    assert_eq!(vec.to_vec(), vec![1, 10, 20, 2, 3]);
}

#[test]
fn test_splice_replace_beginning() {
    let mut vec = CowVec::from(vec![1, 2, 3, 4, 5]);
    let removed: Vec<i32> = vec.splice(0..2, vec![10]);
    assert_eq!(removed, vec![1, 2]);
    assert_eq!(vec.to_vec(), vec![10, 3, 4, 5]);
}

#[test]
fn test_splice_replace_end() {
    let mut vec = CowVec::from(vec![1, 2, 3, 4, 5]);
    let removed: Vec<i32> = vec.splice(3..5, vec![10, 20, 30]);
    assert_eq!(removed, vec![4, 5]);
    assert_eq!(vec.to_vec(), vec![1, 2, 3, 10, 20, 30]);
}

#[test]
fn test_splice_replace_all() {
    let mut vec = CowVec::from(vec![1, 2, 3]);
    let removed: Vec<i32> = vec.splice(.., vec![10, 20]);
    assert_eq!(removed, vec![1, 2, 3]);
    assert_eq!(vec.to_vec(), vec![10, 20]);
}

#[test]
fn test_splice_inclusive_range() {
    let mut vec = CowVec::from(vec![1, 2, 3, 4, 5]);
    let removed: Vec<i32> = vec.splice(1..=3, vec![10]);
    assert_eq!(removed, vec![2, 3, 4]);
    assert_eq!(vec.to_vec(), vec![1, 10, 5]);
}

#[test]
fn test_splice_does_not_affect_clones() {
    let vec1 = CowVec::from(vec![1, 2, 3, 4, 5]);
    let mut vec2 = vec1.clone();
    vec2.splice(1..3, vec![10, 20]);
    assert_eq!(vec1.to_vec(), vec![1, 2, 3, 4, 5]);
    assert_eq!(vec2.to_vec(), vec![1, 10, 20, 4, 5]);
}

// ============================================================================
// Sharing introspection tests
// ============================================================================

#[test]
fn test_is_structure_shared_fresh_vec() {
    let vec = CowVec::from(vec![1, 2, 3]);
    assert!(!vec.is_structure_shared());
}

#[test]
fn test_is_storage_shared_fresh_vec() {
    let vec = CowVec::from(vec![1, 2, 3]);
    assert!(!vec.is_storage_shared());
}

#[test]
fn test_is_structure_shared_after_clone() {
    let vec1 = CowVec::from(vec![1, 2, 3]);
    let vec2 = vec1.clone();
    assert!(vec1.is_structure_shared());
    assert!(vec2.is_structure_shared());
}

#[test]
fn test_is_storage_shared_after_clone() {
    let vec1 = CowVec::from(vec![1, 2, 3]);
    let vec2 = vec1.clone();
    assert!(vec1.is_storage_shared());
    assert!(vec2.is_storage_shared());
}

#[test]
fn test_is_structure_shared_after_mutation() {
    let vec1 = CowVec::from(vec![1, 2, 3]);
    let mut vec2 = vec1.clone();

    // Before mutation, both share structure
    assert!(vec1.is_structure_shared());
    assert!(vec2.is_structure_shared());

    // Mutation triggers COW on structure
    vec2.push(4);

    // vec2 now has its own structure, vec1's structure is no longer shared
    assert!(!vec1.is_structure_shared());
    assert!(!vec2.is_structure_shared());
}

#[test]
fn test_is_storage_shared_after_mutation() {
    let vec1 = CowVec::from(vec![1, 2, 3]);
    let mut vec2 = vec1.clone();

    // Mutation does NOT affect storage sharing (arena is always shared)
    vec2.push(4);

    assert!(vec1.is_storage_shared());
    assert!(vec2.is_storage_shared());
}

#[test]
fn test_sharing_with_multiple_clones() {
    let vec1 = CowVec::from(vec![1, 2, 3]);
    let vec2 = vec1.clone();
    let mut vec3 = vec1.clone();

    // All three share structure
    assert!(vec1.is_structure_shared());
    assert!(vec2.is_structure_shared());
    assert!(vec3.is_structure_shared());

    // vec3 mutates, gets its own structure
    vec3.push(4);

    // vec1 and vec2 still share structure with each other
    assert!(vec1.is_structure_shared());
    assert!(vec2.is_structure_shared());
    // vec3 has its own unique structure
    assert!(!vec3.is_structure_shared());

    // All three still share storage
    assert!(vec1.is_storage_shared());
    assert!(vec2.is_storage_shared());
    assert!(vec3.is_storage_shared());
}

#[test]
fn test_iter_reverse() {
    let vec = CowVec::from(vec![1, 2, 3, 4, 5]);
    let reversed: Vec<i32> = vec.iter().rev().cloned().collect();
    assert_eq!(reversed, vec![5, 4, 3, 2, 1]);
}

#[test]
fn test_iter_from_both_ends() {
    let vec = CowVec::from(vec![1, 2, 3, 4, 5]);
    let mut iter = vec.iter();
    assert_eq!(iter.next(), Some(&1));
    assert_eq!(iter.next_back(), Some(&5));
    assert_eq!(iter.next(), Some(&2));
    assert_eq!(iter.next_back(), Some(&4));
    assert_eq!(iter.next(), Some(&3));
    assert_eq!(iter.next(), None);
    assert_eq!(iter.next_back(), None);
}

#[test]
fn test_iter_nth_and_len() {
    let vec = CowVec::from(vec![10, 20, 30, 40, 50]);
    let mut iter = vec.iter();
    assert_eq!(iter.len(), 5);
    assert_eq!(iter.nth(2), Some(&30));
    assert_eq!(iter.len(), 2);
    assert_eq!(iter.nth_back(1), Some(&40));
    assert_eq!(iter.len(), 0);
}

#[test]
fn test_iter_clone_is_independent() {
    let vec = CowVec::from(vec![1, 2, 3]);
    let mut a = vec.iter();
    a.next();
    let mut b = a.clone();
    assert_eq!(a.next(), Some(&2));
    assert_eq!(b.next(), Some(&2));
}

#[test]
#[allow(clippy::unnecessary_fold)] // exercises the forwarded fold implementation
fn test_iter_fold_and_rfold() {
    let vec = CowVec::from(vec![1, 2, 3, 4]);
    let sum = vec.iter().fold(0, |acc, &x| acc + x);
    assert_eq!(sum, 10);
    let concat = vec
        .iter()
        .rfold(String::new(), |acc, x| acc + &x.to_string());
    assert_eq!(concat, "4321");
}

#[test]
fn test_iter_count_and_last() {
    let vec = CowVec::from(vec![7, 8, 9]);
    assert_eq!(vec.iter().count(), 3);
    assert_eq!(vec.iter().last(), Some(&9));
}

#[test]
fn test_eq_by_elements() {
    let a = CowVec::from(vec![1, 2, 3]);
    let b = CowVec::from(vec![1, 2, 3]);
    let c = CowVec::from(vec![1, 2, 4]);
    assert_eq!(a, b);
    assert_ne!(a, c);
    assert_ne!(a, CowVec::from(vec![1, 2]));
}

#[test]
fn test_eq_shared_structure_fast_path() {
    let a = CowVec::from(vec![1, 2, 3]);
    let b = a.clone();
    assert_eq!(a, b);
}

#[test]
fn test_eq_against_vec_and_slice() {
    let a = CowVec::from(vec![1, 2, 3]);
    assert_eq!(a, vec![1, 2, 3]);
    assert_eq!(a, *[1, 2, 3].as_slice());
    assert_ne!(a, vec![1, 2]);
}

#[test]
fn test_ord_lexicographic() {
    let a = CowVec::from(vec![1, 2]);
    let b = CowVec::from(vec![1, 3]);
    let c = CowVec::from(vec![1, 2, 0]);
    assert!(a < b);
    assert!(a < c);
    assert!(b > c);
}

#[test]
fn test_hash_consistent_with_eq() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    fn hash_of<T: Hash>(value: &T) -> u64 {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }

    let a = CowVec::from(vec![1, 2, 3]);
    let mut b = a.clone();
    b.set(0, 1); // same value, different arena slot
    assert_eq!(hash_of(&a), hash_of(&b));
}

#[test]
fn test_from_iterator_collect() {
    let vec: CowVec<i32> = (1..=5).collect();
    assert_eq!(vec, vec![1, 2, 3, 4, 5]);
}

#[test]
fn test_from_slice() {
    let source = [String::from("a"), String::from("b")];
    let vec = CowVec::from(source.as_slice());
    assert_eq!(vec.len(), 2);
    assert_eq!(vec[0], "a");
}

#[test]
fn test_extend_trait_from_generic_code() {
    fn fill<E: Extend<i32>>(target: &mut E) {
        target.extend([1, 2, 3]);
    }
    let mut vec = CowVec::new();
    fill(&mut vec);
    assert_eq!(vec, vec![1, 2, 3]);
}

#[test]
fn test_sort_permutes_pointers_only() {
    let mut vec = CowVec::from(vec![3, 1, 4, 1, 5, 9, 2, 6]);
    vec.sort();
    assert_eq!(vec, vec![1, 1, 2, 3, 4, 5, 6, 9]);
}

#[test]
fn test_sort_does_not_affect_clones() {
    let vec1 = CowVec::from(vec![3, 1, 2]);
    let mut vec2 = vec1.clone();
    vec2.sort();
    assert_eq!(vec1, vec![3, 1, 2]);
    assert_eq!(vec2, vec![1, 2, 3]);
}

#[test]
fn test_sort_by_and_unstable() {
    let mut vec = CowVec::from(vec![1, 2, 3, 4]);
    vec.sort_by(|a, b| b.cmp(a));
    assert_eq!(vec, vec![4, 3, 2, 1]);
    vec.sort_unstable();
    assert_eq!(vec, vec![1, 2, 3, 4]);
    vec.sort_unstable_by(|a, b| b.cmp(a));
    assert_eq!(vec, vec![4, 3, 2, 1]);
}

#[test]
fn test_sort_by_key() {
    let mut vec = CowVec::from(vec!["hello", "hi", "hey"]);
    vec.sort_by_key(|s| s.len());
    assert_eq!(vec, vec!["hi", "hey", "hello"]);
}

#[test]
fn test_sort_is_stable() {
    let mut vec = CowVec::from(vec![(1, 'b'), (0, 'a'), (1, 'a'), (0, 'b')]);
    vec.sort_by_key(|&(n, _)| n);
    assert_eq!(vec, vec![(0, 'a'), (0, 'b'), (1, 'b'), (1, 'a')]);
}

#[test]
fn test_rotate() {
    let mut vec = CowVec::from(vec![1, 2, 3, 4, 5]);
    vec.rotate_left(2);
    assert_eq!(vec, vec![3, 4, 5, 1, 2]);
    vec.rotate_right(2);
    assert_eq!(vec, vec![1, 2, 3, 4, 5]);
}

#[test]
fn test_dedup() {
    let mut vec = CowVec::from(vec![1, 1, 2, 2, 2, 3, 1]);
    vec.dedup();
    assert_eq!(vec, vec![1, 2, 3, 1]);
}

#[test]
fn test_dedup_by() {
    let mut vec = CowVec::from(vec!["foo", "FOO", "bar"]);
    vec.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    assert_eq!(vec, vec!["foo", "bar"]);
}

#[test]
fn test_binary_search() {
    let vec = CowVec::from(vec![10, 20, 30, 40]);
    assert_eq!(vec.binary_search(&30), Ok(2));
    assert_eq!(vec.binary_search(&25), Err(2));
    assert_eq!(vec.binary_search(&5), Err(0));
    assert_eq!(vec.binary_search(&50), Err(4));
}

#[test]
fn test_append_copies_pointers_not_elements() {
    let mut a = CowVec::from(vec!["x".to_string(), "y".to_string()]);
    let b = CowVec::from(vec!["z".to_string()]);
    a.append(&b);
    assert_eq!(a.len(), 3);
    assert_eq!(a[2], "z");
    // b is untouched and its element is literally the same allocation.
    assert_eq!(b.len(), 1);
    assert!(std::ptr::eq(&a[2] as *const String, &b[0] as *const String));
}

#[test]
fn test_append_then_drop_source_keeps_values_alive() {
    let mut a = CowVec::from(vec![1]);
    {
        let b = CowVec::from(vec![2, 3]);
        a.append(&b);
    } // b dropped here
    assert_eq!(a, vec![1, 2, 3]);
}

#[test]
fn test_append_self_clone() {
    let mut a = CowVec::from(vec![1, 2]);
    let c = a.clone();
    a.append(&c);
    assert_eq!(a, vec![1, 2, 1, 2]);
    assert_eq!(c, vec![1, 2]);
}

#[test]
fn test_deep_version_chain_drops_iteratively() {
    // Every set() under sharing freezes the active arena and grows the
    // keep-alive chain by one node. A recursive drop would overflow the
    // stack at this depth.
    let n = if cfg!(miri) { 200 } else { 200_000 };
    let mut keep = Vec::new();
    let mut v = CowVec::from(vec![0usize]);
    for i in 0..n {
        keep.push(v.clone());
        v.set(0, i);
    }
    assert_eq!(v[0], n - 1);
    drop(keep);
    drop(v); // must not overflow the stack
}

#[test]
fn test_concurrent_divergence_no_contention() {
    let base = CowVec::from((0..100).collect::<Vec<i32>>());
    let handles: Vec<_> = (0..4)
        .map(|t| {
            let mut branch = base.clone();
            thread::spawn(move || {
                for i in 0..50 {
                    branch.set(i, t * 1000 + i as i32);
                    branch.push(i as i32);
                }
                (branch[0], branch.len())
            })
        })
        .collect();
    for (t, handle) in handles.into_iter().enumerate() {
        let (first, len) = handle.join().unwrap();
        assert_eq!(first, t as i32 * 1000);
        assert_eq!(len, 150);
    }
    // Base is untouched.
    assert_eq!(base[0], 0);
    assert_eq!(base.len(), 100);
}

#[test]
fn test_storage_allocations_counts_garbage() {
    let mut v = CowVec::from(vec![1, 2, 3]);
    assert_eq!(v.storage_allocations(), 3);
    for i in 0..10 {
        v.set(0, i);
    }
    assert_eq!(v.storage_allocations(), 13);
    let compacted = v.clone_compacted(5);
    assert_eq!(compacted.storage_allocations(), 3);
    assert_eq!(compacted, vec![9, 2, 3]);
}

#[test]
fn test_values_outlive_original_after_clone_drop() {
    let v2;
    {
        let v1 = CowVec::from(vec![String::from("alpha"), String::from("beta")]);
        v2 = v1.clone();
    } // v1 dropped; storage kept alive by v2
    assert_eq!(v2[0], "alpha");
    assert_eq!(v2[1], "beta");
}

// The behavior contract every vector type in this crate must satisfy.
super::shared_suite::shared_vec_tests!(shared_behavior, CowVec);
