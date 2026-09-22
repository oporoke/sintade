use std::fmt;
use std::marker::PhantomData;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A UUIDv7 newtype ID, phantom-tagged by `T` so `Id<User>` and `Id<Workspace>` are distinct
/// types even though both wrap a `Uuid`.
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub struct Id<T> {
    value: Uuid,
    #[serde(skip)]
    _tag: PhantomData<fn() -> T>,
}

impl<T> Id<T> {
    pub fn new_v7() -> Self {
        Self {
            value: Uuid::now_v7(),
            _tag: PhantomData,
        }
    }

    pub fn from_uuid(value: Uuid) -> Self {
        Self {
            value,
            _tag: PhantomData,
        }
    }

    pub fn into_uuid(self) -> Uuid {
        self.value
    }
}

impl<T> Clone for Id<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Id<T> {}

impl<T> PartialEq for Id<T> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl<T> Eq for Id<T> {}

impl<T> PartialOrd for Id<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for Id<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.value.cmp(&other.value)
    }
}

impl<T> std::hash::Hash for Id<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.value.hash(state);
    }
}

impl<T> fmt::Debug for Id<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Id({})", self.value)
    }
}

impl<T> fmt::Display for Id<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.value)
    }
}

impl<T> FromStr for Id<T> {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::from_uuid(Uuid::from_str(s)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Marker;

    #[test]
    fn roundtrips_through_display_and_from_str() {
        let id = Id::<Marker>::new_v7();
        let parsed: Id<Marker> = id.to_string().parse().expect("valid uuid string");
        assert_eq!(id, parsed);
    }

    #[test]
    fn v7_ids_generated_later_sort_higher() {
        let first = Id::<Marker>::new_v7();
        let second = Id::<Marker>::new_v7();
        assert!(second > first);
    }
}
