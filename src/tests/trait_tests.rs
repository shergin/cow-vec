use crate::{CowVec, CowVector, PagedVec};

/// Exercises the whole trait through a generic bound, so it stays usable
/// without naming either concrete type.
fn exercise<V: CowVector<String>>() {
    let mut base: V = (0..40).map(|i| i.to_string()).collect();
    assert_eq!(base.len(), 40);
    assert_eq!(base.first().map(String::as_str), Some("0"));
    assert_eq!(base.last().map(String::as_str), Some("39"));
    assert_eq!(base.iter().next_back().map(String::as_str), Some("39"));
    assert_eq!(base.iter().len(), 40);

    let mut branch = base.clone();
    assert!(branch.is_structure_shared());
    assert!(branch.is_storage_shared());
    branch.set(3, "three".into());
    branch.make_mut(4).push('!');
    branch.push("forty".into());
    assert_eq!(branch.pop().as_deref(), Some("forty"));
    branch.insert(0, "front".into());
    assert_eq!(branch.remove(0).as_str(), "front");
    assert_eq!(branch[3], "three");
    assert_eq!(branch[4], "4!");
    assert_eq!(base[3], "3");
    assert_eq!(base[4], "4");

    branch.truncate(10);
    assert_eq!(branch.to_vec().len(), 10);
    let allocations = branch.storage_allocations();
    branch.compact(allocations); // under the limit: nothing happens
    assert_eq!(branch.storage_allocations(), allocations);
    let compacted = branch.clone_compacted(0);
    assert_eq!(compacted.storage_allocations(), 10);
    assert_eq!(compacted.to_vec(), branch.to_vec());

    branch.clear();
    assert!(branch.is_empty());
    assert_eq!(branch.get(0), None);

    base.extend(["a".to_string()]);
    assert_eq!(base.len(), 41);
    let fresh = V::new();
    assert!(fresh.is_empty());
    let defaulted = V::default();
    assert!(defaulted.is_empty());
    let from_vec = V::from(vec!["x".to_string()]);
    assert_eq!(from_vec[0], "x");
}

#[test]
fn cow_vec_satisfies_the_trait() {
    exercise::<CowVec<String>>();
}

#[test]
fn paged_vec_satisfies_the_trait() {
    exercise::<PagedVec<String, 1024>>();
    exercise::<PagedVec<String, 8>>();
}
