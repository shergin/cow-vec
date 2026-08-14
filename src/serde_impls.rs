use serde::ser::SerializeSeq;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{CowVec, PagedVec};

impl<T: Serialize> Serialize for CowVec<T> {
    /// Serializes as a sequence, like `Vec`.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.len()))?;
        for item in self {
            seq.serialize_element(item)?;
        }
        seq.end()
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for CowVec<T> {
    /// Deserializes from a sequence, like `Vec`.
    ///
    /// All elements are moved into fresh storage in one bulk allocation.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Vec::<T>::deserialize(deserializer)?.into())
    }
}

impl<T: Serialize, const N: usize> Serialize for PagedVec<T, N> {
    /// Serializes as a sequence, like `Vec`.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.len()))?;
        for item in self {
            seq.serialize_element(item)?;
        }
        seq.end()
    }
}

impl<'de, T: Deserialize<'de>, const N: usize> Deserialize<'de> for PagedVec<T, N> {
    /// Deserializes from a sequence, like `Vec`.
    ///
    /// All elements are moved into fresh storage in one bulk allocation.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Vec::<T>::deserialize(deserializer)?.into())
    }
}

#[cfg(test)]
mod tests {
    use crate::CowVec;

    #[test]
    fn round_trip_through_json() {
        let vec = CowVec::from(vec!["a".to_string(), "b".to_string()]);
        let json = serde_json::to_string(&vec).unwrap();
        assert_eq!(json, r#"["a","b"]"#);
        let back: CowVec<String> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, vec);
    }

    #[test]
    fn serializes_like_vec() {
        let cow = CowVec::from(vec![1, 2, 3]);
        let plain = vec![1, 2, 3];
        assert_eq!(
            serde_json::to_string(&cow).unwrap(),
            serde_json::to_string(&plain).unwrap()
        );
    }

    #[test]
    fn empty_round_trip() {
        let vec: CowVec<i32> = CowVec::new();
        let json = serde_json::to_string(&vec).unwrap();
        assert_eq!(json, "[]");
        let back: CowVec<i32> = serde_json::from_str(&json).unwrap();
        assert!(back.is_empty());
    }
}
