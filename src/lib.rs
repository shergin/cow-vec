#![doc = include_str!("../README.md")]

mod cow_vec;
mod iterator;
mod paged_vec;
#[cfg(feature = "serde")]
mod serde_impls;
mod storage;

pub use cow_vec::CowVec;
pub use iterator::CowVecIter;
pub use paged_vec::{PagedVec, PagedVecIter};

#[cfg(test)]
#[path = "tests"]
mod tests {
    mod shared_suite;

    mod cow_vec_tests;
    mod paged_vec_tests;
}
