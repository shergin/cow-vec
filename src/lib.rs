#![doc = include_str!("../README.md")]

mod cow_vec;
mod iterator;
mod paged_vec;
#[cfg(feature = "serde")]
mod serde_impls;
mod storage;
mod traits;

pub use cow_vec::CowVec;
pub use iterator::{CowVecIntoIter, CowVecIter};
pub use paged_vec::{PagedVec, PagedVecIntoIter, PagedVecIter};
pub use traits::CowVector;

#[cfg(test)]
#[path = "tests"]
mod tests {
    mod shared_suite;

    mod cow_vec_tests;
    mod paged_vec_tests;
    mod trait_tests;
}
