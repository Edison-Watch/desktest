# Next Tasks — Medium-Impact Improvements

Items identified during the structural refactoring that are worth addressing but were out of scope for the split.

## 1. `recording.rs` `format_caption()` decomposition

Mixed concerns (text truncation, layout calculation, magic numbers for font sizes/margins). Extract into smaller functions with named constants.
