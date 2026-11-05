//! Standalone harness around the production film-gate detector.
//!
//! The detector itself lives in the app crate (`src/film_border.rs`); this
//! crate compiles that exact file so the same code path can be evaluated over
//! a dataset.

#[path = "../../../src/film_border.rs"]
pub mod film_border;

pub use film_border::{
    detect_film_border, film_border_candidates, film_border_ranked_quads, DetectionConfidence, FilmBorderDetection,
};
