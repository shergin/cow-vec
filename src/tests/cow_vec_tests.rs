use std::thread;

use crate::tests::shared_suite::Counters;
use crate::CowVec;

#[test]
fn test_with_capacity() {
    let vec: CowVec<i32> = CowVec::with_capacity(100);
    assert!(vec.is_empty());
    assert_eq!(vec.len(), 0);
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
fn append_of_related_vector_disables_moves_until_compacted() {
    let counters = Counters::new();
    let mut v = CowVec::from(vec![counters.tracked(1)]);
    let twin = v.clone();
    v.append(&twin); // the same value now sits at index 0 and 1
    drop(twin);

    assert_eq!(v.pop().map(|t| t.value), Some(1));
    assert_eq!(counters.clones(), 1, "aliased storage must clone");
    assert_eq!(v[0].value, 1);

    v.compact(0);
    assert_eq!(
        counters.clones(),
        2,
        "aliased storage is rebuilt by cloning"
    );
    assert_eq!(v.pop().map(|t| t.value), Some(1));
    assert_eq!(counters.clones(), 2, "fresh storage owns its values again");
}

#[test]
fn append_of_unrelated_vector_keeps_moves_once_it_is_gone() {
    let counters = Counters::new();
    let mut v = CowVec::from(vec![counters.tracked(1)]);
    let other = CowVec::from(vec![counters.tracked(2)]);
    v.append(&other);

    // `other` still owns its arena, so its value has to be cloned out.
    assert_eq!(v.pop().map(|t| t.value), Some(2));
    assert_eq!(counters.clones(), 1);

    drop(other);
    v.push(counters.tracked(3));
    assert_eq!(v.pop().map(|t| t.value), Some(3));
    assert_eq!(v.pop().map(|t| t.value), Some(1));
    assert_eq!(counters.clones(), 1, "everything moved once other was gone");
}

#[test]
fn split_off_shares_storage_until_one_side_is_gone() {
    let counters = Counters::new();
    let mut v = CowVec::from((0..4).map(|i| counters.tracked(i)).collect::<Vec<_>>());
    let tail = v.split_off(2);
    assert_eq!(v.pop().map(|t| t.value), Some(1));
    assert_eq!(counters.clones(), 1);
    drop(tail);
    assert_eq!(v.pop().map(|t| t.value), Some(0));
    assert_eq!(counters.clones(), 1);
}

#[test]
fn remove_splice_retain_dedup_move_or_drop_when_owned() {
    let counters = Counters::new();
    let mut v = CowVec::from((0..6).map(|i| counters.tracked(i)).collect::<Vec<_>>());
    assert_eq!(v.remove(0).value, 0);
    let removed = v.splice(0..2, [counters.tracked(10)]);
    assert_eq!(
        removed.iter().map(|t| t.value).collect::<Vec<_>>(),
        vec![1, 2]
    );
    drop(removed);
    assert_eq!(counters.drops(), 3);
    v.retain(|t| t.value != 3);
    assert_eq!(counters.drops(), 4);
    v.push(counters.tracked(5));
    v.dedup();
    assert_eq!(counters.drops(), 5);
    assert_eq!(
        v.iter().map(|t| t.value).collect::<Vec<_>>(),
        vec![10, 4, 5]
    );
    assert_eq!(counters.clones(), 0);
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
#[should_panic(expected = "index out of bounds")]
fn test_make_mut_out_of_bounds() {
    let mut vec = CowVec::from(vec![1, 2, 3]);
    *vec.make_mut(3) = 100;
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
fn test_from_slice() {
    let source = [String::from("a"), String::from("b")];
    let vec = CowVec::from(source.as_slice());
    assert_eq!(vec.len(), 2);
    assert_eq!(vec[0], "a");
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

// The behavior contract every vector type in this crate must satisfy.
super::shared_suite::shared_vec_tests!(shared_behavior, CowVec);
