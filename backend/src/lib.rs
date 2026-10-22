pub mod app;
pub mod domain;

#[doc(hidden)]
pub mod test_support {
    pub use crate::app::test_support::{build_fixture, FixtureKind};
}
