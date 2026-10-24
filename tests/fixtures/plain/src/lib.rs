//! A crate without any diagram.
//!
//! # Example
//!
//! ```
//! # fn hidden_setup() {}
//! let answer = plain::answer();
//! ## Not hidden: a doubled hash keeps one.
//! assert_eq!(answer, 42);
//! ```
//!
//! ## Details
//!
//! See [`answer`] and [the module](crate::inner) for more.

/// The answer.
pub fn answer() -> u32 {
    42
}

/// Inner module.
pub mod inner {}
