use crate::PagedVec;

/// The default page size, spelled concretely: const-generic defaults do not
/// participate in inference, so the suite needs a fully applied alias.
type DefaultPaged<T> = PagedVec<T, 1024>;
/// Tiny pages so every test constantly crosses page boundaries.
type SmallPaged<T> = PagedVec<T, 8>;

// The behavior contract every vector type in this crate must satisfy,
// run at the default page size and at a tiny one.
super::shared_suite::shared_vec_tests!(shared_behavior_default_pages, DefaultPaged);
super::shared_suite::shared_vec_tests!(shared_behavior_small_pages, SmallPaged);

#[test]
fn set_copies_only_the_touched_page() {
    let v1: SmallPaged<i32> = (0..64).collect(); // 8 pages
    let mut v2 = v1.clone();

    v2.set(20, -1); // page 2

    // Only page 2 diverged; all other pages are still the same allocation.
    for page in 0..8 {
        if page == 2 {
            assert_ne!(v1.page_addr(page), v2.page_addr(page));
        } else {
            assert_eq!(v1.page_addr(page), v2.page_addr(page));
        }
    }
    assert_eq!(v1[20], 20);
    assert_eq!(v2[20], -1);
}

#[test]
fn push_copies_only_the_last_page() {
    let v1: SmallPaged<i32> = (0..16).collect(); // 2 full pages
    let mut v2 = v1.clone();

    v2.push(16); // new page 2; pages 0 and 1 stay shared

    assert_eq!(v1.page_addr(0), v2.page_addr(0));
    assert_eq!(v1.page_addr(1), v2.page_addr(1));
    assert_eq!(v2.len(), 17);
    assert_eq!(v1.len(), 16);
}

#[test]
fn push_into_partial_page_diverges_it() {
    let v1: SmallPaged<i32> = (0..12).collect(); // page 1 half full
    let mut v2 = v1.clone();

    v2.push(12); // lands in shared page 1 -> page copied

    assert_eq!(v1.page_addr(0), v2.page_addr(0));
    assert_ne!(v1.page_addr(1), v2.page_addr(1));
    assert_eq!(v1.len(), 12);
    assert_eq!(v2[12], 12);
    // v1's view of page 1 is untouched.
    assert_eq!(v1.to_vec(), (0..12).collect::<Vec<_>>());
}

#[test]
fn pop_copies_nothing() {
    let v1: SmallPaged<i32> = (0..16).collect();
    let mut v2 = v1.clone();

    assert_eq!(v2.pop(), Some(15));

    // Not even the root table diverged.
    assert!(v2.is_structure_shared());
    assert_eq!(v1.page_addr(0), v2.page_addr(0));
    assert_eq!(v1.page_addr(1), v2.page_addr(1));
}

#[test]
fn exact_page_boundary_lengths() {
    for n in [7, 8, 9, 15, 16, 17, 63, 64, 65] {
        let v: SmallPaged<usize> = (0..n).collect();
        assert_eq!(v.len(), n);
        assert_eq!(v.to_vec(), (0..n).collect::<Vec<_>>());
        assert_eq!(v.iter().next_back(), Some(&(n - 1)));
    }
}

#[test]
fn push_pop_across_page_boundary() {
    let mut v: SmallPaged<i32> = (0..8).collect();
    v.push(8); // opens page 1
    assert_eq!(v.len(), 9);
    assert_eq!(v.pop(), Some(8));
    assert_eq!(v.pop(), Some(7)); // back into page 0
    v.push(70);
    assert_eq!(v.to_vec(), vec![0, 1, 2, 3, 4, 5, 6, 70]);
}

#[test]
fn truncate_releases_trailing_pages_when_unshared() {
    let mut v: SmallPaged<i32> = (0..64).collect();
    v.truncate(9);
    assert_eq!(v.to_vec(), (0..9).collect::<Vec<_>>());
    v.push(9);
    assert_eq!(v.len(), 10);
}

#[test]
fn make_mut_diverges_one_page() {
    let v1: SmallPaged<String> = (0..16).map(|i| i.to_string()).collect();
    let mut v2 = v1.clone();
    v2.make_mut(3).push('!');
    assert_eq!(v1[3], "3");
    assert_eq!(v2[3], "3!");
    assert_ne!(v1.page_addr(0), v2.page_addr(0));
    assert_eq!(v1.page_addr(1), v2.page_addr(1));
}

#[test]
fn nth_skips_pages() {
    let v: SmallPaged<usize> = (0..40).collect();
    let mut iter = v.iter();
    assert_eq!(iter.nth(20), Some(&20));
    assert_eq!(iter.next(), Some(&21));
    assert_eq!(iter.nth(100), None);
}

#[test]
fn chained_generations_diverge_page_by_page() {
    // The driving workload in miniature: each generation replaces a few
    // scattered elements; every version stays intact.
    let n_gens = if cfg!(miri) { 20 } else { 200 };
    let mut versions: Vec<SmallPaged<usize>> = vec![(0..256).collect()];
    for g in 1..n_gens {
        let mut next = versions.last().unwrap().clone();
        for k in 0..5 {
            next.set((g * 37 + k * 53) % 256, g * 1000 + k);
        }
        versions.push(next);
    }
    // The first version never changed.
    assert_eq!(versions[0].to_vec(), (0..256).collect::<Vec<_>>());
    // Each later version sees its own last write.
    for g in 1..n_gens {
        assert_eq!(versions[g][(g * 37) % 256], g * 1000);
    }
}

#[cfg(feature = "serde")]
#[test]
fn serde_round_trip() {
    let v: SmallPaged<i32> = (0..20).collect();
    let json = serde_json::to_string(&v).unwrap();
    let back: SmallPaged<i32> = serde_json::from_str(&json).unwrap();
    assert_eq!(back, v);
    // Wire format matches Vec.
    assert_eq!(
        json,
        serde_json::to_string(&(0..20).collect::<Vec<i32>>()).unwrap()
    );
}
