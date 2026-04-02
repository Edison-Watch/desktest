# Next Tasks — Medium-Impact Improvements

Items identified during the structural refactoring that are worth addressing but were out of scope for the split.

## 1. Blanket `#![allow(dead_code)]` cleanup

~8 files have `#![allow(dead_code)]` at the top where the code is actively used. Remove the blanket allows and address any actual dead code warnings individually.

## 2. `recording.rs` `format_caption()` decomposition

Mixed concerns (text truncation, layout calculation, magic numbers for font sizes/margins). Extract into smaller functions with named constants.
