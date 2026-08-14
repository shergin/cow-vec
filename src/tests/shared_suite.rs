//! Behavioral test suite shared by every vector type in this crate.
//!
//! `shared_vec_tests!(module_name, VecType)` generates the same battery of
//! tests for the given type, which must be an identifier (or generic type
//! alias) visible at the invocation site. This is the parity contract:
//! every vector type must pass the identical suite.

macro_rules! shared_vec_tests {
    ($module:ident, $V:ident) => {
        mod $module {
            use std::thread;

            use super::$V;

            #[test]
            fn new_is_empty() {
                let v: $V<i32> = $V::new();
                assert!(v.is_empty());
                assert_eq!(v.len(), 0);
                assert_eq!(v.get(0), None);
            }

            #[test]
            fn default_is_empty() {
                let v: $V<i32> = Default::default();
                assert!(v.is_empty());
            }

            #[test]
            fn from_vec_and_index() {
                let v = $V::from(vec![10, 20, 30]);
                assert_eq!(v.len(), 3);
                assert_eq!(v[0], 10);
                assert_eq!(v[1], 20);
                assert_eq!(v[2], 30);
            }

            #[test]
            #[should_panic(expected = "index out of bounds")]
            fn index_out_of_bounds_panics() {
                let v = $V::from(vec![1]);
                let _ = v[1];
            }

            #[test]
            fn push_and_get() {
                let mut v = $V::new();
                for i in 0..100 {
                    v.push(i);
                }
                assert_eq!(v.len(), 100);
                for i in 0..100 {
                    assert_eq!(v.get(i), Some(&(i)));
                }
                assert_eq!(v.get(100), None);
            }

            #[test]
            fn set_is_copy_on_write() {
                let v1 = $V::from(vec![1, 2, 3]);
                let mut v2 = v1.clone();
                v2.set(0, 100);
                assert_eq!(v1[0], 1);
                assert_eq!(v2[0], 100);
                assert_eq!(v2[1], 2);
            }

            #[test]
            #[should_panic(expected = "index out of bounds")]
            fn set_out_of_bounds_panics() {
                let mut v = $V::from(vec![1]);
                v.set(1, 2);
            }

            #[test]
            fn push_after_clone_is_independent() {
                let v1 = $V::from(vec![1, 2, 3]);
                let mut v2 = v1.clone();
                v2.push(4);
                assert_eq!(v1.len(), 3);
                assert_eq!(v2.len(), 4);
                assert_eq!(v2[3], 4);
            }

            #[test]
            fn make_mut_is_copy_on_write() {
                let v1 = $V::from(vec![String::from("a")]);
                let mut v2 = v1.clone();
                v2.make_mut(0).push('b');
                assert_eq!(v1[0], "a");
                assert_eq!(v2[0], "ab");
            }

            #[test]
            fn pop_returns_owned_values() {
                let mut v = $V::from(vec![1, 2, 3]);
                assert_eq!(v.pop(), Some(3));
                assert_eq!(v.pop(), Some(2));
                assert_eq!(v.len(), 1);
                assert_eq!(v.pop(), Some(1));
                assert_eq!(v.pop(), None);
            }

            #[test]
            fn pop_does_not_affect_clones() {
                let v1 = $V::from(vec![1, 2, 3]);
                let mut v2 = v1.clone();
                v2.pop();
                assert_eq!(v1.len(), 3);
                assert_eq!(v2.len(), 2);
            }

            #[test]
            fn push_after_pop_overwrites() {
                let mut v = $V::from(vec![1, 2, 3]);
                v.pop();
                v.push(30);
                assert_eq!(v.to_vec(), vec![1, 2, 30]);
            }

            #[test]
            fn truncate_and_clear() {
                let mut v = $V::from(vec![1, 2, 3, 4, 5]);
                v.truncate(3);
                assert_eq!(v.to_vec(), vec![1, 2, 3]);
                v.truncate(10);
                assert_eq!(v.len(), 3);
                v.clear();
                assert!(v.is_empty());
            }

            #[test]
            fn truncate_does_not_affect_clones() {
                let v1 = $V::from(vec![1, 2, 3]);
                let mut v2 = v1.clone();
                v2.truncate(1);
                assert_eq!(v1.len(), 3);
                assert_eq!(v2.len(), 1);
            }

            #[test]
            fn extend_trait_works() {
                let mut v = $V::from(vec![1]);
                v.extend([2, 3, 4]);
                assert_eq!(v.to_vec(), vec![1, 2, 3, 4]);
            }

            #[test]
            fn collect_from_iterator() {
                let v: $V<i32> = (0..10).collect();
                assert_eq!(v.len(), 10);
                assert_eq!(v[9], 9);
            }

            #[test]
            fn iterates_forward_and_backward() {
                let v = $V::from(vec![1, 2, 3, 4, 5]);
                let forward: Vec<i32> = v.iter().cloned().collect();
                assert_eq!(forward, vec![1, 2, 3, 4, 5]);
                let backward: Vec<i32> = v.iter().rev().cloned().collect();
                assert_eq!(backward, vec![5, 4, 3, 2, 1]);
            }

            #[test]
            fn iterator_is_exact_size() {
                let v = $V::from(vec![1, 2, 3]);
                let mut iter = v.iter();
                assert_eq!(iter.len(), 3);
                iter.next();
                assert_eq!(iter.len(), 2);
                assert_eq!(iter.size_hint(), (2, Some(2)));
            }

            #[test]
            fn into_iterator_for_ref() {
                let v = $V::from(vec![1, 2, 3]);
                let mut sum = 0;
                for &item in &v {
                    sum += item;
                }
                assert_eq!(sum, 6);
            }

            #[test]
            fn first_and_last() {
                let v = $V::from(vec![1, 2, 3]);
                assert_eq!(v.first(), Some(&1));
                assert_eq!(v.last(), Some(&3));
                let empty: $V<i32> = $V::new();
                assert_eq!(empty.first(), None);
                assert_eq!(empty.last(), None);
            }

            #[test]
            fn contains_value() {
                let v = $V::from(vec![1, 2, 3]);
                assert!(v.contains(&2));
                assert!(!v.contains(&4));
            }

            #[test]
            fn equality_with_self_and_vec() {
                let a = $V::from(vec![1, 2, 3]);
                let b = $V::from(vec![1, 2, 3]);
                let c = a.clone();
                assert_eq!(a, b);
                assert_eq!(a, c);
                assert_eq!(a, vec![1, 2, 3]);
                assert_ne!(a, $V::from(vec![1, 2]));
                assert_ne!(a, $V::from(vec![1, 2, 4]));
            }

            #[test]
            fn ordering_is_lexicographic() {
                let a = $V::from(vec![1, 2]);
                let b = $V::from(vec![1, 3]);
                let c = $V::from(vec![1, 2, 0]);
                assert!(a < b);
                assert!(a < c);
            }

            #[test]
            fn hash_matches_equality() {
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                fn h<T: Hash>(value: &T) -> u64 {
                    let mut hasher = DefaultHasher::new();
                    value.hash(&mut hasher);
                    hasher.finish()
                }
                let a = $V::from(vec![1, 2, 3]);
                let mut b = a.clone();
                b.set(0, 1); // same value, different storage slot
                assert_eq!(h(&a), h(&b));
            }

            #[test]
            fn debug_formats_like_a_list() {
                let v = $V::from(vec![1, 2, 3]);
                assert_eq!(format!("{:?}", v), "[1, 2, 3]");
            }

            #[test]
            fn to_vec_round_trip() {
                let source = vec![String::from("x"), String::from("y"), String::from("z")];
                let v = $V::from(source.clone());
                assert_eq!(v.to_vec(), source);
            }

            #[test]
            fn clone_compacted_reclaims_garbage() {
                let mut v = $V::from(vec![1, 2, 3]);
                for i in 0..20 {
                    v.set(0, i);
                }
                assert_eq!(v.storage_allocations(), 23);
                let compacted = v.clone_compacted(10);
                assert_eq!(compacted.storage_allocations(), 3);
                assert_eq!(compacted, v);
                // Under the limit, it behaves like clone().
                let cheap = compacted.clone_compacted(10);
                assert_eq!(cheap.storage_allocations(), 3);
            }

            #[test]
            fn storage_sharing_is_reported() {
                let v1 = $V::from(vec![1, 2, 3]);
                assert!(!v1.is_storage_shared());
                let v2 = v1.clone();
                assert!(v1.is_storage_shared());
                assert!(v2.is_storage_shared());
                assert!(v1.is_structure_shared());
            }

            #[test]
            fn values_survive_original_drop() {
                let survivor;
                {
                    let original = $V::from(vec![String::from("alpha"), String::from("beta")]);
                    survivor = original.clone();
                }
                assert_eq!(survivor[0], "alpha");
                assert_eq!(survivor[1], "beta");
            }

            #[test]
            fn bulk_load_large() {
                let n = if cfg!(miri) { 300 } else { 10_000 };
                let v = $V::from((0..n as i32).collect::<Vec<_>>());
                assert_eq!(v.len(), n);
                assert_eq!(v[n - 1], n as i32 - 1);
                let sum: i64 = v.iter().map(|&x| x as i64).sum();
                assert_eq!(sum, (n as i64 - 1) * n as i64 / 2);
            }

            #[test]
            fn many_generations_stay_consistent() {
                let n = if cfg!(miri) { 50 } else { 1_000 };
                let mut versions = vec![$V::from(vec![0usize; 4])];
                for i in 1..n {
                    let mut next = versions.last().unwrap().clone();
                    next.set(i % 4, i);
                    versions.push(next);
                }
                // Spot-check: each version still sees its own values.
                assert_eq!(versions[0].to_vec(), vec![0, 0, 0, 0]);
                let last = versions.last().unwrap();
                assert_eq!(last[(n - 1) % 4], n - 1);
            }

            #[test]
            fn concurrent_branches_are_independent() {
                let base = $V::from((0..64).collect::<Vec<i32>>());
                let handles: Vec<_> = (0..4)
                    .map(|t| {
                        let mut branch = base.clone();
                        thread::spawn(move || {
                            for i in 0..32 {
                                branch.set(i, t * 100 + i as i32);
                            }
                            branch.push(-1);
                            (branch[0], branch.len())
                        })
                    })
                    .collect();
                for (t, handle) in handles.into_iter().enumerate() {
                    let (first, len) = handle.join().unwrap();
                    assert_eq!(first, t as i32 * 100);
                    assert_eq!(len, 65);
                }
                assert_eq!(base[0], 0);
                assert_eq!(base.len(), 64);
            }
        }
    };
}

pub(crate) use shared_vec_tests;
