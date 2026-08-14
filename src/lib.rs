#![doc = include_str!("../README.md")]

mod cow_vec;
mod iterator;
#[cfg(feature = "serde")]
mod serde_impls;
mod storage;

pub use cow_vec::CowVec;
pub use iterator::CowVecIter;

#[cfg(test)]
#[path = "tests/cow_vec_tests.rs"]
mod tests;
